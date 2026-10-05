use super::*;

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
                let mut tooltip_process = None;
                let mut clear_tooltip = false;
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
                            tooltip_process = next;
                            clear_tooltip = next.is_none();
                        }
                    }
                    PointerEventKind::Leave { .. } => {
                        if let Some(popup) = self.cpu_popup.as_mut()
                            && popup.model.hovered_process.take().is_some()
                        {
                            redraw = true;
                        }
                        clear_tooltip = true;
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
                if let Some(index) = tooltip_process {
                    self.show_cpu_process_tooltip(qh, index, event.position.1);
                } else if clear_tooltip {
                    self.clear_cpu_process_tooltip();
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
                            let recalled = if let Some(popup) = self.monitor_popup.as_mut() {
                                match popup.model.apply_action(action) {
                                    Ok(address) => address,
                                    Err(error) => {
                                        eprintln!("mhyprbar: monitor setting action failed: {error:#}");
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            if let Some(address) = recalled {
                                self.monitor_hotplug.acknowledge_window(&address);
                                self.close_monitor_popup();
                                self.draw_all();
                            } else {
                                if self.modules.force_refresh("monitor") {
                                    self.draw_all();
                                }
                                redraw = true;
                            }
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
                            #[cfg(mhypr_module = "monitor")]
                            if self.activate_monitor_missing_popup(qh, index, event.position.0) {
                                continue;
                            }
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
