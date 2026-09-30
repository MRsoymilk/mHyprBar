use super::*;
use super::{layout::*, primitives::*};

impl Renderer {
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        background: [u8; 4],
        config: &BarConfig,
        modules: &ModuleManager,
        tray: Option<&TrayState>,
        monitor: Option<&MonitorState>,
        snapshot: &Snapshot,
    ) -> Result<()> {
        canvas.fill(0);
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: 0,
                y: 0,
                w: width as i32,
                h: height as i32,
            },
            background,
        );

        let left_width = group_width(&config.left, modules, config.height as i32);
        let workspace_width = self.draw_workspaces(
            canvas,
            width,
            height,
            left_width,
            &config.workspaces,
            monitor,
            snapshot,
        )?;
        let (left_x, center_x, right_x) = group_origins(width, workspace_width, config, modules);

        self.draw_group(canvas, width, height, left_x, &config.left, modules, tray)?;
        self.draw_group(
            canvas,
            width,
            height,
            center_x,
            &config.center,
            modules,
            tray,
        )?;
        self.draw_group(canvas, width, height, right_x, &config.right, modules, tray)?;

        Ok(())
    }

    fn draw_workspaces(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        start_x: i32,
        style: &WorkspacesConfig,
        monitor: Option<&MonitorState>,
        snapshot: &Snapshot,
    ) -> Result<i32> {
        let Some(monitor) = monitor else {
            return Ok(0);
        };

        let text_rgba = style.text_rgba()?;
        let text_color = Color::rgba(text_rgba[0], text_rgba[1], text_rgba[2], text_rgba[3]);
        let empty = style.empty_rgba()?;
        let occupied = style.occupied_rgba()?;
        let active = style.active_rgba()?;

        for local in 1..=style.count {
            let global = style.global_workspace_id(&monitor.name, monitor.id, local);
            let background = if monitor.active_workspace == global {
                active
            } else if snapshot.workspace_windows(global) > 0 {
                occupied
            } else {
                empty
            };

            let x = start_x.saturating_add(
                (local as i32 - 1).saturating_mul(style.width.saturating_add(style.gap)),
            );
            let rect = Rect {
                x,
                y: 0,
                w: style.width,
                h: height as i32,
            };
            if background[3] > 0 {
                fill_rect(canvas, width, height, rect, background);
            }

            self.draw_workspace_label(
                canvas,
                width,
                height,
                rect,
                &local.to_string(),
                style,
                text_color,
            );
            self.draw_workspace_window_dots(
                canvas,
                width,
                height,
                rect,
                snapshot.workspace_windows(global),
                style,
                text_rgba,
            );
        }

        Ok(style.strip_width())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_workspace_window_dots(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        windows: u32,
        style: &WorkspacesConfig,
        color: [u8; 4],
    ) {
        if windows == 0 {
            return;
        }

        let dot_w = style.window_dot_width.max(1);
        let dot_h = style.window_dot_height.max(1);
        let bar_w = style.window_group_bar_width.max(1);
        let bar_h = style.window_group_bar_height.max(1);
        let gap = style.window_dot_gap.max(0);
        let mut x = rect.x.saturating_add(style.label_padding_x);
        let bottom = rect
            .y
            .saturating_add(rect.h)
            .saturating_sub(style.window_dot_bottom);

        let bars = windows / 5;
        let dots = windows % 5;

        for index in 0..bars {
            if index > 0 {
                x = x.saturating_add(gap);
            }
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x,
                    y: bottom.saturating_sub(bar_h),
                    w: bar_w,
                    h: bar_h,
                },
                color,
            );
            x = x.saturating_add(bar_w);
        }

        for index in 0..dots {
            if bars > 0 || index > 0 {
                x = x.saturating_add(gap);
            }
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x,
                    y: bottom.saturating_sub(dot_h),
                    w: dot_w,
                    h: dot_h,
                },
                color,
            );
            x = x.saturating_add(dot_w);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_workspace_label(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        text: &str,
        style: &WorkspacesConfig,
        color: Color,
    ) {
        let text_x = rect.x.saturating_add(style.label_padding_x);
        let text_y = rect.y.saturating_add(style.label_padding_y);
        let text_w = rect
            .w
            .saturating_sub(style.label_padding_x.saturating_mul(2))
            .max(1) as f32;
        let line_height = (style.font_size * 1.15).ceil().max(style.font_size);
        let text_h = line_height.ceil().max(1.0);

        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.font_size, line_height));
        buffer.set_size(Some(text_w), Some(text_h));
        let family = match style.font_family.as_str() {
            "sans-serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            name => Family::Name(name),
        };
        let attrs = Attrs::new().family(family);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            color,
            |x, y, w, h, pixel| {
                blend_block(
                    canvas,
                    width,
                    height,
                    text_x.saturating_add(x),
                    text_y.saturating_add(y),
                    w,
                    h,
                    pixel,
                );
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_group(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        mut x: i32,
        names: &[String],
        modules: &ModuleManager,
        tray: Option<&TrayState>,
    ) -> Result<()> {
        for name in names {
            let Some(view) = modules.view(name) else {
                continue;
            };
            let module_width = module_width(&view, height as i32);
            if module_width <= 0 {
                continue;
            }
            let rect = Rect {
                x,
                y: 0,
                w: module_width,
                h: height as i32,
            };

            let background = view.style.background_rgba()?;
            if background[3] > 0 {
                fill_rect(canvas, width, height, rect, background);
            }

            match &view.visual {
                #[cfg(mhypr_module = "active_window")]
                ModuleVisual::ActiveWindow(active_window) => {
                    self.draw_active_window(canvas, width, height, rect, &view, active_window)?;
                }
                #[cfg(mhypr_module = "audio")]
                ModuleVisual::Audio(audio) => {
                    self.draw_audio(canvas, width, height, rect, &view, audio)?;
                }
                #[cfg(mhypr_module = "battery")]
                ModuleVisual::Battery(battery) => {
                    self.draw_battery(canvas, width, height, rect, &view, battery)?;
                }
                #[cfg(mhypr_module = "brightness")]
                ModuleVisual::Brightness(brightness) => {
                    self.draw_brightness(canvas, width, height, rect, &view, brightness)?;
                }
                #[cfg(mhypr_module = "clock")]
                ModuleVisual::Clock(clock) => {
                    self.draw_clock(canvas, width, height, rect, &view, clock)?;
                }
                #[cfg(mhypr_module = "layout")]
                ModuleVisual::Layout(layout) => {
                    self.draw_layout(canvas, width, height, rect, &view, layout)?;
                }
                #[cfg(mhypr_module = "cpu")]
                ModuleVisual::Cpu(cpu) => {
                    self.draw_cpu(canvas, width, height, rect, &view, cpu)?;
                }
                #[cfg(mhypr_module = "disk")]
                ModuleVisual::Disk(disk) => {
                    self.draw_disk(canvas, width, height, rect, &view, disk)?;
                }
                #[cfg(mhypr_module = "gpu")]
                ModuleVisual::Gpu(gpu) => {
                    self.draw_gpu(canvas, width, height, rect, &view, gpu)?;
                }
                #[cfg(mhypr_module = "monitor")]
                ModuleVisual::Monitor(monitor) => {
                    self.draw_monitor(canvas, width, height, rect, &view, monitor)?;
                }
                #[cfg(mhypr_module = "memory")]
                ModuleVisual::Memory(memory) => {
                    self.draw_memory(canvas, width, height, rect, &view, memory)?;
                }
                #[cfg(mhypr_module = "menu")]
                ModuleVisual::Menu(menu) => {
                    self.draw_menu(canvas, width, height, rect, &view, menu);
                }
                #[cfg(mhypr_module = "network")]
                ModuleVisual::Network(network) => {
                    self.draw_network(canvas, width, height, rect, &view, network)?;
                }
                ModuleVisual::Text => {
                    if name == "tray" {
                        if let Some(tray) = tray {
                            self.draw_tray(canvas, width, height, rect, tray, &view);
                        }
                    } else {
                        self.draw_text(canvas, width, height, rect, &view)?;
                    }
                }
            }
            x = x.saturating_add(module_width);
        }
        Ok(())
    }

    #[cfg(mhypr_module = "active_window")]
    fn draw_active_window(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        active_window: &crate::modules::active_window::ActiveWindowVisual,
    ) -> Result<()> {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let mut text_x = rect.x.saturating_add(padding_x);

        if let Some(icon) = active_window.icon.as_ref() {
            let icon_size = active_window.icon_size.min(content_h).max(1);
            let icon_y = rect
                .y
                .saturating_add(padding_y)
                .saturating_add((content_h - icon_size) / 2);
            draw_native_argb_pixmap(
                canvas,
                width,
                height,
                text_x,
                icon_y,
                icon_size,
                icon.width,
                icon.height,
                &icon.pixels,
            );
            text_x = text_x.saturating_add(icon_size);
            if !view.text.is_empty() {
                text_x = text_x.saturating_add(active_window.icon_gap);
            }
        }

        if view.text.is_empty() {
            return Ok(());
        }
        let right = rect
            .x
            .saturating_add(rect.w)
            .saturating_sub(padding_x);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: text_x,
                y: rect.y,
                w: right.saturating_sub(text_x).max(1),
                h: rect.h,
            },
            view.text,
            view.style,
            0,
            padding_y,
        )
    }

    #[cfg(mhypr_module = "network")]
    fn draw_network(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        network: &crate::modules::network::NetworkVisual,
    ) -> Result<()> {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(2);
        let row_h = (content_h / 2).max(1);
        let mut row_style = view.style.clone();
        row_style.font_size = (view.style.font_size - 1.0).max(8.0);
        let x = rect.x.saturating_add(padding_x);
        let w = rect.w.saturating_sub(padding_x.saturating_mul(2)).max(1);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect { x, y: rect.y + padding_y, w, h: row_h },
            &network.download_text,
            &row_style,
            0,
            0,
        )?;
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect { x, y: rect.y + padding_y + row_h, w, h: content_h - row_h },
            &network.upload_text,
            &row_style,
            0,
            0,
        )?;
        Ok(())
    }

    #[cfg(mhypr_module = "audio")]
    fn draw_audio(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        audio: &crate::modules::audio::AudioVisual,
    ) -> Result<()> {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let icon_size = audio_icon_slot_size(audio, view.style, rect.h);
        let icon_x = rect.x.saturating_add(padding_x);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);
        let color = if audio.muted {
            audio.muted_fill
        } else {
            audio.fill
        };

        draw_tinted_rgba_mask(
            canvas,
            width,
            height,
            icon_x,
            icon_y,
            icon_size,
            audio.icon_width,
            audio.icon_height,
            &audio.icon_pixels,
            color,
        );

        let text_x = icon_x
            .saturating_add(icon_size)
            .saturating_add(audio.text_gap);
        let right = rect.x.saturating_add(rect.w).saturating_sub(padding_x);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: text_x,
                y: rect.y,
                w: right.saturating_sub(text_x).max(1),
                h: rect.h,
            },
            &format!("{}%", audio.percent),
            view.style,
            0,
            padding_y,
        )?;
        Ok(())
    }

    #[cfg(mhypr_module = "brightness")]
    fn draw_brightness(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        brightness: &crate::modules::brightness::BrightnessVisual,
    ) -> Result<()> {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let icon_size = brightness_icon_slot_size(brightness, view.style, rect.h);
        let icon_x = rect.x.saturating_add(padding_x);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);

        draw_tinted_rgba_mask(
            canvas,
            width,
            height,
            icon_x,
            icon_y,
            icon_size,
            brightness.icon_width,
            brightness.icon_height,
            &brightness.icon_pixels,
            brightness.fill,
        );

        let text_x = icon_x
            .saturating_add(icon_size)
            .saturating_add(brightness.text_gap);
        let right = rect.x.saturating_add(rect.w).saturating_sub(padding_x);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: text_x,
                y: rect.y,
                w: right.saturating_sub(text_x).max(1),
                h: rect.h,
            },
            &format!("{}%", brightness.percent),
            view.style,
            0,
            padding_y,
        )?;
        Ok(())
    }

    #[cfg(mhypr_module = "menu")]
    fn draw_menu(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        menu: &crate::modules::menu::MenuVisual,
    ) {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_w = rect.w.saturating_sub(padding_x.saturating_mul(2)).max(1);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let icon_size = ((content_h as f32 * menu.icon_scale).round() as i32)
            .clamp(1, content_h.min(content_w));
        let icon_x = rect
            .x
            .saturating_add(padding_x)
            .saturating_add((content_w - icon_size) / 2);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);
        draw_rgba_pixmap(
            canvas,
            width,
            height,
            icon_x,
            icon_y,
            icon_size,
            menu.icon_width,
            menu.icon_height,
            &menu.icon_pixels,
        );
    }

    #[cfg(mhypr_module = "layout")]
    fn draw_layout(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        layout: &crate::modules::layout::LayoutVisual,
    ) -> Result<()> {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let icon_size = layout_icon_slot_size(layout, view.style, rect.h);
        let icon_x = rect.x.saturating_add(padding_x);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);
        let color = view.style.foreground_rgba()?;

        if let Some(pixels) = layout.icon_pixels.as_ref() {
            draw_tinted_rgba_mask(
                canvas,
                width,
                height,
                icon_x,
                icon_y,
                icon_size,
                layout.icon_width,
                layout.icon_height,
                pixels,
                color,
            );
        } else {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: rect.x.saturating_add(padding_x),
                    y: rect.y,
                    w: rect.w.saturating_sub(padding_x.saturating_mul(2)).max(1),
                    h: rect.h,
                },
                &layout.name,
                view.style,
                0,
                padding_y,
            )?;
        }

        Ok(())
    }

    #[cfg(mhypr_module = "battery")]
    fn draw_battery(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        battery: &crate::modules::battery::BatteryVisual,
    ) -> Result<()> {
        let style = view.style;
        let padding_x = style.padding_x.max(0);
        let padding_y = style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let icon_h = battery.icon_height.min(content_h).max(5);
        let body = Rect {
            x: rect.x.saturating_add(padding_x),
            y: rect
                .y
                .saturating_add(padding_y)
                .saturating_add((content_h - icon_h) / 2),
            w: battery.icon_width,
            h: icon_h,
        };
        let color = battery_level_color(battery);

        fill_rect(canvas, width, height, body, battery.icon_background);
        let border = battery
            .icon_border_width
            .min((body.w.min(body.h) / 2).max(1));
        draw_rect_border(canvas, width, height, body, color, border);

        // Keep a visible dark inset between the colored shell and the charge fill.
        let inner_gap = 2;
        let inset = border.saturating_add(inner_gap);
        let inner = Rect {
            x: body.x.saturating_add(inset),
            y: body.y.saturating_add(inset),
            w: body.w.saturating_sub(inset.saturating_mul(2)).max(0),
            h: body.h.saturating_sub(inset.saturating_mul(2)).max(0),
        };
        let fill_w = ((inner.w as f32 * battery.capacity.clamp(0.0, 100.0) / 100.0).round() as i32)
            .clamp(0, inner.w);
        if fill_w > 0 {
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: inner.x,
                    y: inner.y,
                    w: fill_w,
                    h: inner.h,
                },
                color,
            );
        }

        let tip_gap = 1;
        let tip_h = (icon_h / 2).max(4);
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: body.x.saturating_add(body.w).saturating_add(tip_gap),
                y: body.y.saturating_add((icon_h - tip_h) / 2),
                w: battery.icon_tip_width,
                h: tip_h,
            },
            color,
        );

        if battery.charging && inner.w >= 7 && inner.h >= 7 {
            draw_battery_bolt(canvas, width, height, inner, [20, 24, 20, 255]);
        }

        let icon_total = battery
            .icon_width
            .saturating_add(1)
            .saturating_add(battery.icon_tip_width);
        let text_x = body
            .x
            .saturating_add(icon_total)
            .saturating_add(battery.text_gap);
        let right = rect.x.saturating_add(rect.w).saturating_sub(padding_x);
        let text_w = right.saturating_sub(text_x).max(1);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: text_x,
                y: rect.y,
                w: text_w,
                h: rect.h,
            },
            &format!("{:.0}%", battery.capacity),
            style,
            0,
            padding_y,
        )?;
        Ok(())
    }

    #[cfg(mhypr_module = "clock")]
    fn draw_clock(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        clock: &crate::modules::clock::ClockVisual,
    ) -> Result<()> {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let inner_w = rect.w.saturating_sub(padding_x.saturating_mul(2)).max(1);
        let inner_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);

        let mut time_style = view.style.clone();
        time_style.font_size = clock.time_font_size;

        if clock.date_text.is_empty() {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: rect.x + padding_x,
                    y: rect.y,
                    w: inner_w,
                    h: rect.h,
                },
                &clock.time_text,
                &time_style,
                0,
                padding_y,
            )?;
            return Ok(());
        }

        let mut date_style = view.style.clone();
        date_style.font_size = clock.date_font_size;
        date_style.foreground = "#9AA0AA".into();

        let date_h = (clock.date_font_size * 1.25).ceil() as i32;
        let time_h = (clock.time_font_size * 1.25).ceil() as i32;
        let content_h = date_h
            .saturating_add(clock.row_gap)
            .saturating_add(time_h)
            .min(inner_h);
        let start_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((inner_h - content_h).max(0) / 2);

        let date_w = estimate_text_width(&clock.date_text, &date_style)
            .min(inner_w)
            .max(1);
        let date_x = rect
            .x
            .saturating_add(padding_x)
            .saturating_add((inner_w - date_w).max(0) / 2);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: date_x,
                y: start_y,
                w: date_w,
                h: date_h,
            },
            &clock.date_text,
            &date_style,
            0,
            0,
        )?;

        let time_y = start_y.saturating_add(date_h).saturating_add(clock.row_gap);
        let time_w = estimate_text_width(&clock.time_text, &time_style)
            .min(inner_w)
            .max(1);
        let time_x = rect
            .x
            .saturating_add(padding_x)
            .saturating_add((inner_w - time_w).max(0) / 2);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: time_x,
                y: time_y,
                w: time_w,
                h: time_h,
            },
            &clock.time_text,
            &time_style,
            0,
            0,
        )?;
        Ok(())
    }

    #[cfg(mhypr_module = "cpu")]
    fn draw_cpu(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        cpu: &crate::modules::cpu::CpuVisual,
    ) -> Result<()> {
        let style = view.style;
        let padding_x = style.padding_x.max(0);
        let padding_y = style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);

        let icon_size = cpu_icon_slot_size(cpu, style, rect.h);
        let icon_x = rect.x.saturating_add(padding_x);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);
        draw_tinted_rgba_mask(
            canvas,
            width,
            height,
            icon_x,
            icon_y,
            icon_size,
            cpu.icon_width,
            cpu.icon_height,
            &cpu.icon_pixels,
            style.foreground_rgba()?,
        );

        let has_following = cpu.graph_enabled || !view.text.is_empty();
        let mut cursor_x = icon_x
            .saturating_add(icon_size)
            .saturating_add(if has_following { cpu.icon_gap } else { 0 });
        if cpu.graph_enabled {
            let graph_h = if cpu.graph_height == 0 {
                content_h
            } else {
                cpu.graph_height.min(content_h).max(1)
            };
            let graph_y = rect
                .y
                .saturating_add(padding_y)
                .saturating_add((content_h - graph_h) / 2);
            let graph_rect = Rect {
                x: cursor_x,
                y: graph_y,
                w: cpu.graph_width,
                h: graph_h,
            };

            if cpu.graph_background[3] > 0 {
                fill_rect(canvas, width, height, graph_rect, cpu.graph_background);
            }

            let stride = cpu.step_width.saturating_add(cpu.step_spacing).max(1);
            let used_width = if cpu.history.is_empty() {
                0
            } else {
                (cpu.history.len() as i32)
                    .saturating_mul(stride)
                    .saturating_sub(cpu.step_spacing)
            };
            let mut step_x = graph_rect
                .x
                .saturating_add((graph_rect.w - used_width).max(0));

            for &sample in &cpu.history {
                let bar_h = if sample <= 0.0 {
                    0
                } else {
                    ((sample.clamp(0.0, 100.0) / 100.0) * graph_h as f32)
                        .round()
                        .max(1.0) as i32
                }
                .min(graph_h);

                let top = graph_y.saturating_add(graph_h.saturating_sub(bar_h));
                for row in 0..bar_h {
                    let y = top.saturating_add(row);
                    let level = if graph_h <= 1 {
                        1.0
                    } else {
                        1.0 - (y - graph_y) as f32 / (graph_h - 1) as f32
                    };
                    let color = cpu_graph_color(cpu, level);
                    fill_rect(
                        canvas,
                        width,
                        height,
                        Rect {
                            x: step_x,
                            y,
                            w: cpu.step_width.min(
                                graph_rect
                                    .x
                                    .saturating_add(graph_rect.w)
                                    .saturating_sub(step_x),
                            ),
                            h: 1,
                        },
                        color,
                    );
                }

                step_x = step_x.saturating_add(stride);
                if step_x >= graph_rect.x.saturating_add(graph_rect.w) {
                    break;
                }
            }

            cursor_x =
                cursor_x
                    .saturating_add(cpu.graph_width)
                    .saturating_add(if view.text.is_empty() {
                        0
                    } else {
                        cpu.text_gap
                    });
        }

        if !view.text.is_empty() {
            let right = rect.x.saturating_add(rect.w).saturating_sub(padding_x);
            let text_w = right.saturating_sub(cursor_x).max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: cursor_x,
                    y: rect.y,
                    w: text_w,
                    h: rect.h,
                },
                view.text,
                style,
                0,
                padding_y,
            )?;
        }

        Ok(())
    }

    #[cfg(mhypr_module = "disk")]
    fn draw_disk(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        disk: &crate::modules::disk::DiskVisual,
    ) -> Result<()> {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let icon_size = disk_icon_slot_size(disk, view.style, rect.h);
        let icon_x = rect.x.saturating_add(padding_x);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);
        let icon_color = view.style.foreground_rgba()?;

        draw_tinted_rgba_mask(
            canvas,
            width,
            height,
            icon_x,
            icon_y,
            icon_size,
            disk.icon_width,
            disk.icon_height,
            &disk.icon_pixels,
            icon_color,
        );

        let bar_h = disk.bar_height.min(content_h).max(1);
        let bar = Rect {
            x: icon_x
                .saturating_add(icon_size)
                .saturating_add(disk.icon_gap),
            y: rect
                .y
                .saturating_add(padding_y)
                .saturating_add((content_h - bar_h) / 2),
            w: disk.bar_width,
            h: bar_h,
        };

        fill_rect(canvas, width, height, bar, disk.bar_background);
        let border = disk.bar_border_width.min((bar.w.min(bar.h) / 2).max(0));
        let inner = Rect {
            x: bar.x.saturating_add(border),
            y: bar.y.saturating_add(border),
            w: bar.w.saturating_sub(border.saturating_mul(2)).max(0),
            h: bar.h.saturating_sub(border.saturating_mul(2)).max(0),
        };
        let fill_w = ((inner.w as f32 * disk.percent.clamp(0.0, 100.0) / 100.0).round() as i32)
            .clamp(0, inner.w);
        if fill_w > 0 {
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: inner.x,
                    y: inner.y,
                    w: fill_w,
                    h: inner.h,
                },
                disk.bar_fill,
            );
        }
        draw_rect_border(
            canvas,
            width,
            height,
            bar,
            disk.bar_border,
            disk.bar_border_width,
        );

        Ok(())
    }

    #[cfg(mhypr_module = "gpu")]
    fn draw_gpu(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        gpu: &crate::modules::gpu::GpuVisual,
    ) -> Result<()> {
        let style = view.style;
        let padding_x = style.padding_x.max(0);
        let padding_y = style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(2);
        let gap = gpu.row_gap.min(content_h.saturating_sub(2)).max(0);
        let row_h = ((content_h - gap) / 2).max(1);
        let percent_w = estimate_text_width("100%", style).max(1);
        let start_x = rect.x.saturating_add(padding_x);
        let (icon_w, _icon_h) = gpu_icon_dimensions(gpu, style, rect.h);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_w) / 2);
        let icon_color = style.foreground_rgba()?;

        draw_tinted_rgba_mask(
            canvas,
            width,
            height,
            start_x,
            icon_y,
            icon_w,
            gpu.icon_width,
            gpu.icon_height,
            &gpu.icon_pixels,
            icon_color,
        );

        let bar_x = start_x.saturating_add(icon_w).saturating_add(gpu.icon_gap);
        let percent_x = bar_x
            .saturating_add(gpu.bar_width)
            .saturating_add(gpu.text_gap);

        for (index, (percent, fill)) in [
            (gpu.utilization_percent, gpu.utilization_fill),
            (gpu.memory_percent, gpu.memory_fill),
        ]
        .into_iter()
        .enumerate()
        {
            let row_y = rect
                .y
                .saturating_add(padding_y)
                .saturating_add(index as i32 * (row_h + gap));

            let bar_h = gpu.bar_height.min(row_h).max(1);
            let bar_rect = Rect {
                x: bar_x,
                y: row_y.saturating_add((row_h - bar_h) / 2),
                w: gpu.bar_width,
                h: bar_h,
            };
            fill_rect(canvas, width, height, bar_rect, gpu.bar_background);
            if let Some(percent) = percent {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: bar_rect.x,
                        y: bar_rect.y,
                        w: ((bar_rect.w as f32 * percent.clamp(0.0, 100.0) / 100.0).round() as i32)
                            .clamp(0, bar_rect.w),
                        h: bar_rect.h,
                    },
                    fill,
                );
            }

            let text = percent
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| "--%".into());
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: percent_x,
                    y: row_y,
                    w: percent_w,
                    h: row_h,
                },
                &text,
                style,
                0,
                0,
            )?;
        }

        Ok(())
    }

    #[cfg(mhypr_module = "monitor")]
    fn draw_monitor(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        monitor: &crate::modules::monitor::MonitorVisual,
    ) -> Result<()> {
        let style = view.style;
        let padding_x = style.padding_x.max(0);
        let padding_y = style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let icon_size = monitor_icon_slot_size(monitor, style, rect.h);
        let icon_x = rect.x.saturating_add(padding_x);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);
        draw_tinted_rgba_mask(
            canvas,
            width,
            height,
            icon_x,
            icon_y,
            icon_size,
            monitor.icon_width,
            monitor.icon_height,
            &monitor.icon_pixels,
            style.foreground_rgba()?,
        );

        let topo_x = icon_x
            .saturating_add(icon_size)
            .saturating_add(monitor.icon_gap);
        let topo_y = rect
            .y
            .saturating_add((rect.h - monitor.topology_height).max(0) / 2);
        let inset = (monitor.dot_size / 2).max(1);
        let usable_w = (monitor.topology_width - inset * 2).max(1);
        let usable_h = (monitor.topology_height - inset * 2).max(1);

        for dot in &monitor.dots {
            let cx = topo_x
                .saturating_add(inset)
                .saturating_add((dot.x * usable_w as f32).round() as i32);
            let cy = topo_y
                .saturating_add(inset)
                .saturating_add((dot.y * usable_h as f32).round() as i32);
            fill_circle(
                canvas,
                width,
                height,
                cx,
                cy,
                monitor.dot_size,
                if dot.focused {
                    monitor.focused_dot_color
                } else {
                    monitor.dot_color
                },
            );
        }

        Ok(())
    }

    #[cfg(mhypr_module = "memory")]
    fn draw_memory(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
        memory: &crate::modules::memory::MemoryVisual,
    ) -> Result<()> {
        let style = view.style;
        let padding_x = style.padding_x.max(0);
        let padding_y = style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(2);
        let gap = memory.row_gap.min(content_h.saturating_sub(2)).max(0);
        let row_h = ((content_h - gap) / 2).max(1);
        let icon_size = memory_icon_slot_size(memory, style, rect.h);
        let percent_w = estimate_text_width("100%", style).max(1);
        let start_x = rect.x.saturating_add(padding_x);
        let bar_x = start_x
            .saturating_add(icon_size)
            .saturating_add(memory.icon_gap);
        let percent_x = bar_x
            .saturating_add(memory.bar_width)
            .saturating_add(memory.text_gap);

        for (index, (percent, fill, pixels)) in [
            (
                memory.memory_percent,
                memory.memory_fill,
                memory.memory_icon_pixels.as_ref(),
            ),
            (
                memory.swap_percent,
                memory.swap_fill,
                memory.swap_icon_pixels.as_ref(),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let row_y = rect
                .y
                .saturating_add(padding_y)
                .saturating_add(index as i32 * (row_h + gap));
            let icon_y = row_y.saturating_add((row_h - icon_size) / 2);

            draw_tinted_rgba_mask(
                canvas,
                width,
                height,
                start_x,
                icon_y,
                icon_size,
                memory.icon_width,
                memory.icon_height,
                pixels,
                fill,
            );

            let bar_h = memory.bar_height.min(row_h).max(1);
            let bar_rect = Rect {
                x: bar_x,
                y: row_y.saturating_add((row_h - bar_h) / 2),
                w: memory.bar_width,
                h: bar_h,
            };
            fill_rect(canvas, width, height, bar_rect, memory.bar_background);
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: bar_rect.x,
                    y: bar_rect.y,
                    w: ((bar_rect.w as f32 * percent.clamp(0.0, 100.0) / 100.0).round() as i32)
                        .clamp(0, bar_rect.w),
                    h: bar_rect.h,
                },
                fill,
            );

            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: percent_x,
                    y: row_y,
                    w: percent_w,
                    h: row_h,
                },
                &format!("{percent:.0}%"),
                style,
                0,
                0,
            )?;
        }

        Ok(())
    }

    fn draw_tray(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        tray: &TrayState,
        view: &ModuleView<'_>,
    ) {
        let icon_size = tray.icon_size().max(1);
        let mut x = rect.x.saturating_add(tray.padding_x());
        let y = rect
            .y
            .saturating_add((rect.h.saturating_sub(icon_size)).max(0) / 2);
        let mut fallback = view.style.foreground_rgba().unwrap_or([255, 255, 255, 255]);
        fallback[3] = fallback[3].min(96);

        for icon in tray.icons() {
            if let Some(pixels) = icon.pixels {
                draw_native_argb_pixmap(
                    canvas,
                    width,
                    height,
                    x,
                    y,
                    icon_size,
                    icon.width,
                    icon.height,
                    pixels,
                );
            } else {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x,
                        y,
                        w: icon_size,
                        h: icon_size,
                    },
                    fallback,
                );
            }
            x = x.saturating_add(icon_size).saturating_add(tray.spacing());
        }
    }

    fn draw_text(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        view: &ModuleView<'_>,
    ) -> Result<()> {
        self.draw_text_content(
            canvas,
            width,
            height,
            rect,
            view.text,
            view.style,
            view.style.padding_x.max(0),
            view.style.padding_y.max(0),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_text_content(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        text: &str,
        style: &ModuleStyle,
        padding_x: i32,
        padding_y: i32,
    ) -> Result<()> {
        let foreground = style.foreground_rgba()?;
        let color = Color::rgba(foreground[0], foreground[1], foreground[2], foreground[3]);
        let text_x = rect.x.saturating_add(padding_x);
        let text_y = rect.y.saturating_add(padding_y);
        let text_w = rect.w.saturating_sub(padding_x.saturating_mul(2)).max(1) as f32;
        let text_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1) as f32;
        let line_height = text_h.max(style.font_size);

        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.font_size, line_height));
        buffer.set_size(Some(text_w), Some(text_h));

        let family = match style.font_family.as_str() {
            "sans-serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            name => Family::Name(name),
        };
        let attrs = Attrs::new().family(family);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);

        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            color,
            |x, y, w, h, pixel| {
                blend_block(
                    canvas,
                    width,
                    height,
                    text_x.saturating_add(x),
                    text_y.saturating_add(y),
                    w,
                    h,
                    pixel,
                );
            },
        );

        Ok(())
    }

    pub(super) fn draw_wrapped_text_content(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        text: &str,
        style: &ModuleStyle,
    ) -> Result<()> {
        let foreground = style.foreground_rgba()?;
        let color = Color::rgba(foreground[0], foreground[1], foreground[2], foreground[3]);
        let text_w = rect.w.max(1) as f32;
        let text_h = rect.h.max(1) as f32;
        let line_height = (style.font_size * 1.35).max(style.font_size);
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.font_size, line_height));
        buffer.set_size(Some(text_w), Some(text_h));

        let family = match style.font_family.as_str() {
            "sans-serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            name => Family::Name(name),
        };
        let attrs = Attrs::new().family(family);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            color,
            |x, y, w, h, pixel| {
                blend_block(
                    canvas,
                    width,
                    height,
                    rect.x.saturating_add(x),
                    rect.y.saturating_add(y),
                    w,
                    h,
                    pixel,
                );
            },
        );
        Ok(())
    }
}
