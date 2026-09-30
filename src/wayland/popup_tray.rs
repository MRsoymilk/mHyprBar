use super::*;

impl App {
    pub(super) fn tray_hover_timeout(&self) -> Option<Duration> {
        let hover = self.tray_hover.as_ref()?;
        if self.tooltip.as_ref().is_some_and(|tooltip| {
            tooltip.bar_index == hover.bar_index && tooltip.item_index == Some(hover.item_index)
        }) {
            return None;
        }
        Some(hover.deadline.saturating_duration_since(Instant::now()))
    }

    pub(super) fn update_tray_hover(&mut self, bar_index: usize, x: f64) {
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

    pub(super) fn refresh_tray_hover_after_change(&mut self) {
        let Some((bar_index, local_x)) = self
            .tray_hover
            .as_ref()
            .map(|hover| (hover.bar_index, hover.local_x))
        else {
            return;
        };
        self.update_tray_hover(bar_index, local_x);
    }

    pub(super) fn clear_tray_hover(&mut self) {
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
    pub(super) fn clear_layout_tooltip(&mut self) {
        if self
            .tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.item_index.is_none() && tooltip.cpu_pid.is_none())
        {
            self.tooltip = None;
        }
    }

    #[cfg(mhypr_module = "layout")]
    pub(super) fn update_layout_tooltip(&mut self, qh: &QueueHandle<Self>, bar_index: usize, x: f64) {
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
            tooltip.bar_index == bar_index
                && tooltip.item_index.is_none()
                && tooltip.cpu_pid.is_none()
                && tooltip.text == text
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
            cpu_pid: None,
            #[cfg(mhypr_module = "cpu")]
            cpu_process: None,
            #[cfg(mhypr_module = "cpu")]
            cpu_accent: None,
        });
    }

    pub(super) fn maybe_show_tray_tooltip(&mut self, qh: &QueueHandle<Self>) {
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
            cpu_pid: None,
            #[cfg(mhypr_module = "cpu")]
            cpu_process: None,
            #[cfg(mhypr_module = "cpu")]
            cpu_accent: None,
        });
    }

    pub(super) fn draw_tooltip(&mut self) {
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
        #[cfg(mhypr_module = "cpu")]
        let cpu_process = tooltip.cpu_process.clone();
        #[cfg(mhypr_module = "cpu")]
        let cpu_accent = tooltip.cpu_accent;
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

        #[cfg(mhypr_module = "cpu")]
        let render_result = if let Some(process) = cpu_process.as_ref() {
            self.renderer.draw_cpu_process_tooltip(
                canvas,
                width,
                height,
                process,
                &style,
                cpu_accent.unwrap_or([242, 242, 242, 255]),
            )
        } else {
            self.renderer
                .draw_tooltip(canvas, width, height, &text, &style)
        };
        #[cfg(not(mhypr_module = "cpu"))]
        let render_result = self
            .renderer
            .draw_tooltip(canvas, width, height, &text, &style);

        if let Err(error) = render_result {
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
    pub(super) fn open_tray_popup_request(
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
    pub(super) fn tray_popup_item_gone(&self) -> bool {
        self.tray_popup.as_ref().is_some_and(|popup| {
            !self
                .tray
                .as_ref()
                .is_some_and(|tray| tray.has_item(&popup.item_id))
        })
    }

    #[cfg(not(mhypr_module = "tray"))]
    pub(super) fn tray_popup_item_gone(&self) -> bool {
        false
    }

    #[cfg(mhypr_module = "tray")]
    pub(super) fn close_tray_popup(&mut self) {
        self.tray_popup = None;
    }

    #[cfg(not(mhypr_module = "tray"))]
    pub(super) fn close_tray_popup(&mut self) {}

    #[cfg(mhypr_module = "tray")]
    pub(super) fn tray_popup_matches_item_at(&self, bar_index: usize, x: f64) -> bool {
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
    pub(super) fn tray_popup_matches_item_at(&self, _bar_index: usize, _x: f64) -> bool {
        false
    }

    #[cfg(mhypr_module = "tray")]
    pub(super) fn open_tray_popup_at(
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
    pub(super) fn open_tray_popup_at(
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
    pub(super) fn open_tray_popup_index(&mut self, _qh: &QueueHandle<Self>, _index: usize) -> Result<()> {
        anyhow::bail!("tray module is not compiled")
    }

    #[cfg(mhypr_module = "tray")]
    pub(super) fn open_tray_popup_index(&mut self, qh: &QueueHandle<Self>, index: usize) -> Result<()> {
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
    pub(super) fn draw_tray_popup(&mut self) {
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
    pub(super) fn activate_tray_popup_at(&mut self, x: f64, y: f64) {
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

}
