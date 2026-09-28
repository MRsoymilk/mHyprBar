use std::io;

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
        pointer::{BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler},
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
        let timeout = app.modules.next_timeout();
        event_loop
            .dispatch(Some(timeout), &mut app)
            .context("event loop dispatch failed")?;
        if app.modules.refresh_due() {
            app.draw_all();
        }
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

    fn reload_config(&mut self) -> Result<()> {
        let config = crate::validate_config().context("reload validation failed")?;
        let background = config.background_rgba()?;
        let mut modules = ModuleManager::load()?;
        modules.refresh_due();
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

    fn activate_module_at(&mut self, bar_index: usize, x: f64) {
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

        if name == "tray" {
            if let Some(tray) = self.tray.as_mut()
                && let Err(error) = tray.activate_at(hit.offset_x)
            {
                eprintln!("mhyprbar: tray action failed: {error:#}");
            }
            return;
        }

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

            if let PointerEventKind::Press { button, .. } = event.kind
                && button == BTN_LEFT
                && !self.activate_workspace(index, event.position.0)
            {
                self.activate_module_at(index, event.position.0);
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
