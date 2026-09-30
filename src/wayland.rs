use std::{
    io,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, Interest, Mode, PostAction, generic::Generic},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{
            AxisScroll, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, PointerEvent, PointerEventKind,
            PointerHandler,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

#[cfg(mhypr_module = "active_window")]
use crate::active_window_popup::{ActiveWindowPopupModel, WindowListScope};
#[cfg(mhypr_module = "audio")]
use crate::audio_popup::AudioPopupModel;
#[cfg(mhypr_module = "battery")]
use crate::battery_popup::BatteryPopupModel;
#[cfg(mhypr_module = "brightness")]
use crate::brightness_popup::BrightnessPopupModel;
#[cfg(mhypr_module = "clock")]
use crate::clock_popup::ClockPopupModel;
#[cfg(mhypr_module = "cpu")]
use crate::cpu_popup::CpuPopupModel;
#[cfg(mhypr_module = "disk")]
use crate::disk_popup::DiskPopupModel;
#[cfg(mhypr_module = "gpu")]
use crate::gpu_popup::{GpuPopupConfig, GpuPopupModel};
#[cfg(mhypr_module = "layout")]
use crate::layout_popup::LayoutPopupModel;
#[cfg(mhypr_module = "memory")]
use crate::memory_popup::MemoryPopupModel;
#[cfg(mhypr_module = "monitor")]
use crate::monitor_popup::{MonitorPopupModel, context_action};
#[cfg(mhypr_module = "network")]
use crate::network_popup::NetworkPopupModel;
use crate::{
    config::{BarConfig, ModuleStyle},
    hyprland::{self, MonitorState, Snapshot},
    ipc::{self, Request},
    modules::{ModuleManager, ModuleVisual},
    render::{self, Renderer},
    tray::TrayState,
};
#[cfg(mhypr_module = "tray")]
use crate::{
    tray::TrayMenuRequest,
    tray_popup::{TrayPopupClick, TrayPopupModel},
};

pub fn run(config: BarConfig) -> Result<()> {
    let (control_listener, _socket_guard) = ipc::bind_listener()?;
    let background = config.background_rgba()?;
    let mut modules = ModuleManager::load()?;
    modules.refresh_due();

    #[cfg(mhypr_module = "tray")]
    let tray = match TrayState::new() {
        Ok(tray) => Some(tray),
        Err(error) => {
            eprintln!("mhyprbar: tray disabled at runtime: {error:#}");
            None
        }
    };
    #[cfg(not(mhypr_module = "tray"))]
    let tray: Option<TrayState> = None;

    let tray_fd = tray.as_ref().and_then(|tray| match tray.duplicate_fd() {
        Ok(fd) => Some(fd),
        Err(error) => {
            eprintln!("mhyprbar: failed to register tray D-Bus fd: {error:#}");
            None
        }
    });
    modules.set_width_override("tray", Some(tray.as_ref().map_or(0, TrayState::width)));

    let hyprland = Snapshot::refresh().context("failed to read initial Hyprland state")?;
    let hypr_events = hyprland::event_stream()?;

    let conn = Connection::connect_to_env().context("failed to connect to Wayland compositor")?;
    let (globals, event_queue) =
        registry_queue_init(&conn).context("failed to enumerate Wayland globals")?;
    let qh = event_queue.handle();

    let mut event_loop: EventLoop<App> =
        EventLoop::try_new().context("failed to create event loop")?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .context("failed to register Wayland event source")?;

    let compositor =
        CompositorState::bind(&globals, &qh).context("wl_compositor is unavailable")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell is unavailable")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm is unavailable")?;
    let pool = SlotPool::new(4, &shm).context("failed to create Wayland SHM pool")?;

    let mut app = App {
        compositor,
        layer_shell,
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        bars: Vec::new(),
        pointer: None,
        config,
        background,
        modules,
        renderer: Renderer::new(),
        hyprland,
        tray,
        tray_hover: None,
        tooltip: None,
        #[cfg(mhypr_module = "active_window")]
        active_window_popup: None,
        #[cfg(mhypr_module = "audio")]
        audio_popup: None,
        #[cfg(mhypr_module = "battery")]
        battery_popup: None,
        #[cfg(mhypr_module = "brightness")]
        brightness_popup: None,
        #[cfg(mhypr_module = "clock")]
        clock_popup: None,
        #[cfg(mhypr_module = "cpu")]
        cpu_popup: None,
        #[cfg(mhypr_module = "disk")]
        disk_popup: None,
        #[cfg(mhypr_module = "gpu")]
        gpu_popup: None,
        #[cfg(mhypr_module = "layout")]
        layout_popup: None,
        #[cfg(mhypr_module = "memory")]
        memory_popup: None,
        #[cfg(mhypr_module = "monitor")]
        monitor_popup: None,
        #[cfg(mhypr_module = "monitor")]
        monitor_context: None,
        #[cfg(mhypr_module = "network")]
        network_popup: None,
        #[cfg(mhypr_module = "tray")]
        tray_popup: None,
        exit: false,
    };

    if let Some(tray_fd) = tray_fd {
        event_loop
            .handle()
            .insert_source(
                Generic::new(tray_fd, Interest::READ, Mode::Level),
                move |_, _, app| {
                    let changed = match app.tray.as_mut() {
                        Some(tray) => match tray.poll() {
                            Ok(changed) => changed,
                            Err(error) => {
                                eprintln!("mhyprbar: tray event processing failed: {error:#}");
                                false
                            }
                        },
                        None => false,
                    };
                    if changed {
                        if app.tray_popup_item_gone() {
                            app.close_tray_popup();
                        }
                        app.sync_tray_width();
                        app.refresh_tray_hover_after_change();
                        app.draw_all();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .context("failed to register tray D-Bus fd")?;
    }

    let mut event_buffer = String::new();
    event_loop
        .handle()
        .insert_source(
            Generic::new(hypr_events, Interest::READ, Mode::Level),
            move |_, stream, app| match hyprland::read_event_batch(
                stream.as_ref(),
                &mut event_buffer,
            ) {
                Ok(batch) => {
                    if batch.state_changed
                        && let Err(error) = app.refresh_hyprland()
                    {
                        eprintln!("mhyprbar: failed to refresh Hyprland state: {error:#}");
                    }

                    if batch.active_window_changed && app.modules.force_refresh("active_window") {
                        app.draw_all();
                    }

                    Ok(PostAction::Continue)
                }
                Err(error) => {
                    eprintln!("mhyprbar: Hyprland event stream stopped: {error:#}");
                    Ok(PostAction::Remove)
                }
            },
        )
        .context("failed to register Hyprland event socket")?;

    let control_qh = qh.clone();
    event_loop
        .handle()
        .insert_source(
            Generic::new(control_listener, Interest::READ, Mode::Level),
            move |_, listener, app| {
                loop {
                    match listener.as_ref().accept() {
                        Ok((mut stream, _)) => {
                            let response = match ipc::read_request(&mut stream) {
                                Ok(Some(Request::Reload)) => match app.reload_config() {
                                    Ok(()) => "ok\n".to_owned(),
                                    Err(error) => format!("error: {error:#}\n"),
                                },
                                Ok(Some(Request::Status)) => app.status_text(),
                                Ok(Some(Request::CpuPopupToggle)) => {
                                    #[cfg(mhypr_module = "cpu")]
                                    {
                                        match app.toggle_cpu_popup_first(&control_qh) {
                                            Ok(()) => "ok\n".to_owned(),
                                            Err(error) => format!("error: {error:#}\n"),
                                        }
                                    }
                                    #[cfg(not(mhypr_module = "cpu"))]
                                    {
                                        "error: cpu module is not compiled\n".to_owned()
                                    }
                                }
                                Ok(Some(Request::TrayList)) => app.tray_list_text(),
                                Ok(Some(Request::TrayMenuOpen { index })) => {
                                    match app.open_tray_popup_index(&control_qh, index) {
                                        Ok(()) => "ok\n".to_owned(),
                                        Err(error) => format!("error: {error:#}\n"),
                                    }
                                }
                                Ok(Some(Request::TrayTooltipOpen { index })) => {
                                    match app.force_tray_tooltip(index) {
                                        Ok(()) => "ok\n".to_owned(),
                                        Err(error) => format!("error: {error:#}\n"),
                                    }
                                }
                                Ok(Some(Request::Quit)) => {
                                    app.exit = true;
                                    "ok\n".to_owned()
                                }
                                Ok(None) => "error: unknown request\n".to_owned(),
                                Err(error) => format!("error: {error:#}\n"),
                            };
                            if let Err(error) = ipc::write_response(&mut stream, &response) {
                                eprintln!("mhyprbar: control response failed: {error:#}");
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(error) => {
                            eprintln!("mhyprbar: control accept failed: {error}");
                            break;
                        }
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .context("failed to register control socket")?;

    while !app.exit {
        let mut timeout = app.modules.next_timeout();
        if let Some(tooltip_timeout) = app.tray_hover_timeout() {
            timeout = timeout.min(tooltip_timeout);
        }
        #[cfg(mhypr_module = "audio")]
        if let Some(audio_timeout) = app.audio_popup_timeout() {
            timeout = timeout.min(audio_timeout);
        }
        #[cfg(mhypr_module = "battery")]
        if let Some(battery_timeout) = app.battery_popup_timeout() {
            timeout = timeout.min(battery_timeout);
        }
        #[cfg(mhypr_module = "brightness")]
        if let Some(brightness_timeout) = app.brightness_popup_timeout() {
            timeout = timeout.min(brightness_timeout);
        }
        #[cfg(mhypr_module = "clock")]
        if let Some(clock_timeout) = app.clock_popup_timeout() {
            timeout = timeout.min(clock_timeout);
        }
        #[cfg(mhypr_module = "cpu")]
        if let Some(cpu_timeout) = app.cpu_popup_timeout() {
            timeout = timeout.min(cpu_timeout);
        }
        #[cfg(mhypr_module = "disk")]
        if let Some(disk_timeout) = app.disk_popup_timeout() {
            timeout = timeout.min(disk_timeout);
        }
        #[cfg(mhypr_module = "gpu")]
        if let Some(gpu_timeout) = app.gpu_popup_timeout() {
            timeout = timeout.min(gpu_timeout);
        }
        #[cfg(mhypr_module = "monitor")]
        if let Some(monitor_timeout) = app.monitor_popup_timeout() {
            timeout = timeout.min(monitor_timeout);
        }
        #[cfg(mhypr_module = "memory")]
        if let Some(memory_timeout) = app.memory_popup_timeout() {
            timeout = timeout.min(memory_timeout);
        }
        event_loop
            .dispatch(Some(timeout), &mut app)
            .context("event loop dispatch failed")?;
        if app.modules.refresh_due() {
            app.draw_all();
        }
        app.maybe_show_tray_tooltip(&qh);
        #[cfg(mhypr_module = "audio")]
        app.refresh_audio_popup_if_due();
        #[cfg(mhypr_module = "battery")]
        app.refresh_battery_popup_if_due();
        #[cfg(mhypr_module = "brightness")]
        app.refresh_brightness_popup_if_due();
        #[cfg(mhypr_module = "clock")]
        app.refresh_clock_popup_if_due();
        #[cfg(mhypr_module = "cpu")]
        app.refresh_cpu_popup_if_due();
        #[cfg(mhypr_module = "disk")]
        app.refresh_disk_popup_if_due();
        #[cfg(mhypr_module = "gpu")]
        app.refresh_gpu_popup_if_due();
        #[cfg(mhypr_module = "monitor")]
        app.refresh_monitor_popup_if_due();
        #[cfg(mhypr_module = "memory")]
        app.refresh_memory_popup_if_due();
    }

    Ok(())
}

struct BarSurface {
    output: wl_output::WlOutput,
    output_name: Option<String>,
    layer: LayerSurface,
    width: u32,
    height: u32,
    configured: bool,
}

struct TrayHover {
    bar_index: usize,
    item_index: usize,
    item_key: String,
    local_x: f64,
    deadline: Instant,
    text: String,
}

struct TooltipSurface {
    layer: LayerSurface,
    text: String,
    style: ModuleStyle,
    width: u32,
    height: u32,
    configured: bool,
    bar_index: usize,
    item_index: Option<usize>,
}

#[cfg(mhypr_module = "battery")]
struct BatteryPopupSurface {
    layer: LayerSurface,
    model: BatteryPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
}

#[cfg(mhypr_module = "clock")]
struct ClockPopupSurface {
    layer: LayerSurface,
    model: ClockPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
}

#[cfg(mhypr_module = "cpu")]
struct CpuPopupSurface {
    layer: LayerSurface,
    model: CpuPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
}

#[cfg(mhypr_module = "gpu")]
struct GpuPopupSurface {
    layer: LayerSurface,
    model: GpuPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
}

#[cfg(mhypr_module = "monitor")]
struct MonitorPopupSurface {
    layer: LayerSurface,
    model: MonitorPopupModel,
    output: wl_output::WlOutput,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
}

#[cfg(mhypr_module = "monitor")]
struct MonitorContextSurface {
    layer: LayerSurface,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    panel_width: i32,
    row_height: i32,
    hovered_action: Option<usize>,
}

#[cfg(mhypr_module = "active_window")]
struct ActiveWindowPopupSurface {
    layer: LayerSurface,
    model: ActiveWindowPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
}

#[cfg(mhypr_module = "audio")]
struct AudioPopupSurface {
    layer: LayerSurface,
    model: AudioPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
    dragging_volume: Option<usize>,
}

#[cfg(mhypr_module = "brightness")]
struct BrightnessPopupSurface {
    layer: LayerSurface,
    model: BrightnessPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
    dragging_brightness: Option<usize>,
}

#[cfg(mhypr_module = "layout")]
struct LayoutPopupSurface {
    layer: LayerSurface,
    model: LayoutPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
}

#[cfg(mhypr_module = "network")]
struct NetworkPopupSurface {
    layer: LayerSurface,
    model: NetworkPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
}

#[cfg(mhypr_module = "disk")]
struct DiskPopupSurface {
    layer: LayerSurface,
    model: DiskPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
}

#[cfg(mhypr_module = "memory")]
struct MemoryPopupSurface {
    layer: LayerSurface,
    model: MemoryPopupModel,
    width: u32,
    height: u32,
    configured: bool,
    panel_x: f64,
    panel_y: f64,
    next_refresh: Instant,
}

#[cfg(mhypr_module = "tray")]
struct TrayPopupSurface {
    layer: LayerSurface,
    item_id: rustsni::ItemId,
    menu: TrayPopupModel,
    width: u32,
    height: u32,
    configured: bool,
}

#[derive(Clone, Copy)]
enum TrayPointerAction {
    Primary,
    Secondary,
    ScrollHorizontal(i32),
    ScrollVertical(i32),
}

fn axis_scroll_delta(scroll: AxisScroll) -> i32 {
    if scroll.value120 != 0 {
        scroll.value120
    } else if scroll.discrete != 0 {
        scroll.discrete.saturating_mul(120)
    } else if scroll.absolute != 0.0 {
        scroll
            .absolute
            .round()
            .clamp(i32::MIN as f64, i32::MAX as f64) as i32
    } else {
        0
    }
}

struct App {
    compositor: CompositorState,
    layer_shell: LayerShell,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    bars: Vec<BarSurface>,
    pointer: Option<wl_pointer::WlPointer>,
    config: BarConfig,
    background: [u8; 4],
    modules: ModuleManager,
    renderer: Renderer,
    hyprland: Snapshot,
    tray: Option<TrayState>,
    tray_hover: Option<TrayHover>,
    tooltip: Option<TooltipSurface>,
    #[cfg(mhypr_module = "active_window")]
    active_window_popup: Option<ActiveWindowPopupSurface>,
    #[cfg(mhypr_module = "audio")]
    audio_popup: Option<AudioPopupSurface>,
    #[cfg(mhypr_module = "battery")]
    battery_popup: Option<BatteryPopupSurface>,
    #[cfg(mhypr_module = "brightness")]
    brightness_popup: Option<BrightnessPopupSurface>,
    #[cfg(mhypr_module = "clock")]
    clock_popup: Option<ClockPopupSurface>,
    #[cfg(mhypr_module = "cpu")]
    cpu_popup: Option<CpuPopupSurface>,
    #[cfg(mhypr_module = "disk")]
    disk_popup: Option<DiskPopupSurface>,
    #[cfg(mhypr_module = "gpu")]
    gpu_popup: Option<GpuPopupSurface>,
    #[cfg(mhypr_module = "layout")]
    layout_popup: Option<LayoutPopupSurface>,
    #[cfg(mhypr_module = "memory")]
    memory_popup: Option<MemoryPopupSurface>,
    #[cfg(mhypr_module = "monitor")]
    monitor_popup: Option<MonitorPopupSurface>,
    #[cfg(mhypr_module = "monitor")]
    monitor_context: Option<MonitorContextSurface>,
    #[cfg(mhypr_module = "network")]
    network_popup: Option<NetworkPopupSurface>,
    #[cfg(mhypr_module = "tray")]
    tray_popup: Option<TrayPopupSurface>,
    exit: bool,
}

impl App {
    fn add_output(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if self.bars.iter().any(|bar| bar.output == output) {
            return;
        }

        let output_name = self.output_state.info(&output).and_then(|info| info.name);
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("mhyprbar"),
            Some(&output),
        );

        let anchor = if self.config.position == "bottom" {
            Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT
        } else {
            Anchor::TOP | Anchor::LEFT | Anchor::RIGHT
        };
        layer.set_anchor(anchor);
        layer.set_size(0, self.config.height);
        layer.set_margin(
            self.config.margin_top,
            self.config.margin_right,
            self.config.margin_bottom,
            self.config.margin_left,
        );
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(if self.config.exclusive_zone {
            self.config.height as i32
        } else {
            0
        });
        layer.commit();

        self.bars.push(BarSurface {
            output,
            output_name,
            layer,
            width: 1,
            height: self.config.height,
            configured: false,
        });
    }

    fn update_output_name(&mut self, output: &wl_output::WlOutput) {
        let name = self.output_state.info(output).and_then(|info| info.name);
        if let Some(bar) = self.bars.iter_mut().find(|bar| &bar.output == output) {
            bar.output_name = name;
        }
    }

    fn remove_output(&mut self, output: &wl_output::WlOutput) {
        self.clear_tray_hover();
        #[cfg(mhypr_module = "active_window")]
        self.close_active_window_popup();
        #[cfg(mhypr_module = "audio")]
        self.close_audio_popup();
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "brightness")]
        self.close_brightness_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "layout")]
        self.close_layout_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();
        self.bars.retain(|bar| &bar.output != output);
    }

    fn refresh_hyprland(&mut self) -> Result<()> {
        self.hyprland = Snapshot::refresh()?;
        self.draw_all();
        Ok(())
    }

    fn sync_tray_width(&mut self) {
        let width = self.tray.as_ref().map_or(0, TrayState::width);
        let _ = self.modules.set_width_override("tray", Some(width));
    }

    fn tray_hover_timeout(&self) -> Option<Duration> {
        let hover = self.tray_hover.as_ref()?;
        if self.tooltip.as_ref().is_some_and(|tooltip| {
            tooltip.bar_index == hover.bar_index && tooltip.item_index == Some(hover.item_index)
        }) {
            return None;
        }
        Some(hover.deadline.saturating_duration_since(Instant::now()))
    }

    fn update_tray_hover(&mut self, bar_index: usize, x: f64) {
        let Some(bar) = self.bars.get(bar_index) else {
            self.clear_tray_hover();
            return;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            self.clear_tray_hover();
            return;
        };
        if hit.name != "tray" {
            self.clear_tray_hover();
            return;
        }

        let Some(tray) = self.tray.as_ref() else {
            self.clear_tray_hover();
            return;
        };
        if !tray.tooltip_enabled() {
            self.clear_tray_hover();
            return;
        }
        let Some(item_index) = tray.item_index_at(hit.offset_x) else {
            self.clear_tray_hover();
            return;
        };
        let Some(item_key) = tray.item_key_for_index(item_index).map(str::to_owned) else {
            self.clear_tray_hover();
            return;
        };
        let Some(text) = tray.tooltip_text_for_index(item_index) else {
            self.clear_tray_hover();
            return;
        };
        let delay = tray.tooltip_delay();

        if let Some(hover) = self.tray_hover.as_mut()
            && hover.bar_index == bar_index
            && hover.item_key == item_key
        {
            let content_changed = hover.text != text || hover.item_index != item_index;
            hover.item_index = item_index;
            hover.local_x = x;
            hover.text = text;
            if content_changed {
                self.tooltip = None;
            }
            return;
        }

        self.tooltip = None;
        self.tray_hover = Some(TrayHover {
            bar_index,
            item_index,
            item_key,
            local_x: x,
            deadline: Instant::now() + delay,
            text,
        });
    }

    fn refresh_tray_hover_after_change(&mut self) {
        let Some((bar_index, local_x)) = self
            .tray_hover
            .as_ref()
            .map(|hover| (hover.bar_index, hover.local_x))
        else {
            return;
        };
        self.update_tray_hover(bar_index, local_x);
    }

    fn clear_tray_hover(&mut self) {
        self.tray_hover = None;
        if self
            .tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.item_index.is_some())
        {
            self.tooltip = None;
        }
    }

    #[cfg(mhypr_module = "layout")]
    fn clear_layout_tooltip(&mut self) {
        if self
            .tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.item_index.is_none())
        {
            self.tooltip = None;
        }
    }

    #[cfg(mhypr_module = "layout")]
    fn update_layout_tooltip(&mut self, qh: &QueueHandle<Self>, bar_index: usize, x: f64) {
        let Some(bar) = self.bars.get(bar_index) else {
            self.clear_layout_tooltip();
            return;
        };
        let bar_width = bar.width;
        let bar_height = bar.height as i32;
        let output = bar.output.clone();
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar_width, workspace_visible, &self.config, &self.modules)
        else {
            self.clear_layout_tooltip();
            return;
        };
        if hit.name != "layout" {
            self.clear_layout_tooltip();
            return;
        }

        let Some(view) = self.modules.view("layout") else {
            self.clear_layout_tooltip();
            return;
        };
        let ModuleVisual::Layout(layout) = view.visual else {
            self.clear_layout_tooltip();
            return;
        };
        let text = layout.tooltip;
        if self.tooltip.as_ref().is_some_and(|tooltip| {
            tooltip.bar_index == bar_index && tooltip.item_index.is_none() && tooltip.text == text
        }) {
            return;
        }

        let mut style = view.style.clone();
        style.background = "#202020EE".into();
        style.font_size = 12.0;
        style.padding_x = 8;
        style.padding_y = 5;
        style.min_width = 0;
        // Match click popups: keep hover tooltips just 2 px from the bar edge.
        let offset = 2;
        let (width, height) = Renderer::tooltip_size(&text, &style);
        let max_left = bar_width.saturating_sub(width) as i32;
        let left = (x.round() as i32 - width as i32 / 2).clamp(0, max_left.max(0));

        self.tooltip = None;
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-layout-tooltip"),
            Some(&output),
        );
        if self.config.position == "bottom" {
            layer.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
            layer.set_margin(
                0,
                0,
                self.config
                    .margin_bottom
                    .saturating_add(bar_height)
                    .saturating_add(offset),
                left,
            );
        } else {
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_margin(
                self.config
                    .margin_top
                    .saturating_add(bar_height)
                    .saturating_add(offset),
                0,
                0,
                left,
            );
        }
        layer.set_size(width, height);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        // Ignore the bar's reserved exclusive zone; margins already include bar_height + offset.
        layer.set_exclusive_zone(-1);
        layer.commit();

        self.tooltip = Some(TooltipSurface {
            layer,
            text,
            style,
            width,
            height,
            configured: false,
            bar_index,
            item_index: None,
        });
    }

    fn maybe_show_tray_tooltip(&mut self, qh: &QueueHandle<Self>) {
        let Some(hover) = self.tray_hover.as_ref() else {
            return;
        };
        if Instant::now() < hover.deadline {
            return;
        }
        if self.tooltip.as_ref().is_some_and(|tooltip| {
            tooltip.bar_index == hover.bar_index && tooltip.item_index == Some(hover.item_index)
        }) {
            return;
        }

        let Some(bar) = self.bars.get(hover.bar_index) else {
            self.clear_tray_hover();
            return;
        };
        let Some(tray) = self.tray.as_ref() else {
            self.clear_tray_hover();
            return;
        };

        let style = tray.tooltip_style().clone();
        let offset = tray.tooltip_offset();
        let text = hover.text.clone();
        let (width, height) = Renderer::tooltip_size(&text, &style);
        let max_left = bar.width.saturating_sub(width) as i32;
        let left = (hover.local_x.round() as i32 - width as i32 / 2).clamp(0, max_left.max(0));
        let output = bar.output.clone();
        let bar_height = bar.height as i32;
        let bar_index = hover.bar_index;
        let item_index = hover.item_index;

        self.tooltip = None;
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-tooltip"),
            Some(&output),
        );
        if self.config.position == "bottom" {
            layer.set_anchor(Anchor::BOTTOM | Anchor::LEFT);
            layer.set_margin(
                0,
                0,
                self.config
                    .margin_bottom
                    .saturating_add(bar_height)
                    .saturating_add(offset),
                left,
            );
        } else {
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_margin(
                self.config
                    .margin_top
                    .saturating_add(bar_height)
                    .saturating_add(offset),
                0,
                0,
                left,
            );
        }
        layer.set_size(width, height);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        // Ignore the bar's reserved exclusive zone; margins already include bar_height + offset.
        layer.set_exclusive_zone(-1);
        layer.commit();

        self.tooltip = Some(TooltipSurface {
            layer,
            text,
            style,
            width,
            height,
            configured: false,
            bar_index,
            item_index: Some(item_index),
        });
    }

    fn draw_tooltip(&mut self) {
        let Some(tooltip) = self.tooltip.as_ref() else {
            return;
        };
        if !tooltip.configured || tooltip.width == 0 || tooltip.height == 0 {
            return;
        }
        let surface = tooltip.layer.wl_surface().clone();
        let layer = tooltip.layer.clone();
        let width = tooltip.width;
        let height = tooltip.height;
        let text = tooltip.text.clone();
        let style = tooltip.style.clone();
        let stride = width as i32 * 4;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create tooltip SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) = self
            .renderer
            .draw_tooltip(canvas, width, height, &text, &style)
        {
            eprintln!("mhyprbar: tooltip render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach tooltip SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "tray")]
    fn open_tray_popup_request(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
        request: TrayMenuRequest,
    ) -> Result<()> {
        let style = self
            .tray
            .as_ref()
            .context("tray is unavailable")?
            .menu_style();
        let mut menu = TrayPopupModel::from_nodes(&request.nodes, style, (0.0, 0.0));
        anyhow::ensure!(!menu.is_empty(), "tray menu has no visible entries");

        let bar = self
            .bars
            .get(bar_index)
            .context("tray bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let origin_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - menu.root_height() - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };
        menu.set_origin(local_x, origin_y);

        self.tooltip = None;
        self.tray_popup = None;

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-tray-menu"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        self.tray_popup = Some(TrayPopupSurface {
            layer,
            item_id: request.item_id,
            menu,
            width: 1,
            height: 1,
            configured: false,
        });
        Ok(())
    }

    #[cfg(mhypr_module = "tray")]
    fn tray_popup_item_gone(&self) -> bool {
        self.tray_popup.as_ref().is_some_and(|popup| {
            !self
                .tray
                .as_ref()
                .is_some_and(|tray| tray.has_item(&popup.item_id))
        })
    }

    #[cfg(not(mhypr_module = "tray"))]
    fn tray_popup_item_gone(&self) -> bool {
        false
    }

    #[cfg(mhypr_module = "tray")]
    fn close_tray_popup(&mut self) {
        self.tray_popup = None;
    }

    #[cfg(not(mhypr_module = "tray"))]
    fn close_tray_popup(&mut self) {}

    #[cfg(mhypr_module = "tray")]
    fn tray_popup_matches_item_at(&self, bar_index: usize, x: f64) -> bool {
        let Some(bar) = self.bars.get(bar_index) else {
            return false;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            return false;
        };
        if hit.name != "tray" {
            return false;
        }

        let Some(item_id) = self
            .tray
            .as_ref()
            .and_then(|tray| tray.item_id_at(hit.offset_x))
        else {
            return false;
        };

        self.tray_popup
            .as_ref()
            .is_some_and(|popup| popup.item_id == item_id)
    }

    #[cfg(not(mhypr_module = "tray"))]
    fn tray_popup_matches_item_at(&self, _bar_index: usize, _x: f64) -> bool {
        false
    }

    #[cfg(mhypr_module = "tray")]
    fn open_tray_popup_at(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        x: f64,
        y: f64,
        require_item_is_menu: bool,
    ) -> bool {
        let Some(bar) = self.bars.get(bar_index) else {
            return false;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            return false;
        };
        if hit.name != "tray" {
            return false;
        }

        let offset_x = hit.offset_x;
        let (screen_x, screen_y) = self.screen_position_for_bar(bar_index, x, y);
        let item_is_menu = self
            .tray
            .as_ref()
            .is_some_and(|tray| tray.item_is_menu_at(offset_x));
        if require_item_is_menu && !item_is_menu {
            return false;
        }

        let request = match self.tray.as_mut() {
            Some(tray) => match tray.menu_request_at(offset_x) {
                Ok(request) => request,
                Err(error) => {
                    eprintln!("mhyprbar: failed to read tray DBusMenu: {error:#}");
                    return true;
                }
            },
            None => return true,
        };

        if let Some(request) = request {
            if let Err(error) = self.open_tray_popup_request(qh, bar_index, x, request) {
                eprintln!("mhyprbar: failed to open tray popup: {error:#}");
            }
            return true;
        }

        if let Some(tray) = self.tray.as_mut()
            && let Err(error) = tray.context_menu_at(offset_x, screen_x, screen_y)
        {
            eprintln!("mhyprbar: tray ContextMenu fallback failed: {error:#}");
        }
        true
    }

    #[cfg(not(mhypr_module = "tray"))]
    fn open_tray_popup_at(
        &mut self,
        _qh: &QueueHandle<Self>,
        _bar_index: usize,
        _x: f64,
        _y: f64,
        _require_item_is_menu: bool,
    ) -> bool {
        false
    }

    #[cfg(not(mhypr_module = "tray"))]
    fn open_tray_popup_index(&mut self, _qh: &QueueHandle<Self>, _index: usize) -> Result<()> {
        anyhow::bail!("tray module is not compiled")
    }

    #[cfg(mhypr_module = "tray")]
    fn open_tray_popup_index(&mut self, qh: &QueueHandle<Self>, index: usize) -> Result<()> {
        let request = self
            .tray
            .as_mut()
            .context("tray is unavailable")?
            .menu_request_index(index)?;
        if self
            .tray_popup
            .as_ref()
            .is_some_and(|popup| popup.item_id == request.item_id)
        {
            self.close_tray_popup();
            return Ok(());
        }

        let bar_index = 0usize;
        let bar = self
            .bars
            .get(bar_index)
            .context("no bar output is available")?;
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let tray_start = (0..bar.width as i32)
            .find(|x| {
                render::module_at_x(
                    *x as f64,
                    bar.width,
                    workspace_visible,
                    &self.config,
                    &self.modules,
                )
                .is_some_and(|hit| hit.name == "tray")
            })
            .context("tray module is not visible on the first bar")?;
        let tray = self.tray.as_ref().context("tray is unavailable")?;
        let local_x = tray_start
            .saturating_add(tray.padding_x())
            .saturating_add(index as i32 * tray.icon_size().saturating_add(tray.spacing()))
            .saturating_add(tray.icon_size() / 2) as f64;
        self.open_tray_popup_request(qh, bar_index, local_x, request)
    }

    #[cfg(mhypr_module = "tray")]
    fn draw_tray_popup(&mut self) {
        let Some(popup) = self.tray_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create tray popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) = self
            .renderer
            .draw_tray_popup(canvas, width, height, &popup.menu)
        {
            eprintln!("mhyprbar: tray popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach tray popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "tray")]
    fn activate_tray_popup_at(&mut self, x: f64, y: f64) {
        let outcome = match self.tray_popup.as_ref() {
            Some(popup) => popup
                .menu
                .click(x, y, popup.width as f64, popup.height as f64),
            None => return,
        };

        match outcome {
            TrayPopupClick::Keep => {}
            TrayPopupClick::Close => self.close_tray_popup(),
            TrayPopupClick::Activate(node_id) => {
                let Some(item_id) = self.tray_popup.as_ref().map(|popup| popup.item_id.clone())
                else {
                    return;
                };
                let result = self
                    .tray
                    .as_mut()
                    .context("tray is unavailable")
                    .and_then(|tray| tray.menu_click_item(&item_id, node_id));
                if let Err(error) = result {
                    eprintln!("mhyprbar: tray DBusMenu click failed: {error:#}");
                } else {
                    self.close_tray_popup();
                }
            }
        }
    }

    #[cfg(mhypr_module = "battery")]
    fn battery_popup_timeout(&self) -> Option<Duration> {
        let popup = self.battery_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "battery")]
    fn refresh_battery_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.battery_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: battery popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        self.draw_battery_popup();
    }

    #[cfg(mhypr_module = "battery")]
    fn close_battery_popup(&mut self) {
        self.battery_popup = None;
    }

    #[cfg(mhypr_module = "battery")]
    fn toggle_battery_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.battery_popup.is_some() {
            self.close_battery_popup();
            return Ok(true);
        }

        let Some(model) = BatteryPopupModel::new()? else {
            return Ok(false);
        };
        if !model.config.enabled {
            return Ok(false);
        }

        let bar = self
            .bars
            .get(bar_index)
            .context("battery popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x = (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-battery-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let interval = model.config.refresh_interval();
        self.battery_popup = Some(BatteryPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh: Instant::now() + interval,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "battery")]
    fn draw_battery_popup(&mut self) {
        let Some(popup) = self.battery_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let panel_x = popup.panel_x;
        let panel_y = popup.panel_y;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create battery popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) =
            self.renderer
                .draw_battery_popup(canvas, width, height, &popup.model, panel_x, panel_y)
        {
            eprintln!("mhyprbar: battery popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach battery popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "clock")]
    fn clock_popup_timeout(&self) -> Option<Duration> {
        let popup = self.clock_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "clock")]
    fn refresh_clock_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.clock_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: clock popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        self.draw_clock_popup();
    }

    #[cfg(mhypr_module = "clock")]
    fn close_clock_popup(&mut self) {
        self.clock_popup = None;
    }

    #[cfg(mhypr_module = "clock")]
    fn toggle_clock_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.clock_popup.is_some() {
            self.close_clock_popup();
            return Ok(true);
        }

        let model = ClockPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }

        let bar = self
            .bars
            .get(bar_index)
            .context("clock popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x = (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-clock-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let interval = model.config.refresh_interval();
        self.clock_popup = Some(ClockPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh: Instant::now() + interval,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "clock")]
    fn draw_clock_popup(&mut self) {
        let Some(popup) = self.clock_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let panel_x = popup.panel_x;
        let panel_y = popup.panel_y;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create clock popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) =
            self.renderer
                .draw_clock_popup(canvas, width, height, &popup.model, panel_x, panel_y)
        {
            eprintln!("mhyprbar: clock popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach clock popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "cpu")]
    fn cpu_popup_timeout(&self) -> Option<Duration> {
        let popup = self.cpu_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "cpu")]
    fn refresh_cpu_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.cpu_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: CPU popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        self.draw_cpu_popup();
    }

    #[cfg(mhypr_module = "cpu")]
    fn close_cpu_popup(&mut self) {
        self.cpu_popup = None;
    }

    #[cfg(mhypr_module = "cpu")]
    fn toggle_cpu_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.cpu_popup.is_some() {
            self.close_cpu_popup();
            return Ok(true);
        }

        let model = CpuPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }
        let bar = self
            .bars
            .get(bar_index)
            .context("CPU popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x = (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-cpu-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let interval = model.config.refresh_interval();
        self.cpu_popup = Some(CpuPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh: Instant::now() + interval,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "cpu")]
    fn toggle_cpu_popup_first(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        if self.cpu_popup.is_some() {
            self.close_cpu_popup();
            return Ok(());
        }

        let bar_index = 0usize;
        let bar = self
            .bars
            .get(bar_index)
            .context("no bar output is available")?;
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let mut start = None;
        let mut end = None;
        for x in 0..bar.width as i32 {
            let is_cpu = render::module_at_x(
                x as f64,
                bar.width,
                workspace_visible,
                &self.config,
                &self.modules,
            )
            .is_some_and(|hit| hit.name == "cpu");
            if is_cpu {
                start.get_or_insert(x);
                end = Some(x + 1);
            } else if start.is_some() {
                break;
            }
        }
        let (start, end) = start
            .zip(end)
            .context("cpu module is not visible on the first bar")?;
        let center = (start + end) as f64 / 2.0;
        let _ = self.toggle_cpu_popup(qh, bar_index, center)?;
        Ok(())
    }

    #[cfg(mhypr_module = "cpu")]
    fn draw_cpu_popup(&mut self) {
        let Some(popup) = self.cpu_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let panel_x = popup.panel_x;
        let panel_y = popup.panel_y;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create CPU popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) =
            self.renderer
                .draw_cpu_popup(canvas, width, height, &popup.model, panel_x, panel_y)
        {
            eprintln!("mhyprbar: CPU popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach CPU popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "gpu")]
    fn current_gpu_visual(&self) -> Option<crate::modules::gpu::GpuVisual> {
        let view = self.modules.view("gpu")?;
        match view.visual {
            ModuleVisual::Gpu(gpu) => Some(gpu),
            _ => None,
        }
    }

    #[cfg(mhypr_module = "gpu")]
    fn gpu_popup_timeout(&self) -> Option<Duration> {
        let popup = self.gpu_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "gpu")]
    fn refresh_gpu_popup_if_due(&mut self) {
        let now = Instant::now();
        let due = self
            .gpu_popup
            .as_ref()
            .is_some_and(|popup| now >= popup.next_refresh);
        if !due {
            return;
        }

        let limit = self
            .gpu_popup
            .as_ref()
            .map(|popup| popup.model.config.max_processes)
            .unwrap_or(8);
        let fallback_processes = self
            .gpu_popup
            .as_ref()
            .map(|popup| popup.model.processes.clone())
            .unwrap_or_default();

        let bar_changed = self.modules.force_refresh("gpu");
        let processes = match self.modules.gpu_processes(limit) {
            Ok(processes) => processes,
            Err(error) => {
                eprintln!("mhyprbar: GPU process refresh failed: {error:#}");
                fallback_processes
            }
        };
        let Some(visual) = self.current_gpu_visual() else {
            return;
        };
        if let Some(popup) = self.gpu_popup.as_mut() {
            popup.model.update(&visual, processes);
            popup.next_refresh = now + popup.model.config.refresh_interval();
        }
        if bar_changed {
            self.draw_all();
        }
        self.draw_gpu_popup();
    }

    #[cfg(mhypr_module = "gpu")]
    fn close_gpu_popup(&mut self) {
        self.gpu_popup = None;
    }

    #[cfg(mhypr_module = "gpu")]
    fn toggle_gpu_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.gpu_popup.is_some() {
            self.close_gpu_popup();
            return Ok(true);
        }

        let popup_config = GpuPopupConfig::load()?;
        if !popup_config.enabled {
            return Ok(false);
        }

        let bar_changed = self.modules.force_refresh("gpu");
        let processes = match self.modules.gpu_processes(popup_config.max_processes) {
            Ok(processes) => processes,
            Err(error) => {
                eprintln!("mhyprbar: GPU process read failed: {error:#}");
                Vec::new()
            }
        };
        let visual = self
            .current_gpu_visual()
            .context("GPU module visual is unavailable")?;
        let model = GpuPopupModel::new(popup_config, &visual, processes);
        if bar_changed {
            self.draw_all();
        }

        let bar = self
            .bars
            .get(bar_index)
            .context("GPU popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x = (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-gpu-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let interval = model.config.refresh_interval();
        self.gpu_popup = Some(GpuPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh: Instant::now() + interval,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "gpu")]
    fn draw_gpu_popup(&mut self) {
        let Some(popup) = self.gpu_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let panel_x = popup.panel_x;
        let panel_y = popup.panel_y;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create GPU popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) =
            self.renderer
                .draw_gpu_popup(canvas, width, height, &popup.model, panel_x, panel_y)
        {
            eprintln!("mhyprbar: GPU popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach GPU popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "monitor")]
    fn monitor_popup_timeout(&self) -> Option<Duration> {
        let popup = self.monitor_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "monitor")]
    fn refresh_monitor_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.monitor_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: monitor popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        if self.modules.force_refresh("monitor") {
            self.draw_all();
        }
        self.draw_monitor_popup();
    }

    #[cfg(mhypr_module = "monitor")]
    fn close_monitor_popup(&mut self) {
        self.monitor_context = None;
        self.monitor_popup = None;
    }

    #[cfg(mhypr_module = "monitor")]
    fn toggle_monitor_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.monitor_popup.is_some() {
            self.close_monitor_popup();
            return Ok(true);
        }

        let model = MonitorPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }

        if self.modules.force_refresh("monitor") {
            self.draw_all();
        }

        let bar = self
            .bars
            .get(bar_index)
            .context("monitor popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x = (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-monitor-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let interval = model.config.refresh_interval();
        self.monitor_popup = Some(MonitorPopupSurface {
            layer,
            model,
            output,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh: Instant::now() + interval,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "monitor")]
    fn draw_monitor_popup(&mut self) {
        let Some(popup) = self.monitor_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let panel_x = popup.panel_x;
        let panel_y = popup.panel_y;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create monitor popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) =
            self.renderer
                .draw_monitor_popup(canvas, width, height, &popup.model, panel_x, panel_y)
        {
            eprintln!("mhyprbar: monitor popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach monitor popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "monitor")]
    fn open_monitor_context(
        &mut self,
        qh: &QueueHandle<Self>,
        cursor_x: f64,
        cursor_y: f64,
    ) -> Result<()> {
        let Some(popup) = self.monitor_popup.as_ref() else {
            return Ok(());
        };
        let output = popup.output.clone();
        let output_w = popup.width.max(1) as f64;
        let output_h = popup.height.max(1) as f64;
        let panel_width = 360;
        let row_height = 30;
        let panel_height = row_height * 9;
        let panel_x = (cursor_x + 8.0).clamp(0.0, (output_w - panel_width as f64).max(0.0));
        let panel_y = (cursor_y + 8.0).clamp(0.0, (output_h - panel_height as f64).max(0.0));

        self.monitor_context = None;
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-monitor-context"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();
        self.monitor_context = Some(MonitorContextSurface {
            layer,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            panel_width,
            row_height,
            hovered_action: None,
        });
        Ok(())
    }

    #[cfg(mhypr_module = "monitor")]
    fn draw_monitor_context(&mut self) {
        let Some(context) = self.monitor_context.as_ref() else {
            return;
        };
        let Some(popup) = self.monitor_popup.as_ref() else {
            return;
        };
        if !context.configured || context.width == 0 || context.height == 0 {
            return;
        }
        let surface = context.layer.wl_surface().clone();
        let layer = context.layer.clone();
        let width = context.width;
        let height = context.height;
        let stride = width as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create monitor context SHM buffer: {error}");
                return;
            }
        };
        if let Err(error) = self.renderer.draw_monitor_context(
            canvas,
            width,
            height,
            &popup.model,
            context.panel_x,
            context.panel_y,
            context.panel_width,
            context.row_height,
            context.hovered_action,
        ) {
            eprintln!("mhyprbar: monitor context render failed: {error:#}");
            return;
        }
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach monitor context SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "audio")]
    fn audio_popup_timeout(&self) -> Option<Duration> {
        let popup = self.audio_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "audio")]
    fn refresh_audio_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.audio_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if popup.dragging_volume.is_some() {
            popup.next_refresh = now + popup.model.config.refresh_interval();
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: audio popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        self.draw_audio_popup();
    }

    #[cfg(mhypr_module = "audio")]
    fn close_audio_popup(&mut self) {
        self.audio_popup = None;
    }

    #[cfg(mhypr_module = "audio")]
    fn toggle_audio_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.audio_popup.is_some() {
            self.close_audio_popup();
            return Ok(true);
        }

        let model = AudioPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }
        let (output, bar_width, bar_height) = {
            let bar = self
                .bars
                .get(bar_index)
                .context("audio popup bar output is unavailable")?;
            (bar.output.clone(), bar.width, bar.height)
        };
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x =
            (local_x - panel_w / 2.0).clamp(0.0, (bar_width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar_height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar_height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "active_window")]
        self.close_active_window_popup();
        #[cfg(mhypr_module = "brightness")]
        self.close_brightness_popup();
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "monitor")]
        self.close_monitor_popup();
        #[cfg(mhypr_module = "network")]
        self.close_network_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let next_refresh = Instant::now() + model.config.refresh_interval();
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-audio-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        self.audio_popup = Some(AudioPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh,
            dragging_volume: None,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "audio")]
    fn draw_audio_popup(&mut self) {
        let Some(popup) = self.audio_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create audio popup SHM buffer: {error}");
                return;
            }
        };
        if let Err(error) = self.renderer.draw_audio_popup(
            canvas,
            width,
            height,
            &popup.model,
            popup.panel_x,
            popup.panel_y,
        ) {
            eprintln!("mhyprbar: audio popup render failed: {error:#}");
            return;
        }
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach audio popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "brightness")]
    fn brightness_popup_timeout(&self) -> Option<Duration> {
        let popup = self.brightness_popup.as_ref()?;
        Some(
            popup
                .next_refresh
                .saturating_duration_since(Instant::now()),
        )
    }

    #[cfg(mhypr_module = "brightness")]
    fn refresh_brightness_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.brightness_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if popup.dragging_brightness.is_some() {
            popup.next_refresh = now + popup.model.config.refresh_interval();
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: brightness popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        self.draw_brightness_popup();
    }

    #[cfg(mhypr_module = "brightness")]
    fn close_brightness_popup(&mut self) {
        self.brightness_popup = None;
    }

    #[cfg(mhypr_module = "brightness")]
    fn toggle_brightness_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.brightness_popup.is_some() {
            self.close_brightness_popup();
            return Ok(true);
        }

        let model = BrightnessPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }
        let (output, bar_width, bar_height) = {
            let bar = self
                .bars
                .get(bar_index)
                .context("brightness popup bar output is unavailable")?;
            (bar.output.clone(), bar.width, bar.height)
        };
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x =
            (local_x - panel_w / 2.0).clamp(0.0, (bar_width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar_height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar_height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "active_window")]
        self.close_active_window_popup();
        #[cfg(mhypr_module = "audio")]
        self.close_audio_popup();
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "monitor")]
        self.close_monitor_popup();
        #[cfg(mhypr_module = "network")]
        self.close_network_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let next_refresh = Instant::now() + model.config.refresh_interval();
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-brightness-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        self.brightness_popup = Some(BrightnessPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh,
            dragging_brightness: None,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "brightness")]
    fn draw_brightness_popup(&mut self) {
        let Some(popup) = self.brightness_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create brightness popup SHM buffer: {error}");
                return;
            }
        };
        if let Err(error) = self.renderer.draw_brightness_popup(
            canvas,
            width,
            height,
            &popup.model,
            popup.panel_x,
            popup.panel_y,
        ) {
            eprintln!("mhyprbar: brightness popup render failed: {error:#}");
            return;
        }
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach brightness popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "layout")]
    fn close_layout_popup(&mut self) {
        self.layout_popup = None;
    }

    #[cfg(mhypr_module = "layout")]
    fn toggle_layout_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.layout_popup.is_some() {
            self.close_layout_popup();
            return Ok(true);
        }

        let model = LayoutPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }
        let (output, bar_width, bar_height) = {
            let bar = self
                .bars
                .get(bar_index)
                .context("layout popup bar output is unavailable")?;
            (bar.output.clone(), bar.width, bar.height)
        };
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x =
            (local_x - panel_w / 2.0).clamp(0.0, (bar_width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar_height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar_height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "active_window")]
        self.close_active_window_popup();
        #[cfg(mhypr_module = "audio")]
        self.close_audio_popup();
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "brightness")]
        self.close_brightness_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "monitor")]
        self.close_monitor_popup();
        #[cfg(mhypr_module = "network")]
        self.close_network_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-layout-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        self.layout_popup = Some(LayoutPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "layout")]
    fn draw_layout_popup(&mut self) {
        let Some(popup) = self.layout_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }
        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create layout popup SHM buffer: {error}");
                return;
            }
        };
        if let Err(error) = self.renderer.draw_layout_popup(
            canvas,
            width,
            height,
            &popup.model,
            popup.panel_x,
            popup.panel_y,
        ) {
            eprintln!("mhyprbar: layout popup render failed: {error:#}");
            return;
        }
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach layout popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "active_window")]
    fn close_active_window_popup(&mut self) {
        self.active_window_popup = None;
    }

    #[cfg(mhypr_module = "active_window")]
    fn toggle_active_window_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
        monitor_scope: bool,
    ) -> Result<bool> {
        if self.active_window_popup.is_some() {
            self.close_active_window_popup();
            return Ok(true);
        }

        let monitor = self
            .monitor_for_bar(bar_index)
            .cloned()
            .context("active window popup monitor is unavailable")?;
        let active = hyprland::active_window()?;
        let scope = if monitor_scope {
            WindowListScope::Monitor {
                id: monitor.id,
                name: monitor.name.clone(),
            }
        } else {
            WindowListScope::Workspace {
                id: monitor.active_workspace,
            }
        };
        let model = ActiveWindowPopupModel::new(scope, &active.address)?;
        if !model.config.enabled {
            return Ok(false);
        }

        let (output, bar_width, bar_height) = {
            let bar = self
                .bars
                .get(bar_index)
                .context("active window popup bar output is unavailable")?;
            (bar.output.clone(), bar.width, bar.height)
        };
        let output_height = monitor.height.max(1) as f64;
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x =
            (local_x - panel_w / 2.0).clamp(0.0, (bar_width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar_height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar_height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "audio")]
        self.close_audio_popup();
        #[cfg(mhypr_module = "brightness")]
        self.close_brightness_popup();
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "monitor")]
        self.close_monitor_popup();
        #[cfg(mhypr_module = "network")]
        self.close_network_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-active-window-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        self.active_window_popup = Some(ActiveWindowPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "active_window")]
    fn draw_active_window_popup(&mut self) {
        let Some(popup) = self.active_window_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }
        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create active window popup SHM buffer: {error}");
                return;
            }
        };
        if let Err(error) = self.renderer.draw_active_window_popup(
            canvas,
            width,
            height,
            &popup.model,
            popup.panel_x,
            popup.panel_y,
        ) {
            eprintln!("mhyprbar: active window popup render failed: {error:#}");
            return;
        }
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach active window popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "network")]
    fn close_network_popup(&mut self) {
        self.network_popup = None;
    }

    #[cfg(mhypr_module = "network")]
    fn toggle_network_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.network_popup.is_some() {
            self.close_network_popup();
            return Ok(true);
        }

        let model = NetworkPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }
        let bar = self
            .bars
            .get(bar_index)
            .context("network popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x =
            (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "monitor")]
        self.close_monitor_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-network-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        self.network_popup = Some(NetworkPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "network")]
    fn draw_network_popup(&mut self) {
        let Some(popup) = self.network_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }
        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create network popup SHM buffer: {error}");
                return;
            }
        };
        if let Err(error) = self.renderer.draw_network_popup(
            canvas,
            width,
            height,
            &popup.model,
            popup.panel_x,
            popup.panel_y,
        ) {
            eprintln!("mhyprbar: network popup render failed: {error:#}");
            return;
        }
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach network popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "disk")]
    fn disk_popup_timeout(&self) -> Option<Duration> {
        let popup = self.disk_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "disk")]
    fn refresh_disk_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.disk_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: disk popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        self.draw_disk_popup();
    }

    #[cfg(mhypr_module = "disk")]
    fn close_disk_popup(&mut self) {
        self.disk_popup = None;
    }

    #[cfg(mhypr_module = "disk")]
    fn toggle_disk_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.disk_popup.is_some() {
            self.close_disk_popup();
            return Ok(true);
        }

        let model = DiskPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }
        let bar = self
            .bars
            .get(bar_index)
            .context("disk popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x = (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-disk-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let interval = model.config.refresh_interval();
        self.disk_popup = Some(DiskPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh: Instant::now() + interval,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "disk")]
    fn draw_disk_popup(&mut self) {
        let Some(popup) = self.disk_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let panel_x = popup.panel_x;
        let panel_y = popup.panel_y;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create disk popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) =
            self.renderer
                .draw_disk_popup(canvas, width, height, &popup.model, panel_x, panel_y)
        {
            eprintln!("mhyprbar: disk popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach disk popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    #[cfg(mhypr_module = "memory")]
    fn memory_popup_timeout(&self) -> Option<Duration> {
        let popup = self.memory_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "memory")]
    fn refresh_memory_popup_if_due(&mut self) {
        let now = Instant::now();
        let Some(popup) = self.memory_popup.as_mut() else {
            return;
        };
        if now < popup.next_refresh {
            return;
        }
        if let Err(error) = popup.model.refresh() {
            eprintln!("mhyprbar: memory popup refresh failed: {error:#}");
        }
        popup.next_refresh = now + popup.model.config.refresh_interval();
        self.draw_memory_popup();
    }

    #[cfg(mhypr_module = "memory")]
    fn close_memory_popup(&mut self) {
        self.memory_popup = None;
    }

    #[cfg(mhypr_module = "memory")]
    fn toggle_memory_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
    ) -> Result<bool> {
        if self.memory_popup.is_some() {
            self.close_memory_popup();
            return Ok(true);
        }

        let model = MemoryPopupModel::new()?;
        if !model.config.enabled {
            return Ok(false);
        }
        let bar = self
            .bars
            .get(bar_index)
            .context("memory popup bar output is unavailable")?;
        let output = bar.output.clone();
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1) as f64)
            .unwrap_or(1080.0);
        let panel_w = model.config.width as f64;
        let panel_h = model.panel_height() as f64;
        let panel_x = (local_x - panel_w / 2.0).clamp(0.0, (bar.width as f64 - panel_w).max(0.0));
        let panel_y = if self.config.position == "bottom" {
            (output_height - bar.height as f64 - panel_h - 2.0).max(0.0)
        } else {
            bar.height as f64 + 2.0
        };

        self.tooltip = None;
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        self.close_tray_popup();

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-memory-popup"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.commit();

        let interval = model.config.refresh_interval();
        self.memory_popup = Some(MemoryPopupSurface {
            layer,
            model,
            width: 1,
            height: 1,
            configured: false,
            panel_x,
            panel_y,
            next_refresh: Instant::now() + interval,
        });
        Ok(true)
    }

    #[cfg(mhypr_module = "memory")]
    fn draw_memory_popup(&mut self) {
        let Some(popup) = self.memory_popup.as_ref() else {
            return;
        };
        if !popup.configured || popup.width == 0 || popup.height == 0 {
            return;
        }

        let surface = popup.layer.wl_surface().clone();
        let layer = popup.layer.clone();
        let width = popup.width;
        let height = popup.height;
        let stride = width as i32 * 4;
        let panel_x = popup.panel_x;
        let panel_y = popup.panel_y;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create memory popup SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) =
            self.renderer
                .draw_memory_popup(canvas, width, height, &popup.model, panel_x, panel_y)
        {
            eprintln!("mhyprbar: memory popup render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach memory popup SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    fn reload_config(&mut self) -> Result<()> {
        let config = crate::validate_config().context("reload validation failed")?;
        let background = config.background_rgba()?;
        let mut modules = ModuleManager::load()?;
        modules.refresh_due();
        self.clear_tray_hover();
        #[cfg(mhypr_module = "battery")]
        self.close_battery_popup();
        #[cfg(mhypr_module = "clock")]
        self.close_clock_popup();
        #[cfg(mhypr_module = "cpu")]
        self.close_cpu_popup();
        #[cfg(mhypr_module = "gpu")]
        self.close_gpu_popup();
        #[cfg(mhypr_module = "disk")]
        self.close_disk_popup();
        #[cfg(mhypr_module = "memory")]
        self.close_memory_popup();
        self.close_tray_popup();
        if let Some(tray) = self.tray.as_mut() {
            tray.reload_config()?;
        }
        modules.set_width_override("tray", Some(self.tray.as_ref().map_or(0, TrayState::width)));

        let anchor = if config.position == "bottom" {
            Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT
        } else {
            Anchor::TOP | Anchor::LEFT | Anchor::RIGHT
        };
        let exclusive_zone = if config.exclusive_zone {
            config.height as i32
        } else {
            0
        };

        for bar in &mut self.bars {
            bar.layer.set_anchor(anchor);
            bar.layer.set_size(0, config.height);
            bar.layer.set_margin(
                config.margin_top,
                config.margin_right,
                config.margin_bottom,
                config.margin_left,
            );
            bar.layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            bar.layer.set_exclusive_zone(exclusive_zone);
            bar.height = config.height;
            bar.layer.commit();
        }

        self.config = config;
        self.background = background;
        self.modules = modules;
        self.draw_all();
        Ok(())
    }

    fn tray_list_text(&self) -> String {
        let mut output = self
            .tray
            .as_ref()
            .map(TrayState::list_text)
            .unwrap_or_default();

        for (bar_index, bar) in self.bars.iter().enumerate() {
            let workspace_visible = self.monitor_for_bar(bar_index).is_some();
            let mut start = None;
            let mut end = None;
            for x in 0..bar.width as i32 {
                if render::module_at_x(
                    x as f64,
                    bar.width,
                    workspace_visible,
                    &self.config,
                    &self.modules,
                )
                .is_some_and(|hit| hit.name == "tray")
                {
                    start.get_or_insert(x);
                    end = Some(x + 1);
                } else if start.is_some() {
                    break;
                }
            }
            if let (Some(start), Some(end)) = (start, end) {
                output.push_str(&format!(
                    "bar{bar_index}\toutput={}\ttray_x={start}..{end}\n",
                    bar.output_name.as_deref().unwrap_or("?")
                ));
            }
        }

        output
    }

    fn force_tray_tooltip(&mut self, item_index: usize) -> Result<()> {
        let tray = self.tray.as_ref().context("tray is unavailable")?;
        anyhow::ensure!(
            item_index < tray.len(),
            "tray index {item_index} is out of range"
        );
        let text = tray
            .tooltip_text_for_index(item_index)
            .context("tray item has no tooltip text")?;
        let item_key = tray
            .item_key_for_index(item_index)
            .context("tray item has no stable id")?
            .to_owned();
        let padding_x = tray.padding_x();
        let icon_size = tray.icon_size();
        let spacing = tray.spacing();

        let bar_index = 0usize;
        let bar = self
            .bars
            .get(bar_index)
            .context("no bar output is available")?;
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let tray_start = (0..bar.width as i32)
            .find(|x| {
                render::module_at_x(
                    *x as f64,
                    bar.width,
                    workspace_visible,
                    &self.config,
                    &self.modules,
                )
                .is_some_and(|hit| hit.name == "tray")
            })
            .context("tray module is not visible on the first bar")?;

        let local_x = tray_start
            .saturating_add(padding_x)
            .saturating_add(item_index as i32 * icon_size.saturating_add(spacing))
            .saturating_add(icon_size / 2) as f64;

        self.tooltip = None;
        self.tray_hover = Some(TrayHover {
            bar_index,
            item_index,
            item_key,
            local_x,
            deadline: Instant::now(),
            text,
        });
        Ok(())
    }

    fn status_text(&self) -> String {
        let compiled = crate::modules::compiled()
            .into_iter()
            .map(|module| module.name)
            .collect::<Vec<_>>()
            .join(",");
        let tray_items = self.tray.as_ref().map_or(0, TrayState::len);
        format!(
            "running=1 pid={} outputs={} position={} height={} tray_items={} compiled={} left={} center={} right={}\n",
            std::process::id(),
            self.bars.len(),
            self.config.position,
            self.config.height,
            tray_items,
            compiled,
            self.config.left.join(","),
            self.config.center.join(","),
            self.config.right.join(","),
        )
    }

    fn monitor_for_bar(&self, index: usize) -> Option<&MonitorState> {
        let bar = self.bars.get(index)?;
        bar.output_name
            .as_deref()
            .and_then(|name| self.hyprland.monitor_by_name(name))
            .or_else(|| self.hyprland.monitor_by_index(index))
    }

    fn draw_all(&mut self) {
        for index in 0..self.bars.len() {
            self.draw(index);
        }
    }

    fn draw(&mut self, index: usize) {
        let Some(bar) = self.bars.get(index) else {
            return;
        };
        if !bar.configured || bar.width == 0 || bar.height == 0 {
            return;
        }

        let surface = bar.layer.wl_surface().clone();
        let layer = bar.layer.clone();
        let width = bar.width;
        let height = bar.height;
        let monitor = self.monitor_for_bar(index).cloned();
        let stride = width as i32 * 4;

        let (buffer, canvas) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("mhyprbar: failed to create SHM buffer: {error}");
                return;
            }
        };

        if let Err(error) = self.renderer.draw(
            canvas,
            width,
            height,
            self.background,
            &self.config,
            &self.modules,
            self.tray.as_ref(),
            monitor.as_ref(),
            &self.hyprland,
        ) {
            eprintln!("mhyprbar: render failed: {error:#}");
            return;
        }

        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(&surface) {
            eprintln!("mhyprbar: failed to attach SHM buffer: {error}");
            return;
        }
        layer.commit();
    }

    fn activate_workspace(&mut self, bar_index: usize, x: f64) -> bool {
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(local_workspace) =
            render::workspace_at_x(x, workspace_visible, &self.config, &self.modules)
        else {
            return false;
        };
        let Some(monitor) = self.monitor_for_bar(bar_index).cloned() else {
            return false;
        };
        let workspace_id =
            self.config
                .workspaces
                .global_workspace_id(&monitor.name, monitor.id, local_workspace);

        if let Err(error) = hyprland::switch_workspace(&monitor.name, workspace_id) {
            eprintln!(
                "mhyprbar: failed to switch {} to workspace {}: {error:#}",
                monitor.name, workspace_id
            );
            return true;
        }

        if let Err(error) = self.refresh_hyprland() {
            eprintln!("mhyprbar: failed to refresh after workspace switch: {error:#}");
        }
        true
    }

    fn screen_position_for_bar(&self, bar_index: usize, x: f64, y: f64) -> (i32, i32) {
        let Some(bar) = self.bars.get(bar_index) else {
            return (x.round() as i32, y.round() as i32);
        };
        let Some(monitor) = self.monitor_for_bar(bar_index) else {
            return (x.round() as i32, y.round() as i32);
        };

        let origin_y = if self.config.position == "bottom" {
            monitor
                .y
                .saturating_add(monitor.height)
                .saturating_sub(bar.height as i32)
        } else {
            monitor.y
        };
        (
            monitor.x.saturating_add(x.round() as i32),
            origin_y.saturating_add(y.round() as i32),
        )
    }

    fn tray_action_at(
        &mut self,
        bar_index: usize,
        x: f64,
        y: f64,
        action: TrayPointerAction,
    ) -> bool {
        let Some(bar) = self.bars.get(bar_index) else {
            return false;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            return false;
        };
        if hit.name != "tray" {
            return false;
        }

        let offset_x = hit.offset_x;
        let (screen_x, screen_y) = self.screen_position_for_bar(bar_index, x, y);
        let Some(tray) = self.tray.as_mut() else {
            return true;
        };
        let result = match action {
            TrayPointerAction::Primary => tray.activate_at(offset_x, screen_x, screen_y),
            TrayPointerAction::Secondary => {
                tray.secondary_activate_at(offset_x, screen_x, screen_y)
            }
            TrayPointerAction::ScrollHorizontal(delta) => {
                tray.scroll_at(offset_x, delta, "horizontal")
            }
            TrayPointerAction::ScrollVertical(delta) => tray.scroll_at(offset_x, delta, "vertical"),
        };
        if let Err(error) = result {
            eprintln!("mhyprbar: tray action failed: {error:#}");
        }
        true
    }

    fn activate_module_at(&mut self, _qh: &QueueHandle<Self>, bar_index: usize, x: f64, y: f64) {
        if self.tray_action_at(bar_index, x, y, TrayPointerAction::Primary) {
            return;
        }

        let Some(bar) = self.bars.get(bar_index) else {
            return;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            return;
        };
        let name = hit.name.to_owned();

        #[cfg(mhypr_module = "menu")]
        if name == "menu" {
            let origin_y = if self.config.position == "bottom" {
                y
            } else {
                bar.height as f64 + 2.0
            };
            match self.modules.activate_at(&name, x, origin_y) {
                Ok(true) => self.draw_all(),
                Ok(false) => {}
                Err(error) => eprintln!("mhyprbar: menu popup action failed: {error:#}"),
            }
            return;
        }

        #[cfg(mhypr_module = "active_window")]
        if name == "active_window" {
            match self.toggle_active_window_popup(_qh, bar_index, x, false) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: active window popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "audio")]
        if name == "audio" {
            match self.toggle_audio_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: audio popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "brightness")]
        if name == "brightness" {
            match self.toggle_brightness_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: brightness popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "battery")]
        if name == "battery" {
            match self.toggle_battery_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: battery popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "clock")]
        if name == "clock" {
            match self.toggle_clock_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: clock popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "cpu")]
        if name == "cpu" {
            match self.toggle_cpu_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: CPU popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "gpu")]
        if name == "gpu" {
            match self.toggle_gpu_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: GPU popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "monitor")]
        if name == "monitor" {
            match self.toggle_monitor_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: monitor popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "network")]
        if name == "network" {
            match self.toggle_network_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: network popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "disk")]
        if name == "disk" {
            match self.toggle_disk_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: disk popup action failed: {error:#}");
                    return;
                }
            }
        }

        #[cfg(mhypr_module = "memory")]
        if name == "memory" {
            match self.toggle_memory_popup(_qh, bar_index, x) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("mhyprbar: memory popup action failed: {error:#}");
                    return;
                }
            }
        }

        match self.modules.activate(&name) {
            Ok(true) => self.draw_all(),
            Ok(false) => {}
            Err(error) => eprintln!("mhyprbar: module {name} action failed: {error:#}"),
        }
    }

    #[cfg(mhypr_module = "layout")]
    fn activate_layout_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        x: f64,
    ) -> bool {
        let Some(bar) = self.bars.get(bar_index) else {
            return false;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            return false;
        };
        if hit.name != "layout" {
            return false;
        }

        if let Err(error) = self.toggle_layout_popup(qh, bar_index, x) {
            eprintln!("mhyprbar: layout popup action failed: {error:#}");
        }
        true
    }

    #[cfg(mhypr_module = "active_window")]
    fn activate_active_window_monitor_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        x: f64,
    ) -> bool {
        let Some(bar) = self.bars.get(bar_index) else {
            return false;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            return false;
        };
        if hit.name != "active_window" {
            return false;
        }

        if let Err(error) = self.toggle_active_window_popup(qh, bar_index, x, true) {
            eprintln!("mhyprbar: active window monitor popup action failed: {error:#}");
        }
        true
    }

    fn scroll_module_at(&mut self, bar_index: usize, x: f64, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let Some(bar) = self.bars.get(bar_index) else {
            return false;
        };
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();
        let Some(hit) =
            render::module_at_x(x, bar.width, workspace_visible, &self.config, &self.modules)
        else {
            return false;
        };
        if hit.name == "tray" {
            return false;
        }

        let name = hit.name.to_owned();
        let direction = if delta < 0 { 1 } else { -1 };
        match self.modules.scroll(&name, direction) {
            Ok(true) => {
                self.draw_all();
                true
            }
            Ok(false) => false,
            Err(error) => {
                eprintln!("mhyprbar: module {name} scroll failed: {error:#}");
                true
            }
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        if self
            .tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.layer.wl_surface() == layer.wl_surface())
        {
            self.tooltip = None;
            return;
        }
        #[cfg(mhypr_module = "active_window")]
        if self
            .active_window_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.active_window_popup = None;
            return;
        }
        #[cfg(mhypr_module = "audio")]
        if self
            .audio_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.audio_popup = None;
            return;
        }
        #[cfg(mhypr_module = "brightness")]
        if self
            .brightness_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.brightness_popup = None;
            return;
        }
        #[cfg(mhypr_module = "battery")]
        if self
            .battery_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.battery_popup = None;
            return;
        }
        #[cfg(mhypr_module = "clock")]
        if self
            .clock_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.clock_popup = None;
            return;
        }
        #[cfg(mhypr_module = "cpu")]
        if self
            .cpu_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.cpu_popup = None;
            return;
        }
        #[cfg(mhypr_module = "gpu")]
        if self
            .gpu_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.gpu_popup = None;
            return;
        }
        #[cfg(mhypr_module = "layout")]
        if self
            .layout_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.layout_popup = None;
            return;
        }
        #[cfg(mhypr_module = "monitor")]
        if self
            .monitor_context
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.monitor_context = None;
            return;
        }
        #[cfg(mhypr_module = "monitor")]
        if self
            .monitor_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.monitor_context = None;
            self.monitor_popup = None;
            return;
        }
        #[cfg(mhypr_module = "network")]
        if self
            .network_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.network_popup = None;
            return;
        }
        #[cfg(mhypr_module = "disk")]
        if self
            .disk_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.disk_popup = None;
            return;
        }
        #[cfg(mhypr_module = "memory")]
        if self
            .memory_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.memory_popup = None;
            return;
        }
        #[cfg(mhypr_module = "tray")]
        if self
            .tray_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            self.tray_popup = None;
            return;
        }

        self.bars
            .retain(|bar| bar.layer.wl_surface() != layer.wl_surface());
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if self
            .tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(tooltip) = self.tooltip.as_mut() {
                if configure.new_size.0 > 0 {
                    tooltip.width = configure.new_size.0;
                }
                if configure.new_size.1 > 0 {
                    tooltip.height = configure.new_size.1;
                }
                tooltip.configured = true;
            }
            self.draw_tooltip();
            return;
        }
        #[cfg(mhypr_module = "active_window")]
        if self
            .active_window_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.active_window_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_active_window_popup();
            return;
        }
        #[cfg(mhypr_module = "audio")]
        if self
            .audio_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.audio_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_audio_popup();
            return;
        }
        #[cfg(mhypr_module = "brightness")]
        if self
            .brightness_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.brightness_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_brightness_popup();
            return;
        }
        #[cfg(mhypr_module = "battery")]
        if self
            .battery_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.battery_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_battery_popup();
            return;
        }
        #[cfg(mhypr_module = "clock")]
        if self
            .clock_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.clock_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_clock_popup();
            return;
        }
        #[cfg(mhypr_module = "cpu")]
        if self
            .cpu_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.cpu_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_cpu_popup();
            return;
        }
        #[cfg(mhypr_module = "gpu")]
        if self
            .gpu_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.gpu_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_gpu_popup();
            return;
        }
        #[cfg(mhypr_module = "layout")]
        if self
            .layout_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.layout_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_layout_popup();
            return;
        }
        #[cfg(mhypr_module = "monitor")]
        if self
            .monitor_context
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.monitor_context.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_monitor_context();
            return;
        }
        #[cfg(mhypr_module = "monitor")]
        if self
            .monitor_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.monitor_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_monitor_popup();
            return;
        }
        #[cfg(mhypr_module = "network")]
        if self
            .network_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.network_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_network_popup();
            return;
        }
        #[cfg(mhypr_module = "disk")]
        if self
            .disk_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.disk_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_disk_popup();
            return;
        }
        #[cfg(mhypr_module = "memory")]
        if self
            .memory_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.memory_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_memory_popup();
            return;
        }
        #[cfg(mhypr_module = "tray")]
        if self
            .tray_popup
            .as_ref()
            .is_some_and(|popup| popup.layer.wl_surface() == layer.wl_surface())
        {
            if let Some(popup) = self.tray_popup.as_mut() {
                popup.width = configure.new_size.0.max(1);
                popup.height = configure.new_size.1.max(1);
                popup.configured = true;
            }
            self.draw_tray_popup();
            return;
        }

        let Some(index) = self
            .bars
            .iter()
            .position(|bar| bar.layer.wl_surface() == layer.wl_surface())
        else {
            return;
        };

        let bar = &mut self.bars[index];
        bar.width = configure.new_size.0.max(1);
        bar.height = configure.new_size.1.max(self.config.height).max(1);
        bar.configured = true;
        self.draw(index);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(error) => eprintln!("mhyprbar: failed to create pointer: {error}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            #[cfg(mhypr_module = "active_window")]
            if self
                .active_window_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.active_window_popup.as_mut() {
                            let next = popup.model.row_at(
                                event.position.0 - popup.panel_x,
                                event.position.1 - popup.panel_y,
                            );
                            if popup.model.hovered_row != next {
                                popup.model.hovered_row = next;
                                redraw = true;
                            }
                        }
                    }
                    PointerEventKind::Leave { .. } => {
                        if let Some(popup) = self.active_window_popup.as_mut()
                            && popup.model.hovered_row.take().is_some()
                        {
                            redraw = true;
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let (inside, address) =
                            self.active_window_popup
                                .as_ref()
                                .map_or((false, None), |popup| {
                                    let local_x = event.position.0 - popup.panel_x;
                                    let local_y = event.position.1 - popup.panel_y;
                                    let inside = local_x >= 0.0
                                        && local_x < popup.model.config.width as f64
                                        && local_y >= 0.0
                                        && local_y < popup.model.panel_height() as f64;
                                    let address = popup
                                        .model
                                        .row_at(local_x, local_y)
                                        .and_then(|index| popup.model.rows.get(index))
                                        .map(|row| row.address.clone());
                                    (inside, address)
                                });
                        if !inside {
                            self.close_active_window_popup();
                        } else if let Some(address) = address {
                            self.close_active_window_popup();
                            if let Err(error) = hyprland::focus_window(&address) {
                                eprintln!("mhyprbar: failed to focus window {address}: {error:#}");
                            } else {
                                self.modules.force_refresh("active_window");
                                if let Err(error) = self.refresh_hyprland() {
                                    eprintln!(
                                        "mhyprbar: failed to refresh after window focus: {error:#}"
                                    );
                                }
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_active_window_popup();
                    }
                    _ => {}
                }
                if redraw {
                    self.draw_active_window_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "layout")]
            if self
                .layout_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.layout_popup.as_mut() {
                            let next = popup.model.row_at(
                                event.position.0 - popup.panel_x,
                                event.position.1 - popup.panel_y,
                            );
                            if popup.model.hovered_row != next {
                                popup.model.hovered_row = next;
                                redraw = true;
                            }
                        }
                    }
                    PointerEventKind::Leave { .. } => {
                        if let Some(popup) = self.layout_popup.as_mut()
                            && popup.model.hovered_row.take().is_some()
                        {
                            redraw = true;
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let (inside, row) = self.layout_popup.as_ref().map_or(
                            (false, None),
                            |popup| {
                                let local_x = event.position.0 - popup.panel_x;
                                let local_y = event.position.1 - popup.panel_y;
                                let inside = local_x >= 0.0
                                    && local_x < popup.model.config.width as f64
                                    && local_y >= 0.0
                                    && local_y < popup.model.panel_height() as f64;
                                (inside, popup.model.row_at(local_x, local_y))
                            },
                        );
                        if !inside {
                            self.close_layout_popup();
                        } else if let Some(row) = row {
                            let result = self
                                .layout_popup
                                .as_ref()
                                .map(|popup| popup.model.apply(row));
                            self.close_layout_popup();
                            if let Some(Err(error)) = result {
                                eprintln!("mhyprbar: layout selection failed: {error:#}");
                            } else {
                                self.modules.force_refresh("layout");
                                self.draw_all();
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_layout_popup();
                    }
                    _ => {}
                }
                if redraw {
                    self.draw_layout_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "audio")]
            if self
                .audio_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                let mut refresh_bar = false;
                match event.kind {
                    PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.audio_popup.as_mut()
                            && let Some(index) = popup.dragging_volume
                        {
                            let local_x = event.position.0 - popup.panel_x;
                            if let Some(percent) = popup.model.volume_percent_for_x(index, local_x) {
                                match popup.model.set_volume(index, percent) {
                                    Ok(changed) => redraw |= changed,
                                    Err(error) => {
                                        eprintln!(
                                            "mhyprbar: audio popup volume drag failed: {error:#}"
                                        );
                                        popup.dragging_volume = None;
                                    }
                                }
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let Some(popup) = self.audio_popup.as_mut() else {
                            continue;
                        };
                        let local_x = event.position.0 - popup.panel_x;
                        let local_y = event.position.1 - popup.panel_y;
                        let inside = local_x >= 0.0
                            && local_x < popup.model.config.width as f64
                            && local_y >= 0.0
                            && local_y < popup.model.panel_height() as f64;
                        if !inside {
                            self.close_audio_popup();
                            continue;
                        }

                        if let Some(index) = popup.model.mute_at(local_x, local_y) {
                            match popup.model.toggle_mute(index) {
                                Ok(()) => {
                                    redraw = true;
                                    refresh_bar = true;
                                }
                                Err(error) => {
                                    eprintln!("mhyprbar: audio output mute failed: {error:#}");
                                }
                            }
                        } else if let Some((index, percent)) =
                            popup.model.volume_at(local_x, local_y)
                        {
                            popup.dragging_volume = Some(index);
                            match popup.model.set_volume(index, percent) {
                                Ok(changed) => redraw |= changed,
                                Err(error) => {
                                    eprintln!("mhyprbar: audio output volume failed: {error:#}");
                                    popup.dragging_volume = None;
                                }
                            }
                        }
                    }
                    PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                        if let Some(popup) = self.audio_popup.as_mut()
                            && popup.dragging_volume.take().is_some()
                        {
                            refresh_bar = true;
                        }
                    }
                    PointerEventKind::Leave { .. } => {
                        if let Some(popup) = self.audio_popup.as_mut()
                            && popup.dragging_volume.take().is_some()
                        {
                            refresh_bar = true;
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_audio_popup();
                        continue;
                    }
                    _ => {}
                }
                if refresh_bar {
                    self.modules.force_refresh("audio");
                    self.draw_all();
                }
                if redraw {
                    self.draw_audio_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "brightness")]
            if self
                .brightness_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                let mut refresh_bar = false;
                match event.kind {
                    PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.brightness_popup.as_mut()
                            && let Some(index) = popup.dragging_brightness
                        {
                            let local_x = event.position.0 - popup.panel_x;
                            if let Some(percent) = popup.model.percent_for_x(index, local_x) {
                                match popup.model.set_percent(index, percent) {
                                    Ok(changed) => redraw |= changed,
                                    Err(error) => {
                                        eprintln!(
                                            "mhyprbar: brightness popup drag failed: {error:#}"
                                        );
                                        popup.dragging_brightness = None;
                                    }
                                }
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let Some(popup) = self.brightness_popup.as_mut() else {
                            continue;
                        };
                        let local_x = event.position.0 - popup.panel_x;
                        let local_y = event.position.1 - popup.panel_y;
                        let inside = local_x >= 0.0
                            && local_x < popup.model.config.width as f64
                            && local_y >= 0.0
                            && local_y < popup.model.panel_height() as f64;
                        if !inside {
                            self.close_brightness_popup();
                            continue;
                        }

                        if let Some((index, percent)) =
                            popup.model.brightness_at(local_x, local_y)
                        {
                            popup.dragging_brightness = Some(index);
                            match popup.model.set_percent(index, percent) {
                                Ok(changed) => redraw |= changed,
                                Err(error) => {
                                    eprintln!("mhyprbar: brightness adjustment failed: {error:#}");
                                    popup.dragging_brightness = None;
                                }
                            }
                        }
                    }
                    PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                        if let Some(popup) = self.brightness_popup.as_mut()
                            && popup.dragging_brightness.take().is_some()
                        {
                            refresh_bar = true;
                        }
                    }
                    PointerEventKind::Leave { .. } => {
                        if let Some(popup) = self.brightness_popup.as_mut()
                            && popup.dragging_brightness.take().is_some()
                        {
                            refresh_bar = true;
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_brightness_popup();
                        continue;
                    }
                    _ => {}
                }
                if refresh_bar {
                    self.modules.force_refresh("brightness");
                    self.draw_all();
                }
                if redraw {
                    self.draw_brightness_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "battery")]
            if self
                .battery_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                match event.kind {
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let inside = self.battery_popup.as_ref().is_some_and(|popup| {
                            event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                                && event.position.1 >= popup.panel_y
                                && event.position.1
                                    < popup.panel_y + popup.model.panel_height() as f64
                        });
                        if !inside {
                            self.close_battery_popup();
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_battery_popup();
                    }
                    _ => {}
                }
                continue;
            }

            #[cfg(mhypr_module = "clock")]
            if self
                .clock_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                match event.kind {
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let (inside, action) =
                            self.clock_popup.as_ref().map_or((false, None), |popup| {
                                let inside = event.position.0 >= popup.panel_x
                                    && event.position.0
                                        < popup.panel_x + popup.model.config.width as f64
                                    && event.position.1 >= popup.panel_y
                                    && event.position.1
                                        < popup.panel_y + popup.model.panel_height() as f64;
                                let action = inside
                                    .then(|| {
                                        popup.model.action_at(
                                            event.position.0 - popup.panel_x,
                                            event.position.1 - popup.panel_y,
                                        )
                                    })
                                    .flatten();
                                (inside, action)
                            });

                        if !inside {
                            self.close_clock_popup();
                        } else if let Some(action) = action {
                            if let Some(popup) = self.clock_popup.as_mut()
                                && let Err(error) = popup.model.apply_action(action)
                            {
                                eprintln!("mhyprbar: clock calendar navigation failed: {error:#}");
                            }
                            self.draw_clock_popup();
                        }
                    }
                    PointerEventKind::Axis {
                        horizontal,
                        vertical,
                        ..
                    } => {
                        let horizontal = axis_scroll_delta(horizontal);
                        let vertical = axis_scroll_delta(vertical);
                        let delta = if horizontal != 0 {
                            horizontal
                        } else {
                            vertical
                        };
                        if delta != 0 {
                            let month_delta = if delta > 0 { 1 } else { -1 };
                            if let Some(popup) = self.clock_popup.as_mut()
                                && let Err(error) = popup.model.navigate(month_delta)
                            {
                                eprintln!("mhyprbar: clock calendar scroll failed: {error:#}");
                            }
                            self.draw_clock_popup();
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_clock_popup();
                    }
                    _ => {}
                }
                continue;
            }

            #[cfg(mhypr_module = "cpu")]
            if self
                .cpu_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.cpu_popup.as_mut() {
                            let local_y = event.position.1 - popup.panel_y;
                            let next = if event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                            {
                                popup.model.process_at(local_y)
                            } else {
                                None
                            };
                            if popup.model.hovered_process != next {
                                popup.model.hovered_process = next;
                                redraw = true;
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let inside = self.cpu_popup.as_ref().is_some_and(|popup| {
                            event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                                && event.position.1 >= popup.panel_y
                                && event.position.1
                                    < popup.panel_y + popup.model.panel_height() as f64
                        });
                        if !inside {
                            self.close_cpu_popup();
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_cpu_popup();
                    }
                    _ => {}
                }
                if redraw {
                    self.draw_cpu_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "gpu")]
            if self
                .gpu_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                match event.kind {
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let inside = self.gpu_popup.as_ref().is_some_and(|popup| {
                            event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                                && event.position.1 >= popup.panel_y
                                && event.position.1
                                    < popup.panel_y + popup.model.panel_height() as f64
                        });
                        if !inside {
                            self.close_gpu_popup();
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_gpu_popup();
                    }
                    _ => {}
                }
                continue;
            }

            #[cfg(mhypr_module = "monitor")]
            if self
                .monitor_context
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw_context = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(context) = self.monitor_context.as_mut() {
                            let inside_x = event.position.0 >= context.panel_x
                                && event.position.0
                                    < context.panel_x + context.panel_width as f64;
                            let inside_y = event.position.1 >= context.panel_y
                                && event.position.1
                                    < context.panel_y + (context.row_height * 9) as f64;
                            let next = if inside_x && inside_y {
                                Some(
                                    ((event.position.1 - context.panel_y)
                                        / context.row_height as f64)
                                        .floor() as usize,
                                )
                            } else {
                                None
                            };
                            if context.hovered_action != next {
                                context.hovered_action = next;
                                redraw_context = true;
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let action = self.monitor_context.as_ref().and_then(|context| {
                            let inside_x = event.position.0 >= context.panel_x
                                && event.position.0
                                    < context.panel_x + context.panel_width as f64;
                            let inside_y = event.position.1 >= context.panel_y
                                && event.position.1
                                    < context.panel_y + (context.row_height * 9) as f64;
                            if !(inside_x && inside_y) {
                                return None;
                            }
                            let index = ((event.position.1 - context.panel_y)
                                / context.row_height as f64)
                                .floor() as usize;
                            context_action(index)
                        });
                        if let Some(action) = action {
                            if let Some(popup) = self.monitor_popup.as_mut()
                                && let Err(error) = popup.model.apply_action(action)
                            {
                                eprintln!("mhyprbar: monitor context action failed: {error:#}");
                            }
                            if self.modules.force_refresh("monitor") {
                                self.draw_all();
                            }
                            self.monitor_context = None;
                            self.draw_monitor_popup();
                        } else {
                            self.monitor_context = None;
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.monitor_context = None;
                    }
                    _ => {}
                }
                if redraw_context {
                    self.draw_monitor_context();
                }
                continue;
            }

            #[cfg(mhypr_module = "monitor")]
            if self
                .monitor_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.monitor_popup.as_mut() {
                            let local_y = event.position.1 - popup.panel_y;
                            let next = if event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                            {
                                popup.model.row_at(local_y)
                            } else {
                                None
                            };
                            if popup.model.hovered_row != next {
                                popup.model.hovered_row = next;
                                redraw = true;
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let (inside, action) =
                            self.monitor_popup.as_ref().map_or((false, None), |popup| {
                                let inside = event.position.0 >= popup.panel_x
                                    && event.position.0
                                        < popup.panel_x + popup.model.config.width as f64
                                    && event.position.1 >= popup.panel_y
                                    && event.position.1
                                        < popup.panel_y + popup.model.panel_height() as f64;
                                let action = inside
                                    .then(|| {
                                        popup.model.action_at(
                                            event.position.0 - popup.panel_x,
                                            event.position.1 - popup.panel_y,
                                        )
                                    })
                                    .flatten();
                                (inside, action)
                            });

                        if !inside {
                            self.close_monitor_popup();
                        } else if let Some(action) = action {
                            if let Some(popup) = self.monitor_popup.as_mut()
                                && let Err(error) = popup.model.apply_action(action)
                            {
                                eprintln!("mhyprbar: monitor setting action failed: {error:#}");
                            }
                            if self.modules.force_refresh("monitor") {
                                self.draw_all();
                            }
                            redraw = true;
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        let handled = if let Some(popup) = self.monitor_popup.as_mut() {
                            let inside_x = event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64;
                            let local_y = event.position.1 - popup.panel_y;
                            inside_x && popup.model.open_context_at(local_y)
                        } else {
                            false
                        };
                        if handled {
                            if let Err(error) =
                                self.open_monitor_context(qh, event.position.0, event.position.1)
                            {
                                eprintln!("mhyprbar: failed to open monitor context: {error:#}");
                            }
                            redraw = true;
                        } else {
                            self.close_monitor_popup();
                        }
                    }
                    _ => {}
                }
                if redraw {
                    self.draw_monitor_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "network")]
            if self
                .network_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                match event.kind {
                    PointerEventKind::Press { button, .. }
                        if button == BTN_LEFT || button == BTN_RIGHT =>
                    {
                        let inside = self.network_popup.as_ref().is_some_and(|popup| {
                            event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                                && event.position.1 >= popup.panel_y
                                && event.position.1
                                    < popup.panel_y + popup.model.panel_height() as f64
                        });
                        if !inside || button == BTN_RIGHT {
                            self.close_network_popup();
                        }
                    }
                    _ => {}
                }
                continue;
            }

            #[cfg(mhypr_module = "disk")]
            if self
                .disk_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.disk_popup.as_mut() {
                            let local_y = event.position.1 - popup.panel_y;
                            let next = if event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                            {
                                popup.model.row_at(local_y)
                            } else {
                                None
                            };
                            if popup.model.hovered_row != next {
                                popup.model.hovered_row = next;
                                redraw = true;
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let inside = self.disk_popup.as_ref().is_some_and(|popup| {
                            event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                                && event.position.1 >= popup.panel_y
                                && event.position.1
                                    < popup.panel_y + popup.model.panel_height() as f64
                        });
                        if !inside {
                            self.close_disk_popup();
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_disk_popup();
                    }
                    _ => {}
                }
                if redraw {
                    self.draw_disk_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "memory")]
            if self
                .memory_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.memory_popup.as_mut() {
                            let local_y = event.position.1 - popup.panel_y;
                            let next = if event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                            {
                                popup.model.process_at(local_y)
                            } else {
                                None
                            };
                            if popup.model.hovered_process != next {
                                popup.model.hovered_process = next;
                                redraw = true;
                            }
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        let inside = self.memory_popup.as_ref().is_some_and(|popup| {
                            event.position.0 >= popup.panel_x
                                && event.position.0
                                    < popup.panel_x + popup.model.config.width as f64
                                && event.position.1 >= popup.panel_y
                                && event.position.1
                                    < popup.panel_y + popup.model.panel_height() as f64
                        });
                        if !inside {
                            self.close_memory_popup();
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_memory_popup();
                    }
                    _ => {}
                }
                if redraw {
                    self.draw_memory_popup();
                }
                continue;
            }

            #[cfg(mhypr_module = "tray")]
            if self
                .tray_popup
                .as_ref()
                .is_some_and(|popup| popup.layer.wl_surface() == &event.surface)
            {
                let mut redraw = false;
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        if let Some(popup) = self.tray_popup.as_mut() {
                            redraw = popup.menu.pointer_moved(
                                event.position.0,
                                event.position.1,
                                popup.width as f64,
                                popup.height as f64,
                            );
                        }
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                        self.activate_tray_popup_at(event.position.0, event.position.1);
                    }
                    PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                        self.close_tray_popup();
                    }
                    _ => {}
                }
                if redraw {
                    self.draw_tray_popup();
                }
                continue;
            }

            let Some(index) = self
                .bars
                .iter()
                .position(|bar| bar.layer.wl_surface() == &event.surface)
            else {
                continue;
            };

            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.update_tray_hover(index, event.position.0);
                    #[cfg(mhypr_module = "layout")]
                    self.update_layout_tooltip(qh, index, event.position.0);
                }
                PointerEventKind::Leave { .. } => {
                    if self
                        .tray_hover
                        .as_ref()
                        .is_some_and(|hover| hover.bar_index == index)
                    {
                        self.clear_tray_hover();
                    }
                    #[cfg(mhypr_module = "layout")]
                    self.clear_layout_tooltip();
                }
                PointerEventKind::Press { button, .. } => {
                    self.clear_tray_hover();
                    #[cfg(mhypr_module = "layout")]
                    self.clear_layout_tooltip();
                    match button {
                        BTN_LEFT => {
                            if !self.activate_workspace(index, event.position.0)
                                && !self.open_tray_popup_at(
                                    qh,
                                    index,
                                    event.position.0,
                                    event.position.1,
                                    true,
                                )
                            {
                                self.activate_module_at(
                                    qh,
                                    index,
                                    event.position.0,
                                    event.position.1,
                                );
                            }
                        }
                        BTN_RIGHT => {
                            #[cfg(mhypr_module = "layout")]
                            if self.activate_layout_popup(qh, index, event.position.0) {
                                continue;
                            }
                            #[cfg(mhypr_module = "active_window")]
                            if self.activate_active_window_monitor_popup(
                                qh,
                                index,
                                event.position.0,
                            ) {
                                continue;
                            }
                            if self.tray_popup_matches_item_at(index, event.position.0) {
                                self.close_tray_popup();
                            } else {
                                let _ = self.open_tray_popup_at(
                                    qh,
                                    index,
                                    event.position.0,
                                    event.position.1,
                                    false,
                                );
                            }
                        }
                        BTN_MIDDLE => {
                            let _ = self.tray_action_at(
                                index,
                                event.position.0,
                                event.position.1,
                                TrayPointerAction::Secondary,
                            );
                        }
                        _ => {}
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    #[cfg(mhypr_module = "layout")]
                    self.clear_layout_tooltip();
                    let horizontal = axis_scroll_delta(horizontal);
                    if horizontal != 0 {
                        let _ = self.tray_action_at(
                            index,
                            event.position.0,
                            event.position.1,
                            TrayPointerAction::ScrollHorizontal(horizontal),
                        );
                    }

                    let vertical = axis_scroll_delta(vertical);
                    if vertical != 0
                        && !self.tray_action_at(
                            index,
                            event.position.0,
                            event.position.1,
                            TrayPointerAction::ScrollVertical(vertical),
                        )
                    {
                        let _ = self.scroll_module_at(index, event.position.0, vertical);
                    } else if vertical == 0 && horizontal != 0 {
                        let _ = self.scroll_module_at(index, event.position.0, horizontal);
                    }
                }
                _ => {}
            }
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.add_output(qh, output);
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.update_output_name(&output);
        self.draw_all();
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.remove_output(&output);
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
