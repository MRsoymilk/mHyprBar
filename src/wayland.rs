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

use crate::{
    config::BarConfig,
    hyprland::{self, MonitorState, Snapshot},
    ipc::{self, Request},
    modules::ModuleManager,
    render::{self, Renderer},
    tray::TrayState,
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
                                Ok(Some(Request::TrayList)) => app.tray_list_text(),
                                Ok(Some(Request::TrayMenuOpen { index })) => {
                                    match app.tray.as_mut() {
                                        Some(tray) => match tray.open_menu_index(index) {
                                            Ok(()) => "ok\n".to_owned(),
                                            Err(error) => format!("error: {error:#}\n"),
                                        },
                                        None => "error: tray is unavailable\n".to_owned(),
                                    }
                                }
                                Ok(Some(Request::TrayTooltipOpen { index })) => {
                                    match app.force_tray_tooltip(index) {
                                        Ok(()) => "ok\n".to_owned(),
                                        Err(error) => format!("error: {error:#}\n"),
                                    }
                                }
                                Ok(Some(Request::TrayMenuClick { token, node_id })) => {
                                    match app.tray.as_mut() {
                                        Some(tray) => match tray.menu_click(token, node_id) {
                                            Ok(()) => "ok\n".to_owned(),
                                            Err(error) => format!("error: {error:#}\n"),
                                        },
                                        None => "error: tray is unavailable\n".to_owned(),
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
        event_loop
            .dispatch(Some(timeout), &mut app)
            .context("event loop dispatch failed")?;
        if app.modules.refresh_due() {
            app.draw_all();
        }
        app.maybe_show_tray_tooltip(&qh);
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
    width: u32,
    height: u32,
    configured: bool,
    bar_index: usize,
    item_index: usize,
}

#[derive(Clone, Copy)]
enum TrayPointerAction {
    Primary,
    ContextMenu,
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
            tooltip.bar_index == hover.bar_index && tooltip.item_index == hover.item_index
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
        self.tooltip = None;
    }

    fn maybe_show_tray_tooltip(&mut self, qh: &QueueHandle<Self>) {
        let Some(hover) = self.tray_hover.as_ref() else {
            return;
        };
        if Instant::now() < hover.deadline {
            return;
        }
        if self.tooltip.as_ref().is_some_and(|tooltip| {
            tooltip.bar_index == hover.bar_index && tooltip.item_index == hover.item_index
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
        layer.set_exclusive_zone(0);
        layer.commit();

        self.tooltip = Some(TooltipSurface {
            layer,
            text,
            width,
            height,
            configured: false,
            bar_index,
            item_index,
        });
    }

    fn draw_tooltip(&mut self) {
        let Some(tooltip) = self.tooltip.as_ref() else {
            return;
        };
        if !tooltip.configured || tooltip.width == 0 || tooltip.height == 0 {
            return;
        }
        let Some(tray) = self.tray.as_ref() else {
            return;
        };

        let surface = tooltip.layer.wl_surface().clone();
        let layer = tooltip.layer.clone();
        let width = tooltip.width;
        let height = tooltip.height;
        let text = tooltip.text.clone();
        let style = tray.tooltip_style().clone();
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

    fn reload_config(&mut self) -> Result<()> {
        let config = crate::validate_config().context("reload validation failed")?;
        let background = config.background_rgba()?;
        let mut modules = ModuleManager::load()?;
        modules.refresh_due();
        self.clear_tray_hover();
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
        let Some(local_workspace) = self.config.workspaces.local_workspace_at_x(x) else {
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
            TrayPointerAction::ContextMenu => tray.context_menu_at(offset_x, screen_x, screen_y),
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

    fn activate_module_at(&mut self, bar_index: usize, x: f64, y: f64) {
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

        match self.modules.activate(&name) {
            Ok(true) => self.draw_all(),
            Ok(false) => {}
            Err(error) => eprintln!("mhyprbar: module {name} action failed: {error:#}"),
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
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
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
                }
                PointerEventKind::Leave { .. } => {
                    if self
                        .tray_hover
                        .as_ref()
                        .is_some_and(|hover| hover.bar_index == index)
                    {
                        self.clear_tray_hover();
                    }
                }
                PointerEventKind::Press { button, .. } => {
                    self.clear_tray_hover();
                    match button {
                        BTN_LEFT => {
                            if !self.activate_workspace(index, event.position.0) {
                                self.activate_module_at(index, event.position.0, event.position.1);
                            }
                        }
                        BTN_RIGHT => {
                            let _ = self.tray_action_at(
                                index,
                                event.position.0,
                                event.position.1,
                                TrayPointerAction::ContextMenu,
                            );
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
                    if vertical != 0 {
                        let _ = self.tray_action_at(
                            index,
                            event.position.0,
                            event.position.1,
                            TrayPointerAction::ScrollVertical(vertical),
                        );
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
