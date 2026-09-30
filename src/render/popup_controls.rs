use super::*;
#[allow(unused_imports)]
use super::{layout::estimate_text_width, primitives::*};

impl Renderer {
    #[cfg(mhypr_module = "brightness")]
    pub fn draw_brightness_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::brightness::popup::BrightnessPopupModel,
        panel_x: f64,
        panel_y: f64,
    ) -> Result<()> {
        canvas.fill(0);
        let cfg = &model.config;
        let panel = Rect {
            x: panel_x.round() as i32,
            y: panel_y.round() as i32,
            w: cfg.width,
            h: model.panel_height(),
        };
        let background = cfg.style.background_rgba()?;
        let border = cfg.border_rgba()?;
        let separator = cfg.separator_rgba()?;
        let active_background = cfg.active_background_rgba()?;
        let bar_background = cfg.bar_background_rgba()?;
        let bar_fill = cfg.bar_fill_rgba()?;

        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(1);
        let mut y = panel.y + pad;

        let mut title_style = cfg.style.clone();
        title_style.font_size = (cfg.style.font_size + 1.0).max(cfg.style.font_size);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: cfg.title_height,
            },
            &format!("Brightness devices · {}", model.devices.len()),
            &title_style,
            0,
            0,
        )?;
        y += cfg.title_height;
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: 1,
            },
            separator,
        );
        y += 1;

        if model.devices.is_empty() {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: content_x,
                    y,
                    w: content_w,
                    h: cfg.row_height,
                },
                "No brightness devices",
                &cfg.style,
                8,
                0,
            )?;
            return Ok(());
        }

        let mut detail_style = cfg.style.clone();
        detail_style.font_size = (cfg.style.font_size - 1.0).max(9.0);
        let percent_w = 58;
        for (index, device) in model.devices.iter().enumerate() {
            let Some(g) = model.row_geometry(index) else {
                continue;
            };
            let row = Rect {
                x: content_x,
                y: panel.y + g.row_y,
                w: content_w,
                h: g.row_h,
            };
            if device.active {
                fill_rect(canvas, width, height, row, active_background);
            }

            let top_h = (cfg.row_height * 3 / 5).max(1);
            let left_x = row.x + 8;
            let right_x = row.x + row.w - percent_w - 8;
            let label = if device.active {
                format!("{} · active", device.name)
            } else {
                device.name.clone()
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: left_x,
                    y: row.y,
                    w: (right_x - left_x - 8).max(1),
                    h: top_h,
                },
                &label,
                &cfg.style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: right_x,
                    y: row.y,
                    w: percent_w,
                    h: top_h,
                },
                &format!("{}%", device.percent),
                &cfg.style,
                0,
                0,
            )?;

            let kind = if device.kind.trim().is_empty() {
                "unknown"
            } else {
                device.kind.as_str()
            };
            let detail = format!("{kind} · {}/{}", device.current, device.max);
            let detail_y = row.y + top_h;
            let detail_h = ((panel.y + g.slider_y) - detail_y - 3).max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: left_x,
                    y: detail_y,
                    w: (row.w - 16).max(1),
                    h: detail_h,
                },
                &detail,
                &detail_style,
                0,
                0,
            )?;

            let bar = Rect {
                x: panel.x + g.slider_x,
                y: panel.y + g.slider_y,
                w: g.slider_w,
                h: g.slider_h,
            };
            fill_rect(canvas, width, height, bar, bar_background);
            let range = 100_u32.saturating_sub(model.min_percent).max(1);
            let normalized = device
                .percent
                .clamp(model.min_percent, 100)
                .saturating_sub(model.min_percent);
            let fill_w =
                ((bar.w as f32 * normalized as f32 / range as f32).round() as i32).clamp(0, bar.w);
            if fill_w > 0 {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: bar.x,
                        y: bar.y,
                        w: fill_w,
                        h: bar.h,
                    },
                    bar_fill,
                );
            }
            let knob_x = bar
                .x
                .saturating_add(fill_w)
                .saturating_sub(1)
                .clamp(bar.x, bar.x + bar.w - 2);
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: knob_x,
                    y: bar.y.saturating_sub(4),
                    w: 2,
                    h: bar.h.saturating_add(8),
                },
                bar_fill,
            );

            y += cfg.row_height;
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: content_x,
                    y: y - 1,
                    w: content_w,
                    h: 1,
                },
                separator,
            );
        }

        Ok(())
    }

    #[cfg(mhypr_module = "layout")]
    pub fn draw_layout_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::layout::popup::LayoutPopupModel,
        panel_x: f64,
        panel_y: f64,
    ) -> Result<()> {
        canvas.fill(0);
        let cfg = &model.config;
        let panel = Rect {
            x: panel_x.round() as i32,
            y: panel_y.round() as i32,
            w: cfg.width,
            h: model.panel_height(),
        };
        let background = cfg.style.background_rgba()?;
        let border = cfg.border_rgba()?;
        let separator = cfg.separator_rgba()?;
        let active_background = cfg.active_background_rgba()?;
        let hover_background = cfg.hover_background_rgba()?;
        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(1);
        let mut y = panel.y + pad;

        let mut title_style = cfg.style.clone();
        title_style.font_size = (cfg.style.font_size + 1.0).max(cfg.style.font_size);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: cfg.title_height,
            },
            "Layout",
            &title_style,
            8,
            0,
        )?;
        y += cfg.title_height;
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: 1,
            },
            separator,
        );
        y += 1;

        for (index, row) in model.rows.iter().enumerate() {
            let rect = Rect {
                x: content_x,
                y,
                w: content_w,
                h: cfg.row_height,
            };
            if row.active {
                fill_rect(canvas, width, height, rect, active_background);
            } else if model.hovered_row == Some(index) {
                fill_rect(canvas, width, height, rect, hover_background);
            }

            let label = if row.active {
                format!("{}  ·  current", row.label)
            } else {
                row.label.clone()
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: rect.x + 10,
                    y: rect.y,
                    w: (rect.w - 20).max(1),
                    h: rect.h,
                },
                &label,
                &cfg.style,
                0,
                0,
            )?;

            y += cfg.row_height;
            if index + 1 < model.rows.len() {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: content_x,
                        y: y - 1,
                        w: content_w,
                        h: 1,
                    },
                    separator,
                );
            }
        }

        Ok(())
    }

    #[cfg(mhypr_module = "audio")]
    pub fn draw_audio_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::audio::popup::AudioPopupModel,
        panel_x: f64,
        panel_y: f64,
    ) -> Result<()> {
        canvas.fill(0);
        let cfg = &model.config;
        let panel = Rect {
            x: panel_x.round() as i32,
            y: panel_y.round() as i32,
            w: cfg.width,
            h: model.panel_height(),
        };
        let background = cfg.style.background_rgba()?;
        let border = cfg.border_rgba()?;
        let separator = cfg.separator_rgba()?;
        let active_background = cfg.active_background_rgba()?;
        let muted = cfg.muted_rgba()?;
        let bar_background = cfg.bar_background_rgba()?;
        let bar_fill = cfg.bar_fill_rgba()?;

        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(1);
        let mut y = panel.y + pad;

        let mut title_style = cfg.style.clone();
        title_style.font_size = (cfg.style.font_size + 1.0).max(cfg.style.font_size);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: cfg.title_height,
            },
            &format!("Audio outputs · {}", model.outputs.len()),
            &title_style,
            0,
            0,
        )?;
        y += cfg.title_height;
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: 1,
            },
            separator,
        );
        y += 1;

        if model.outputs.is_empty() {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: content_x,
                    y,
                    w: content_w,
                    h: cfg.row_height,
                },
                "No audio outputs",
                &cfg.style,
                8,
                0,
            )?;
            return Ok(());
        }

        let mut detail_style = cfg.style.clone();
        detail_style.font_size = (cfg.style.font_size - 1.0).max(9.0);
        for (index, output) in model.outputs.iter().enumerate() {
            let Some(g) = model.row_geometry(index) else {
                continue;
            };
            let row = Rect {
                x: panel.x + cfg.padding,
                y: panel.y + g.row_y,
                w: content_w,
                h: g.row_h,
            };
            if output.is_default {
                fill_rect(canvas, width, height, row, active_background);
            }

            let mute_rect = Rect {
                x: panel.x + g.mute_x,
                y: panel.y + g.mute_y,
                w: g.mute_w,
                h: g.mute_h,
            };
            draw_rect_border(
                canvas,
                width,
                height,
                mute_rect,
                if output.muted { muted } else { separator },
                1,
            );
            let mut mute_style = cfg.style.clone();
            if output.muted {
                mute_style.foreground = cfg.muted.clone();
            }
            self.draw_text_content(
                canvas,
                width,
                height,
                mute_rect,
                if output.muted { "MUTED" } else { "MUTE" },
                &mute_style,
                6,
                0,
            )?;

            let label = if output.is_default {
                format!("{} · default", output.name)
            } else {
                output.name.clone()
            };
            let top_h = (g.row_h / 2).max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: panel.x + g.text_x,
                    y: row.y,
                    w: g.text_w,
                    h: top_h,
                },
                &label,
                &cfg.style,
                0,
                0,
            )?;

            let detail = if output.detail.trim().is_empty() {
                output.id.as_str()
            } else {
                output.detail.as_str()
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: panel.x + g.text_x,
                    y: row.y + top_h,
                    w: g.text_w,
                    h: (g.row_h - top_h).max(1),
                },
                detail,
                &detail_style,
                0,
                0,
            )?;

            let slider = Rect {
                x: panel.x + g.slider_x,
                y: panel.y + g.slider_y,
                w: g.slider_w,
                h: g.slider_h,
            };
            fill_rect(canvas, width, height, slider, bar_background);
            let fill_w = ((slider.w as f32
                * output.percent.min(model.max_percent) as f32
                / model.max_percent.max(1) as f32)
                .round() as i32)
                .clamp(0, slider.w);
            if fill_w > 0 {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: slider.x,
                        y: slider.y,
                        w: fill_w,
                        h: slider.h,
                    },
                    if output.muted { muted } else { bar_fill },
                );
            }
            let knob_x = slider
                .x
                .saturating_add(fill_w)
                .saturating_sub(1)
                .clamp(slider.x, slider.x + slider.w - 2);
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: knob_x,
                    y: slider.y.saturating_sub(4),
                    w: 2,
                    h: slider.h.saturating_add(8),
                },
                if output.muted { muted } else { bar_fill },
            );

            let mut percent_style = cfg.style.clone();
            if output.muted {
                percent_style.foreground = cfg.muted.clone();
            }
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: panel.x + g.percent_x,
                    y: row.y,
                    w: g.percent_w,
                    h: row.h,
                },
                &format!("{}%", output.percent),
                &percent_style,
                0,
                0,
            )?;

            y += cfg.row_height;
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: content_x,
                    y: y - 1,
                    w: content_w,
                    h: 1,
                },
                separator,
            );
        }

        Ok(())
    }

    #[cfg(mhypr_module = "active_window")]
    pub fn draw_active_window_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::active_window::popup::ActiveWindowPopupModel,
        panel_x: f64,
        panel_y: f64,
    ) -> Result<()> {
        canvas.fill(0);
        let cfg = &model.config;
        let panel = Rect {
            x: panel_x.round() as i32,
            y: panel_y.round() as i32,
            w: cfg.width,
            h: model.panel_height(),
        };
        let background = cfg.style.background_rgba()?;
        let border = cfg.border_rgba()?;
        let separator = cfg.separator_rgba()?;
        let active_background = cfg.active_background_rgba()?;
        let hover_background = cfg.hover_background_rgba()?;
        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(1);
        let mut y = panel.y + pad;

        let mut title_style = cfg.style.clone();
        title_style.font_size = (cfg.style.font_size + 1.0).max(cfg.style.font_size);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: cfg.title_height,
            },
            &model.title,
            &title_style,
            0,
            0,
        )?;
        y += cfg.title_height;
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w,
                h: 1,
            },
            separator,
        );
        y += 1;

        if model.rows.is_empty() {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: content_x,
                    y,
                    w: content_w,
                    h: cfg.row_height,
                },
                "No windows",
                &cfg.style,
                8,
                0,
            )?;
            return Ok(());
        }

        let mut detail_style = cfg.style.clone();
        detail_style.font_size = (cfg.style.font_size - 1.0).max(9.0);
        for (index, window) in model.rows.iter().enumerate() {
            let row = Rect {
                x: content_x,
                y,
                w: content_w,
                h: cfg.row_height,
            };
            if model.hovered_row == Some(index) {
                fill_rect(canvas, width, height, row, hover_background);
            } else if window.active {
                fill_rect(canvas, width, height, row, active_background);
            }

            let icon_x = row.x + 8;
            let icon_y = row.y + (row.h - cfg.icon_size) / 2;
            let mut text_x = icon_x;
            if let Some(icon) = window.icon.as_ref() {
                draw_native_argb_pixmap(
                    canvas,
                    width,
                    height,
                    icon_x,
                    icon_y,
                    cfg.icon_size,
                    icon.width,
                    icon.height,
                    &icon.pixels,
                );
                text_x += cfg.icon_size + 10;
            }

            let text_w = (row.x + row.w - 8 - text_x).max(1);
            let top_h = (cfg.row_height / 2).max(1);
            let title = if window.title.trim().is_empty() {
                window.class.as_str()
            } else {
                window.title.as_str()
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: text_x,
                    y: row.y,
                    w: text_w,
                    h: top_h,
                },
                title,
                &cfg.style,
                0,
                0,
            )?;

            let detail = if window.active {
                format!("{} · workspace {} · active", window.class, window.workspace_id)
            } else {
                format!("{} · workspace {}", window.class, window.workspace_id)
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: text_x,
                    y: row.y + top_h,
                    w: text_w,
                    h: row.h - top_h,
                },
                &detail,
                &detail_style,
                0,
                0,
            )?;

            y += cfg.row_height;
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: content_x,
                    y: y - 1,
                    w: content_w,
                    h: 1,
                },
                separator,
            );
        }
        Ok(())
    }

    #[cfg(mhypr_module = "network")]
    pub fn draw_network_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::network::popup::NetworkPopupModel,
        panel_x: f64,
        panel_y: f64,
    ) -> Result<()> {
        canvas.fill(0);
        let cfg = &model.config;
        let panel = Rect {
            x: panel_x.round() as i32,
            y: panel_y.round() as i32,
            w: cfg.width,
            h: model.panel_height(),
        };
        let background = cfg.style.background_rgba()?;
        let border = cfg.border_rgba()?;
        let separator = cfg.separator_rgba()?;
        let active_background = cfg.active_background_rgba()?;
        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(1);
        let mut y = panel.y + pad;

        let mut title_style = cfg.style.clone();
        title_style.font_size = (cfg.style.font_size + 1.0).max(cfg.style.font_size);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect { x: content_x, y, w: content_w, h: cfg.title_height },
            &format!("Network interfaces · {}", model.interfaces.len()),
            &title_style,
            0,
            0,
        )?;
        y += cfg.title_height;
        fill_rect(
            canvas,
            width,
            height,
            Rect { x: content_x, y, w: content_w, h: 1 },
            separator,
        );
        y += 1;

        if model.interfaces.is_empty() {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect { x: content_x, y, w: content_w, h: cfg.row_height },
                "No network interfaces",
                &cfg.style,
                0,
                0,
            )?;
            return Ok(());
        }

        let mut detail_style = cfg.style.clone();
        detail_style.font_size = (cfg.style.font_size - 1.0).max(9.0);
        for interface in &model.interfaces {
            let row = Rect { x: content_x, y, w: content_w, h: cfg.row_height };
            if interface.is_default {
                fill_rect(canvas, width, height, row, active_background);
            }
            let top_h = (cfg.row_height / 2).max(1);
            let title = if interface.is_default {
                format!("{} · default · {}", interface.name, interface.state)
            } else {
                format!("{} · {}", interface.name, interface.state)
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect { x: row.x + 8, y: row.y, w: row.w - 16, h: top_h },
                &title,
                &cfg.style,
                0,
                0,
            )?;
            let detail = format!(
                "{}   speed {}   mtu {}   mac {}",
                interface.addresses, interface.speed, interface.mtu, interface.mac
            );
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect { x: row.x + 8, y: row.y + top_h, w: row.w - 16, h: row.h - top_h },
                &detail,
                &detail_style,
                0,
                0,
            )?;
            y += cfg.row_height;
            fill_rect(
                canvas,
                width,
                height,
                Rect { x: content_x, y: y - 1, w: content_w, h: 1 },
                separator,
            );
        }
        Ok(())
    }

}
