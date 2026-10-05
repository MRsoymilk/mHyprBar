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
use crate::modules::active_window::popup::{ActiveWindowPopupModel, WindowListScope};
#[cfg(mhypr_module = "audio")]
use crate::modules::audio::popup::AudioPopupModel;
#[cfg(mhypr_module = "battery")]
use crate::modules::battery::popup::BatteryPopupModel;
#[cfg(mhypr_module = "brightness")]
use crate::modules::brightness::popup::BrightnessPopupModel;
#[cfg(mhypr_module = "clock")]
use crate::modules::clock::popup::ClockPopupModel;
#[cfg(mhypr_module = "cpu")]
use crate::modules::cpu::popup::CpuPopupModel;
#[cfg(mhypr_module = "disk")]
use crate::modules::disk::popup::DiskPopupModel;
#[cfg(mhypr_module = "gpu")]
use crate::modules::gpu::popup::{GpuPopupConfig, GpuPopupModel};
#[cfg(mhypr_module = "layout")]
use crate::modules::layout::popup::LayoutPopupModel;
#[cfg(mhypr_module = "memory")]
use crate::modules::memory::popup::MemoryPopupModel;
#[cfg(mhypr_module = "monitor")]
use crate::modules::monitor::popup::{MonitorPopupModel, context_action};
#[cfg(mhypr_module = "network")]
use crate::modules::network::popup::NetworkPopupModel;
use crate::{
    config::{BarConfig, ModuleStyle},
    hyprland::{self, MonitorState, Snapshot},
    ipc::{self, Request},
    modules::{ModuleManager, ModuleVisual, tray::runtime::TrayState},
    render::{self, Renderer},
};
#[cfg(mhypr_module = "tray")]
use crate::modules::tray::{
    popup::{TrayPopupClick, TrayPopupModel},
    runtime::TrayMenuRequest,
};

mod core;
mod handlers;
#[cfg(mhypr_module = "monitor")]
mod hotplug;
mod popup_controls;
mod popup_storage;
mod popup_system;
mod popup_tray;

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
    #[cfg(mhypr_module = "monitor")]
    let monitor_hotplug = hotplug::MonitorHotplugState::new(&hyprland)
        .context("failed to initialize monitor hotplug state")?;
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
        #[cfg(mhypr_module = "monitor")]
        monitor_hotplug,
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
                    #[cfg(mhypr_module = "monitor")]
                    let hotplug_pending = if batch.monitor_topology_changed() {
                        app.schedule_monitor_hotplug(&batch)
                    } else {
                        app.monitor_hotplug_pending()
                    };
                    #[cfg(not(mhypr_module = "monitor"))]
                    let hotplug_pending = false;

                    if batch.state_changed
                        && !hotplug_pending
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
                                Ok(Some(Request::PopupToggle { name })) => {
                                    match app.toggle_popup_first(&control_qh, &name) {
                                        Ok(info) => format!("ok {info}\n"),
                                        Err(error) => format!("error: {error:#}\n"),
                                    }
                                }
                                Ok(Some(Request::PopupInfo { name })) => {
                                    match app.popup_debug_info(&name) {
                                        Ok(info) => format!("ok {info}\n"),
                                        Err(error) => format!("error: {error:#}\n"),
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
        #[cfg(mhypr_module = "monitor")]
        if let Some(hotplug_timeout) = app.monitor_hotplug_timeout() {
            timeout = timeout.min(hotplug_timeout);
        }
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
        #[cfg(mhypr_module = "monitor")]
        if let Err(error) = app.process_monitor_hotplug_if_due() {
            eprintln!("mhyprbar: monitor hotplug processing failed: {error:#}");
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
    cpu_pid: Option<u32>,
    debug_origin: Option<(i32, i32)>,
    #[cfg(mhypr_module = "cpu")]
    cpu_process: Option<crate::modules::cpu::popup::CpuProcessRow>,
    #[cfg(mhypr_module = "cpu")]
    cpu_accent: Option<[u8; 4]>,
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
    bar_index: usize,
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
    #[cfg(mhypr_module = "monitor")]
    monitor_hotplug: hotplug::MonitorHotplugState,
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
