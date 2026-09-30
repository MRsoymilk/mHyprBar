use super::*;

impl App {
    #[cfg(mhypr_module = "audio")]
    pub(super) fn audio_popup_timeout(&self) -> Option<Duration> {
        let popup = self.audio_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "audio")]
    pub(super) fn refresh_audio_popup_if_due(&mut self) {
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
    pub(super) fn close_audio_popup(&mut self) {
        self.audio_popup = None;
    }

    #[cfg(mhypr_module = "audio")]
    pub(super) fn toggle_audio_popup(
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
    pub(super) fn draw_audio_popup(&mut self) {
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
    pub(super) fn brightness_popup_timeout(&self) -> Option<Duration> {
        let popup = self.brightness_popup.as_ref()?;
        Some(
            popup
                .next_refresh
                .saturating_duration_since(Instant::now()),
        )
    }

    #[cfg(mhypr_module = "brightness")]
    pub(super) fn refresh_brightness_popup_if_due(&mut self) {
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
    pub(super) fn close_brightness_popup(&mut self) {
        self.brightness_popup = None;
    }

    #[cfg(mhypr_module = "brightness")]
    pub(super) fn toggle_brightness_popup(
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
    pub(super) fn draw_brightness_popup(&mut self) {
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
    pub(super) fn close_layout_popup(&mut self) {
        self.layout_popup = None;
    }

    #[cfg(mhypr_module = "layout")]
    pub(super) fn toggle_layout_popup(
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
    pub(super) fn draw_layout_popup(&mut self) {
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
    pub(super) fn close_active_window_popup(&mut self) {
        self.active_window_popup = None;
    }

    #[cfg(mhypr_module = "active_window")]
    pub(super) fn toggle_active_window_popup(
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
    pub(super) fn draw_active_window_popup(&mut self) {
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
    pub(super) fn close_network_popup(&mut self) {
        self.network_popup = None;
    }

    #[cfg(mhypr_module = "network")]
    pub(super) fn toggle_network_popup(
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
    pub(super) fn draw_network_popup(&mut self) {
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

}
