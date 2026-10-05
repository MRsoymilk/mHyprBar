use super::*;

impl App {
    pub(super) fn add_output(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
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

    pub(super) fn update_output_name(&mut self, output: &wl_output::WlOutput) {
        let name = self.output_state.info(output).and_then(|info| info.name);
        if let Some(bar) = self.bars.iter_mut().find(|bar| &bar.output == output) {
            bar.output_name = name;
        }
    }

    pub(super) fn remove_output(&mut self, output: &wl_output::WlOutput) {
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

    pub(super) fn refresh_hyprland(&mut self) -> Result<()> {
        self.hyprland = Snapshot::refresh()?;
        #[cfg(mhypr_module = "monitor")]
        self.monitor_hotplug.sync_if_idle(&self.hyprland);
        self.draw_all();
        Ok(())
    }

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn schedule_monitor_hotplug(&mut self, batch: &hyprland::EventBatch) -> bool {
        self.monitor_hotplug.schedule(&self.hyprland, batch)
    }

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn monitor_hotplug_pending(&self) -> bool {
        self.monitor_hotplug.pending()
    }

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn monitor_hotplug_timeout(&self) -> Option<Duration> {
        self.monitor_hotplug.timeout()
    }

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn process_monitor_hotplug_if_due(&mut self) -> Result<()> {
        if !self.monitor_hotplug.due() {
            return Ok(());
        }

        let mut snapshot = match Snapshot::refresh() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.monitor_hotplug.defer();
                return Err(error);
            }
        };
        let restored = self.monitor_hotplug.restore_workspaces(&snapshot)?;
        if restored > 0 {
            snapshot = match Snapshot::refresh() {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    self.monitor_hotplug.defer();
                    return Err(error);
                }
            };
        }

        self.hyprland = snapshot;
        self.monitor_hotplug.complete(&self.hyprland);
        self.modules.force_refresh("monitor");
        if let Some(popup) = self.monitor_popup.as_mut()
            && let Err(error) = popup.model.refresh()
        {
            eprintln!("mhyprbar: monitor popup refresh after hotplug failed: {error:#}");
        }
        self.draw_all();
        Ok(())
    }

    pub(super) fn sync_tray_width(&mut self) {
        let width = self.tray.as_ref().map_or(0, TrayState::width);
        let _ = self.modules.set_width_override("tray", Some(width));
    }


    pub(super) fn reload_config(&mut self) -> Result<()> {
        let config = crate::validate_config().context("reload validation failed")?;
        let background = config.background_rgba()?;
        #[cfg(mhypr_module = "monitor")]
        self.monitor_hotplug.reload_config()?;
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

    pub(super) fn tray_list_text(&self) -> String {
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

    pub(super) fn force_tray_tooltip(&mut self, item_index: usize) -> Result<()> {
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

    pub(super) fn status_text(&self) -> String {
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

    pub(super) fn monitor_for_bar(&self, index: usize) -> Option<&MonitorState> {
        let bar = self.bars.get(index)?;
        bar.output_name
            .as_deref()
            .and_then(|name| self.hyprland.monitor_by_name(name))
            .or_else(|| self.hyprland.monitor_by_index(index))
    }

    pub(super) fn draw_all(&mut self) {
        for index in 0..self.bars.len() {
            self.draw(index);
        }
    }

    pub(super) fn draw(&mut self, index: usize) {
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

    pub(super) fn activate_workspace(&mut self, bar_index: usize, x: f64) -> bool {
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

    pub(super) fn screen_position_for_bar(&self, bar_index: usize, x: f64, y: f64) -> (i32, i32) {
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

    pub(super) fn toggle_popup_first(
        &mut self,
        qh: &QueueHandle<Self>,
        name: &str,
    ) -> Result<String> {
        #[cfg(mhypr_module = "monitor")]
        let bar_index = hyprland::monitor_infos()?
            .into_iter()
            .find(|monitor| monitor.focused)
            .and_then(|monitor| {
                self.bars
                    .iter()
                    .position(|bar| bar.output_name.as_deref() == Some(monitor.name.as_str()))
            })
            .unwrap_or(0);
        #[cfg(not(mhypr_module = "monitor"))]
        let bar_index = 0usize;
        let bar = self
            .bars
            .get(bar_index)
            .context("no bar output is available")?;
        let bar_width = bar.width;
        let bar_height = bar.height;
        let workspace_visible = self.monitor_for_bar(bar_index).is_some();

        let mut start = None;
        let mut end = None;
        for x in 0..bar_width as i32 {
            let is_target = render::module_at_x(
                x as f64,
                bar_width,
                workspace_visible,
                &self.config,
                &self.modules,
            )
            .is_some_and(|hit| hit.name == name);
            if is_target {
                start.get_or_insert(x);
                end = Some(x + 1);
            } else if start.is_some() {
                break;
            }
        }
        let (start, end) = start
            .zip(end)
            .with_context(|| format!("module {name:?} is not visible on the first bar"))?;
        let center = (start + end) as f64 / 2.0;

        match name {
            #[cfg(mhypr_module = "menu")]
            "menu" => {
                let origin_y = if self.config.position == "bottom" {
                    bar_height as f64 / 2.0
                } else {
                    bar_height as f64 + 2.0
                };
                let _ = self.modules.activate_at(name, center, origin_y)?;
            }
            #[cfg(mhypr_module = "active_window")]
            "active_window" => {
                let _ = self.toggle_active_window_popup(qh, bar_index, center, false)?;
            }
            #[cfg(mhypr_module = "audio")]
            "audio" => {
                let _ = self.toggle_audio_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "brightness")]
            "brightness" => {
                let _ = self.toggle_brightness_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "battery")]
            "battery" => {
                let _ = self.toggle_battery_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "clock")]
            "clock" => {
                let _ = self.toggle_clock_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "cpu")]
            "cpu" => {
                let _ = self.toggle_cpu_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "gpu")]
            "gpu" => {
                let _ = self.toggle_gpu_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "monitor")]
            "monitor" => {
                let _ = self.toggle_monitor_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "network")]
            "network" => {
                let _ = self.toggle_network_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "disk")]
            "disk" => {
                let _ = self.toggle_disk_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "memory")]
            "memory" => {
                let _ = self.toggle_memory_popup(qh, bar_index, center)?;
            }
            #[cfg(mhypr_module = "layout")]
            "layout" => {
                let _ = self.toggle_layout_popup(qh, bar_index, center)?;
            }
            _ => anyhow::bail!("module {name:?} does not expose a debug popup"),
        }

        let monitor = self
            .bars
            .get(bar_index)
            .and_then(|bar| bar.output_name.as_deref())
            .unwrap_or("unknown");
        if name == "menu" {
            return Ok(format!("monitor={monitor} external=true"));
        }
        let Some((x, y, w, h)) = self.popup_debug_geometry(name) else {
            return Ok(format!("monitor={monitor} closed=true"));
        };
        Ok(format!(
            "monitor={monitor} x={x:.0} y={y:.0} w={w} h={h}"
        ))
    }

    pub(super) fn popup_debug_info(&self, name: &str) -> Result<String> {
        #[cfg(mhypr_module = "monitor")]
        let monitor = hyprland::monitor_infos()?
            .into_iter()
            .find(|monitor| monitor.focused)
            .map(|monitor| monitor.name)
            .unwrap_or_else(|| "unknown".into());
        #[cfg(not(mhypr_module = "monitor"))]
        let monitor = "unknown".to_owned();

        let (x, y, w, h) = self
            .popup_debug_geometry(name)
            .with_context(|| format!("popup {name:?} is not open"))?;
        let mut info = format!("monitor={monitor} x={x:.0} y={y:.0} w={w} h={h}");
        #[cfg(mhypr_module = "cpu")]
        if name == "cpu"
            && let Some(tooltip) = self
                .tooltip
                .as_ref()
                .filter(|tooltip| tooltip.cpu_pid.is_some())
            && let Some((tx, ty)) = tooltip.debug_origin
        {
            info.push_str(&format!(
                " tooltip_x={tx} tooltip_y={ty} tooltip_w={} tooltip_h={}",
                tooltip.width, tooltip.height
            ));
        }
        Ok(info)
    }

    fn popup_debug_geometry(&self, name: &str) -> Option<(f64, f64, i32, i32)> {
        match name {
            #[cfg(mhypr_module = "active_window")]
            "active_window" => self.active_window_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "audio")]
            "audio" => self.audio_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "brightness")]
            "brightness" => self.brightness_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "battery")]
            "battery" => self.battery_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "clock")]
            "clock" => self.clock_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "cpu")]
            "cpu" => self.cpu_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "gpu")]
            "gpu" => self.gpu_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "monitor")]
            "monitor" => self.monitor_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "network")]
            "network" => self.network_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "disk")]
            "disk" => self.disk_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "memory")]
            "memory" => self.memory_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            #[cfg(mhypr_module = "layout")]
            "layout" => self.layout_popup.as_ref().map(|popup| {
                (
                    popup.panel_x,
                    popup.panel_y,
                    popup.model.config.width,
                    popup.model.panel_height(),
                )
            }),
            _ => None,
        }
    }

    pub(super) fn tray_action_at(
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

    pub(super) fn activate_module_at(&mut self, _qh: &QueueHandle<Self>, bar_index: usize, x: f64, y: f64) {
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

    #[cfg(mhypr_module = "monitor")]
    pub(super) fn activate_monitor_missing_popup(
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
        if hit.name != "monitor" {
            return false;
        }

        let Some(target_monitor) = self
            .monitor_for_bar(bar_index)
            .map(|monitor| monitor.name.clone())
        else {
            return true;
        };
        let missing_windows = match self.monitor_hotplug.missing_windows() {
            Ok(windows) => windows,
            Err(error) => {
                eprintln!("mhyprbar: failed to read disconnected display windows: {error:#}");
                Vec::new()
            }
        };
        if let Err(error) = self.toggle_monitor_missing_popup(
            qh,
            bar_index,
            x,
            target_monitor,
            missing_windows,
        ) {
            eprintln!("mhyprbar: monitor missing-window popup action failed: {error:#}");
        }
        true
    }

    #[cfg(mhypr_module = "layout")]
    pub(super) fn activate_layout_popup(
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
    pub(super) fn activate_active_window_monitor_popup(
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

    pub(super) fn scroll_module_at(&mut self, bar_index: usize, x: f64, delta: i32) -> bool {
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
