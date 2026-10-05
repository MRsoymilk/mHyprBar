use super::*;

impl App {
    #[cfg(mhypr_module = "battery")]
    pub(super) fn battery_popup_timeout(&self) -> Option<Duration> {
        let popup = self.battery_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "battery")]
    pub(super) fn refresh_battery_popup_if_due(&mut self) {
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
    pub(super) fn close_battery_popup(&mut self) {
        self.battery_popup = None;
    }

    #[cfg(mhypr_module = "battery")]
    pub(super) fn toggle_battery_popup(
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
    pub(super) fn draw_battery_popup(&mut self) {
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
    pub(super) fn clock_popup_timeout(&self) -> Option<Duration> {
        let popup = self.clock_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "clock")]
    pub(super) fn refresh_clock_popup_if_due(&mut self) {
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
    pub(super) fn close_clock_popup(&mut self) {
        self.clock_popup = None;
    }

    #[cfg(mhypr_module = "clock")]
    pub(super) fn toggle_clock_popup(
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
    pub(super) fn draw_clock_popup(&mut self) {
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
    pub(super) fn cpu_popup_timeout(&self) -> Option<Duration> {
        let popup = self.cpu_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "cpu")]
    pub(super) fn refresh_cpu_popup_if_due(&mut self) {
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
    pub(super) fn clear_cpu_process_tooltip(&mut self) {
        if self
            .tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.cpu_pid.is_some())
        {
            self.tooltip = None;
        }
    }

    #[cfg(mhypr_module = "cpu")]
    pub(super) fn show_cpu_process_tooltip(
        &mut self,
        qh: &QueueHandle<Self>,
        index: usize,
        pointer_y: f64,
    ) {
        let Some((
            text,
            process,
            bar_index,
            panel_x,
            panel_width,
            mut style,
            warn_percent,
            graph_low,
            graph_mid,
            graph_high,
        )) = self.cpu_popup.as_ref().and_then(|popup| {
            let process = popup.model.processes.get(index)?.clone();
            Some((
                popup.model.process_tooltip_text(index)?,
                process,
                popup.bar_index,
                popup.panel_x,
                popup.model.config.width,
                popup.model.config.style.clone(),
                popup.model.warn_percent,
                popup.model.graph_low,
                popup.model.graph_mid,
                popup.model.graph_high,
            ))
        }) else {
            self.clear_cpu_process_tooltip();
            return;
        };
        let pid = process.pid;

        if self.tooltip.as_ref().is_some_and(|tooltip| {
            tooltip.cpu_pid == Some(pid) && tooltip.text == text
        }) {
            return;
        }

        let Some(bar) = self.bars.get(bar_index) else {
            self.clear_cpu_process_tooltip();
            return;
        };
        let output = bar.output.clone();
        let output_width = bar.width as i32;
        let output_height = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.height.max(1))
            .unwrap_or(1080);

        style.background = "#17191DF2".into();
        style.font_size = 11.0;
        style.padding_x = 10;
        style.padding_y = 8;
        style.min_width = 0;

        let accent = render::cpu_usage_color(
            warn_percent,
            graph_low,
            graph_mid,
            graph_high,
            process.cpu.clamp(0.0, 100.0) / 100.0,
        );
        let (width, height) = Renderer::cpu_process_tooltip_size(&process, &style);
        let tooltip_w = width as i32;
        let tooltip_h = height as i32;
        let gap = 8_i32;
        let panel_left = panel_x.round() as i32;
        let panel_right = panel_left.saturating_add(panel_width);
        let right_candidate = panel_right.saturating_add(gap);
        let left = if right_candidate.saturating_add(tooltip_w) <= output_width {
            right_candidate
        } else {
            panel_left.saturating_sub(tooltip_w).saturating_sub(gap).max(0)
        };
        let max_top = output_height.saturating_sub(tooltip_h).max(0);
        let top = (pointer_y.round() as i32 - tooltip_h / 2).clamp(0, max_top);

        self.tooltip = None;
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("mhyprbar-cpu-process-tooltip"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin(top, 0, 0, left);
        layer.set_size(width, height);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
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
            cpu_pid: Some(pid),
            debug_origin: Some((left, top)),
            cpu_process: Some(process),
            cpu_accent: Some(accent),
        });
    }

    #[cfg(mhypr_module = "cpu")]
    pub(super) fn close_cpu_popup(&mut self) {
        self.clear_cpu_process_tooltip();
        self.cpu_popup = None;
    }

    #[cfg(mhypr_module = "cpu")]
    pub(super) fn toggle_cpu_popup(
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
            bar_index,
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
    pub(super) fn toggle_cpu_popup_first(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
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
    pub(super) fn draw_cpu_popup(&mut self) {
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
    pub(super) fn current_gpu_visual(&self) -> Option<crate::modules::gpu::GpuVisual> {
        let view = self.modules.view("gpu")?;
        match view.visual {
            ModuleVisual::Gpu(gpu) => Some(gpu),
            _ => None,
        }
    }

    #[cfg(mhypr_module = "gpu")]
    pub(super) fn gpu_popup_timeout(&self) -> Option<Duration> {
        let popup = self.gpu_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "gpu")]
    pub(super) fn refresh_gpu_popup_if_due(&mut self) {
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
    pub(super) fn close_gpu_popup(&mut self) {
        self.gpu_popup = None;
    }

    #[cfg(mhypr_module = "gpu")]
    pub(super) fn toggle_gpu_popup(
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
    pub(super) fn draw_gpu_popup(&mut self) {
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
    pub(super) fn monitor_popup_timeout(&self) -> Option<Duration> {
        let popup = self.monitor_popup.as_ref()?;
        Some(popup.next_refresh.saturating_duration_since(Instant::now()))
    }

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn refresh_monitor_popup_if_due(&mut self) {
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
    pub(super) fn close_monitor_popup(&mut self) {
        self.monitor_context = None;
        self.monitor_popup = None;
    }

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn toggle_monitor_popup(
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
        self.open_monitor_popup_model(qh, bar_index, local_x, model)
    }

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn toggle_monitor_missing_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
        target_monitor: String,
        missing_windows: Vec<crate::modules::monitor::MissingMonitorWindow>,
    ) -> Result<bool> {
        self.close_monitor_popup();
        let model = MonitorPopupModel::new_missing(target_monitor, missing_windows)?;
        self.open_monitor_popup_model(qh, bar_index, local_x, model)
    }

    #[cfg(mhypr_module = "monitor")]
    fn open_monitor_popup_model(
        &mut self,
        qh: &QueueHandle<Self>,
        bar_index: usize,
        local_x: f64,
        model: MonitorPopupModel,
    ) -> Result<bool> {
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
    pub(super) fn draw_monitor_popup(&mut self) {
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
    pub(super) fn open_monitor_context(
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
    pub(super) fn draw_monitor_context(&mut self) {
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

}
