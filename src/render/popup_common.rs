use super::*;
#[allow(unused_imports)]
use super::{layout::estimate_text_width, primitives::*};

impl Renderer {
    pub fn tooltip_size(text: &str, style: &ModuleStyle) -> (u32, u32) {
        let estimated_text = (text.chars().count() as f32 * style.font_size * 0.62)
            .ceil()
            .max(1.0) as i32;
        let max_text_width = 440;
        let text_width = estimated_text.min(max_text_width).max(1);
        let lines = ((estimated_text + max_text_width - 1) / max_text_width).clamp(1, 3);
        let line_height = (style.font_size * 1.35).ceil().max(1.0) as i32;
        let width = text_width
            .saturating_add(style.padding_x.max(0).saturating_mul(2))
            .max(48);
        let height = lines
            .saturating_mul(line_height)
            .saturating_add(style.padding_y.max(0).saturating_mul(2))
            .max(24);
        (width as u32, height as u32)
    }

    #[cfg(mhypr_module = "cpu")]
    pub fn cpu_process_tooltip_size(
        process: &crate::modules::cpu::popup::CpuProcessRow,
        style: &ModuleStyle,
    ) -> (u32, u32) {
        let char_width = (style.font_size.max(11.0) * 0.62).max(1.0);
        let longest = [
            process.name.chars().count(),
            process.executable.chars().count(),
            process.working_dir.chars().count(),
            process.command_line.chars().count().min(110),
        ]
        .into_iter()
        .max()
        .unwrap_or(40);
        let width = ((longest as f32 * char_width).ceil() as i32 + 128).clamp(460, 640);
        let args_width = (width - 24).max(1);
        let chars_per_line = ((args_width as f32 / char_width).floor() as usize).max(24);
        let args_lines = process
            .command_line
            .chars()
            .count()
            .div_ceil(chars_per_line)
            .clamp(1, 4) as i32;
        let line_height = (style.font_size.max(11.0) * 1.35).ceil() as i32;
        let height = 196_i32
            .saturating_add(args_lines.saturating_mul(line_height))
            .clamp(220, 292);
        (width as u32, height as u32)
    }

    #[cfg(mhypr_module = "cpu")]
    pub fn draw_cpu_process_tooltip(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        process: &crate::modules::cpu::popup::CpuProcessRow,
        style: &ModuleStyle,
        accent: [u8; 4],
    ) -> Result<()> {
        canvas.fill(0);

        let panel = Rect {
            x: 0,
            y: 0,
            w: width as i32,
            h: height as i32,
        };
        let background = style.background_rgba()?;
        fill_rect(canvas, width, height, panel, background);

        let foreground = style.foreground_rgba()?;
        let border = [foreground[0], foreground[1], foreground[2], 48];
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = 12_i32;
        let content_w = panel.w.saturating_sub(pad * 2).max(1);
        let mut title_style = style.clone();
        title_style.font_size = 14.0;
        title_style.foreground = "#F5F7FA".into();

        let mut value_style = style.clone();
        value_style.font_size = 11.0;
        value_style.foreground = "#E6E9EE".into();

        let mut label_style = style.clone();
        label_style.font_size = 9.5;
        label_style.foreground = "#8F98A6".into();

        let mut meta_style = style.clone();
        meta_style.font_size = 10.0;
        meta_style.foreground = "#B8C0CC".into();

        let mut accent_style = style.clone();
        accent_style.font_size = 12.0;
        accent_style.foreground = format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            accent[0], accent[1], accent[2], accent[3]
        );

        let header_y = pad;
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: pad,
                y: header_y,
                w: 4,
                h: 36,
            },
            accent,
        );

        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: pad + 12,
                y: header_y,
                w: (content_w - 104).max(1),
                h: 24,
            },
            &process.name,
            &title_style,
            0,
            0,
        )?;

        let cpu_text = format!("{:.1}% CPU", process.cpu);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: panel.w - pad - 88,
                y: header_y,
                w: 88,
                h: 24,
            },
            &cpu_text,
            &accent_style,
            0,
            0,
        )?;

        let cpu_bar = Rect {
            x: pad + 12,
            y: header_y + 28,
            w: (content_w - 12).max(1),
            h: 4,
        };
        fill_rect(canvas, width, height, cpu_bar, [255, 255, 255, 18]);
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: cpu_bar.x,
                y: cpu_bar.y,
                w: ((cpu_bar.w as f32 * process.cpu.clamp(0.0, 100.0) / 100.0).round()
                    as i32)
                    .clamp(0, cpu_bar.w),
                h: cpu_bar.h,
            },
            accent,
        );

        let meta_y = header_y + 44;
        let chip_gap = 6;
        let chip_w = (content_w - chip_gap * 3) / 4;
        let state = process.state.as_deref().unwrap_or("-");
        let meta = [
            ("PID", process.pid.to_string()),
            (
                "PPID",
                process
                    .parent_pid
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".into()),
            ),
            ("STATE", state.to_owned()),
            (
                "THREADS",
                process
                    .threads
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".into()),
            ),
        ];
        for (index, (label, value)) in meta.into_iter().enumerate() {
            let x = pad + index as i32 * (chip_w + chip_gap);
            let chip = Rect {
                x,
                y: meta_y,
                w: chip_w.max(1),
                h: 30,
            };
            fill_rect(canvas, width, height, chip, [255, 255, 255, 10]);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: chip.x + 7,
                    y: chip.y + 2,
                    w: (chip.w - 14).max(1),
                    h: 11,
                },
                label,
                &label_style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: chip.x + 7,
                    y: chip.y + 13,
                    w: (chip.w - 14).max(1),
                    h: 15,
                },
                &value,
                &meta_style,
                0,
                0,
            )?;
        }

        let mut y = meta_y + 40;
        for (label, value) in [
            ("EXECUTABLE", process.executable.as_str()),
            ("WORKING DIR", process.working_dir.as_str()),
        ] {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: pad,
                    y,
                    w: 86,
                    h: 26,
                },
                label,
                &label_style,
                0,
                0,
            )?;
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: pad + 92,
                    y,
                    w: (content_w - 92).max(1),
                    h: 26,
                },
                [255, 255, 255, 7],
            );
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: pad + 100,
                    y,
                    w: (content_w - 108).max(1),
                    h: 26,
                },
                value,
                &value_style,
                0,
                0,
            )?;
            y += 32;
        }

        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: pad,
                y,
                w: content_w,
                h: 18,
            },
            "ARGUMENTS",
            &label_style,
            0,
            0,
        )?;
        y += 20;

        let args_rect = Rect {
            x: pad,
            y,
            w: content_w,
            h: (panel.h - y - pad).max(1),
        };
        fill_rect(canvas, width, height, args_rect, [255, 255, 255, 7]);

        let mut mono_style = value_style.clone();
        mono_style.font_family = "monospace".into();
        mono_style.font_size = 10.5;
        mono_style.foreground = "#D7DCE4".into();
        self.draw_wrapped_text_content(
            canvas,
            width,
            height,
            Rect {
                x: args_rect.x + 8,
                y: args_rect.y + 6,
                w: (args_rect.w - 16).max(1),
                h: (args_rect.h - 12).max(1),
            },
            &process.command_line,
            &mono_style,
        )?;

        Ok(())
    }

    pub fn draw_tooltip(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        text: &str,
        style: &ModuleStyle,
    ) -> Result<()> {
        canvas.fill(0);
        let background = style.background_rgba()?;
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

        let foreground = style.foreground_rgba()?;
        let color = Color::rgba(foreground[0], foreground[1], foreground[2], foreground[3]);
        let padding_x = style.padding_x.max(0);
        let padding_y = style.padding_y.max(0);
        let content_width = width
            .saturating_sub(padding_x.saturating_mul(2) as u32)
            .max(1);
        let content_height = height
            .saturating_sub(padding_y.saturating_mul(2) as u32)
            .max(1);
        let line_height = (style.font_size * 1.35).max(style.font_size);

        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.font_size, line_height));
        buffer.set_size(Some(content_width as f32), Some(content_height as f32));
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
                    padding_x.saturating_add(x),
                    padding_y.saturating_add(y),
                    w,
                    h,
                    pixel,
                );
            },
        );
        Ok(())
    }

    #[cfg(mhypr_module = "tray")]
    pub fn draw_tray_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        menu: &TrayPopupModel,
    ) -> Result<()> {
        canvas.fill(0);

        let background = menu.style.style.background_rgba()?;
        let foreground = menu.style.style.foreground_rgba()?;
        let hover = menu.style.hover_rgba()?;
        let border = menu.style.border_rgba()?;
        let separator = menu.style.separator_rgba()?;

        let root = menu.root_rect(width as f64, height as f64);
        draw_popup_panel(
            canvas,
            width,
            height,
            root,
            background,
            border,
            menu.style.border_width,
        );

        for (index, item) in menu.items.iter().enumerate() {
            let Some(rect) = menu.root_item_rect(index, width as f64, height as f64) else {
                continue;
            };
            if menu.hovered == Some(TrayPopupHit::Root(index)) {
                fill_rect(canvas, width, height, popup_rect(rect), hover);
            }
            if item.separator_before {
                draw_popup_separator(
                    canvas,
                    width,
                    height,
                    rect,
                    separator,
                    menu.style.separator_inset,
                );
            }

            let mut color = foreground;
            if !item.enabled {
                color[3] = color[3].min(110);
            }
            self.draw_popup_label(
                canvas,
                width,
                height,
                &item.label,
                rect,
                menu.style.padding_x,
                menu.style.style.font_size,
                Color::rgba(color[0], color[1], color[2], color[3]),
                &menu.style.style.font_family,
            );

            if !item.children.is_empty() {
                self.draw_popup_label(
                    canvas,
                    width,
                    height,
                    &menu.style.indicator,
                    TrayPopupRect {
                        x: rect.x + rect.w - 24.0,
                        y: rect.y,
                        w: 20.0,
                        h: rect.h,
                    },
                    0,
                    menu.style.style.font_size,
                    Color::rgba(foreground[0], foreground[1], foreground[2], 180),
                    &menu.style.style.font_family,
                );
            }
        }

        if let (Some(root_index), Some(submenu)) = (
            menu.open_root,
            menu.submenu_rect(width as f64, height as f64),
        ) {
            draw_popup_panel(
                canvas,
                width,
                height,
                submenu,
                background,
                border,
                menu.style.border_width,
            );

            for (child, item) in menu.items[root_index].children.iter().enumerate() {
                let Some(rect) = menu.child_item_rect(child, width as f64, height as f64) else {
                    continue;
                };
                if menu.hovered
                    == Some(TrayPopupHit::Child {
                        root: root_index,
                        child,
                    })
                {
                    fill_rect(canvas, width, height, popup_rect(rect), hover);
                }
                if item.separator_before {
                    draw_popup_separator(
                        canvas,
                        width,
                        height,
                        rect,
                        separator,
                        menu.style.separator_inset,
                    );
                }

                let mut color = foreground;
                if !item.enabled {
                    color[3] = color[3].min(110);
                }
                self.draw_popup_label(
                    canvas,
                    width,
                    height,
                    &item.label,
                    rect,
                    menu.style.padding_x,
                    menu.style.style.font_size,
                    Color::rgba(color[0], color[1], color[2], color[3]),
                    &menu.style.style.font_family,
                );
            }
        }

        Ok(())
    }

    #[cfg(mhypr_module = "tray")]
    #[allow(clippy::too_many_arguments)]
    fn draw_popup_label(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        text: &str,
        rect: TrayPopupRect,
        padding_x: i32,
        font_size: f32,
        color: Color,
        font_family: &str,
    ) {
        let text_x = rect.x.round() as i32 + padding_x.max(0);
        let text_y = rect.y.round() as i32;
        let text_w = (rect.w - (padding_x.max(0) * 2) as f64).max(1.0) as f32;

        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, rect.h as f32));
        buffer.set_size(Some(text_w), Some(rect.h as f32));
        let family = match font_family {
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
                blend_block(canvas, width, height, text_x + x, text_y + y, w, h, pixel);
            },
        );
    }
}
