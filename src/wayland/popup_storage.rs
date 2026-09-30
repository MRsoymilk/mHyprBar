use super::*;

impl App {
    #[cfg(mhypr_module = "disk")]
    pub(super) fn disk_popup_timeout(&self) -> Option<Duration> {
        let popup = self.disk_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "disk")]
    pub(super) fn refresh_disk_popup_if_due(&mut self) {
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
    pub(super) fn close_disk_popup(&mut self) {
        self.disk_popup = None;
    }

    #[cfg(mhypr_module = "disk")]
    pub(super) fn toggle_disk_popup(
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
    pub(super) fn draw_disk_popup(&mut self) {
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
    pub(super) fn memory_popup_timeout(&self) -> Option<Duration> {
        let popup = self.memory_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "memory")]
    pub(super) fn refresh_memory_popup_if_due(&mut self) {
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
    pub(super) fn close_memory_popup(&mut self) {
        self.memory_popup = None;
    }

    #[cfg(mhypr_module = "memory")]
    pub(super) fn toggle_memory_popup(
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
    pub(super) fn draw_memory_popup(&mut self) {
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

}
