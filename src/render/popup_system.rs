use super::*;
#[allow(unused_imports)]
use super::{layout::estimate_text_width, primitives::*};

impl Renderer {
    #[cfg(mhypr_module = "battery")]
    pub fn draw_battery_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::battery::popup::BatteryPopupModel,
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
        let bar_background = cfg.bar_background_rgba()?;
        let fill = model.fill_color();
        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(1);
        let mut title_style = cfg.style.clone();
        title_style.font_size = 16.0;
        let mut muted_style = cfg.style.clone();
        muted_style.foreground = "#9AA0AA".into();
        muted_style.font_size = 11.0;

        let mut y = panel.y + pad;
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y,
                w: content_w - 80,
                h: cfg.title_height,
            },
            "Battery",
            &title_style,
            0,
            0,
        )?;
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x + content_w - 64,
                y,
                w: 64,
                h: cfg.title_height,
            },
            &format!("{:.0}%", model.stats.capacity),
            &title_style,
            0,
            0,
        )?;
        y = y.saturating_add(cfg.title_height);

        let progress = Rect {
            x: content_x,
            y,
            w: content_w,
            h: cfg.progress_height,
        };
        fill_rect(canvas, width, height, progress, bar_background);
        let fill_w = ((progress.w as f32 * model.stats.capacity.clamp(0.0, 100.0) / 100.0).round()
            as i32)
            .clamp(0, progress.w);
        if fill_w > 0 {
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: progress.x,
                    y: progress.y,
                    w: fill_w,
                    h: progress.h,
                },
                fill,
            );
        }
        y = y.saturating_add(cfg.progress_height).saturating_add(8);

        let status_icon_w = 26;
        if model.stats.charging() {
            draw_battery_bolt(
                canvas,
                width,
                height,
                Rect {
                    x: content_x + 2,
                    y: y + 10,
                    w: 14,
                    h: 24,
                },
                fill,
            );
        }
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x + status_icon_w,
                y,
                w: content_w - status_icon_w,
                h: 26,
            },
            &model.stats.status,
            &cfg.style,
            0,
            0,
        )?;
        let detail = model.status_detail_text();
        if !detail.is_empty() {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: content_x + status_icon_w,
                    y: y + 24,
                    w: content_w - status_icon_w,
                    h: 24,
                },
                &detail,
                &muted_style,
                0,
                0,
            )?;
        }
        y = y.saturating_add(cfg.status_height);

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
        y = y.saturating_add(1);

        let rows = [
            ("Capacity", model.capacity_text()),
            ("Time remaining", model.time_remaining_text()),
            ("Battery health", model.health_text()),
            ("Design capacity", model.design_capacity_text()),
            ("Current rate", model.current_rate_text()),
        ];
        let value_w = 112;
        for (label, value) in rows {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: content_x,
                    y,
                    w: content_w - value_w - 8,
                    h: cfg.row_height,
                },
                label,
                &muted_style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: content_x + content_w - value_w,
                    y,
                    w: value_w,
                    h: cfg.row_height,
                },
                &value,
                &cfg.style,
                0,
                0,
            )?;
            y = y.saturating_add(cfg.row_height);
        }

        Ok(())
    }

    #[cfg(mhypr_module = "clock")]
    pub fn draw_clock_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::clock::popup::ClockPopupModel,
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
        let today_background = cfg.today_background_rgba()?;
        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(7);
        let mut title_style = cfg.style.clone();
        title_style.font_size = 15.5;
        let mut summary_style = cfg.style.clone();
        summary_style.font_size = 10.0;
        summary_style.foreground = cfg.weekday_foreground.clone();
        let mut weekday_style = cfg.style.clone();
        weekday_style.font_size = 9.5;
        weekday_style.foreground = cfg.weekday_foreground.clone();
        let mut adjacent_style = cfg.style.clone();
        adjacent_style.foreground = cfg.adjacent_foreground.clone();
        let mut today_style = cfg.style.clone();
        today_style.foreground = cfg.today_foreground.clone();

        let mut y = panel.y + pad;
        let nav_w = 30;
        let nav_h = 26;
        let previous_rect = Rect {
            x: content_x,
            y,
            w: nav_w,
            h: nav_h,
        };
        let next_rect = Rect {
            x: content_x + content_w - nav_w,
            y,
            w: nav_w,
            h: nav_h,
        };
        draw_rect_border(canvas, width, height, previous_rect, separator, 1);
        draw_rect_border(canvas, width, height, next_rect, separator, 1);

        for (button, label) in [(previous_rect, "<"), (next_rect, ">")] {
            let label_w = estimate_text_width(label, &title_style)
                .min(button.w)
                .max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: button.x + (button.w - label_w).max(0) / 2,
                    y: button.y,
                    w: label_w,
                    h: button.h,
                },
                label,
                &title_style,
                0,
                0,
            )?;
        }

        let title_rect = Rect {
            x: content_x + nav_w,
            y,
            w: (content_w - nav_w * 2).max(1),
            h: nav_h,
        };
        if model.mode == crate::modules::clock::popup::ClockPopupMode::Months {
            let year_text = model.view_year.to_string();
            let year_w = estimate_text_width(&year_text, &title_style)
                .min(title_rect.w)
                .max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: title_rect.x + (title_rect.w - year_w).max(0) / 2,
                    y: title_rect.y,
                    w: year_w,
                    h: title_rect.h,
                },
                &year_text,
                &title_style,
                0,
                0,
            )?;
        } else {
            let year_w = 68.min(title_rect.w).max(1);
            let month_rect = Rect {
                x: title_rect.x,
                y: title_rect.y,
                w: title_rect.w.saturating_sub(year_w),
                h: title_rect.h,
            };
            let year_rect = Rect {
                x: title_rect.x + title_rect.w - year_w,
                y: title_rect.y,
                w: year_w,
                h: title_rect.h,
            };
            let month_text = crate::modules::clock::month_name(model.view_month);
            let month_w = estimate_text_width(month_text, &title_style)
                .min(month_rect.w)
                .max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: month_rect.x + (month_rect.w - month_w).max(0) / 2,
                    y: month_rect.y,
                    w: month_w,
                    h: month_rect.h,
                },
                month_text,
                &title_style,
                0,
                0,
            )?;
            draw_rect_border(canvas, width, height, year_rect, separator, 1);
            let year_text = model.view_year.to_string();
            let year_text_w = estimate_text_width(&year_text, &title_style)
                .min(year_rect.w)
                .max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: year_rect.x + (year_rect.w - year_text_w).max(0) / 2,
                    y: year_rect.y,
                    w: year_text_w,
                    h: year_rect.h,
                },
                &year_text,
                &title_style,
                0,
                0,
            )?;
        }

        let summary_y = y + nav_h;
        let summary_h = (cfg.header_height - nav_h).max(1);
        let time_text = model.time_text();
        let time_w = estimate_text_width(&time_text, &summary_style)
            .min(content_w)
            .max(1);
        let summary_w = (content_w - time_w - 8).max(1);
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x,
                y: summary_y,
                w: summary_w,
                h: summary_h,
            },
            &model.date_summary(),
            &summary_style,
            0,
            0,
        )?;
        self.draw_text_content(
            canvas,
            width,
            height,
            Rect {
                x: content_x + content_w - time_w,
                y: summary_y,
                w: time_w,
                h: summary_h,
            },
            &time_text,
            &summary_style,
            0,
            0,
        )?;
        y = y.saturating_add(cfg.header_height);

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
        y = y.saturating_add(1);

        if model.mode == crate::modules::clock::popup::ClockPopupMode::Months {
            let body_height = cfg
                .weekday_height
                .saturating_add(cfg.cell_height.saturating_mul(6));
            let column_w = (content_w / 3).max(1);
            let row_h = (body_height / 4).max(1);
            let months = [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ];

            for (index, label) in months.iter().enumerate() {
                let row = index / 3;
                let column = index % 3;
                let cell = Rect {
                    x: content_x + column as i32 * column_w,
                    y: y + row as i32 * row_h,
                    w: column_w,
                    h: row_h,
                };
                let selected = model.view_month == index as u32 + 1;
                if selected {
                    let inset = 4;
                    fill_rect(
                        canvas,
                        width,
                        height,
                        Rect {
                            x: cell.x + inset,
                            y: cell.y + inset,
                            w: (cell.w - inset * 2).max(1),
                            h: (cell.h - inset * 2).max(1),
                        },
                        today_background,
                    );
                }
                let style = if selected { &today_style } else { &cfg.style };
                let text_w = estimate_text_width(label, style).min(cell.w).max(1);
                self.draw_text_content(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: cell.x + (cell.w - text_w).max(0) / 2,
                        y: cell.y,
                        w: text_w,
                        h: cell.h,
                    },
                    label,
                    style,
                    0,
                    0,
                )?;
            }
            return Ok(());
        }

        let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let column_w = (content_w / 7).max(1);
        for (column, label) in weekdays.iter().enumerate() {
            let x = content_x + column as i32 * column_w;
            let text_w = estimate_text_width(label, &weekday_style)
                .min(column_w)
                .max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: x + (column_w - text_w).max(0) / 2,
                    y,
                    w: text_w,
                    h: cfg.weekday_height,
                },
                label,
                &weekday_style,
                0,
                0,
            )?;
        }
        y = y.saturating_add(cfg.weekday_height);

        for (index, cell) in model.cells.iter().enumerate() {
            let row = index / 7;
            let column = index % 7;
            let cell_x = content_x + column as i32 * column_w;
            let cell_y = y + row as i32 * cfg.cell_height;
            let mark_size = column_w.min(cfg.cell_height).clamp(1, 28);

            if cell.today {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: cell_x + (column_w - mark_size).max(0) / 2,
                        y: cell_y + (cfg.cell_height - mark_size).max(0) / 2,
                        w: mark_size,
                        h: mark_size,
                    },
                    today_background,
                );
            }

            let style = if cell.today {
                &today_style
            } else if cell.in_month {
                &cfg.style
            } else {
                &adjacent_style
            };
            let text = cell.day.to_string();
            let text_w = estimate_text_width(&text, style).min(column_w).max(1);
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: cell_x + (column_w - text_w).max(0) / 2,
                    y: cell_y,
                    w: text_w,
                    h: cfg.cell_height,
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
    pub fn draw_monitor_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::monitor::popup::MonitorPopupModel,
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
        let selected_background = cfg.selected_background_rgba()?;
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
            &format!("Displays · {}", model.monitors.len()),
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

        let mut detail_style = cfg.style.clone();
        detail_style.font_size = (cfg.style.font_size - 1.0).max(9.0);

        if model.monitors.is_empty() {
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
                "No active monitors",
                &cfg.style,
                0,
                0,
            )?;
            y += cfg.row_height;
        } else {
            for (index, monitor) in model.monitors.iter().enumerate() {
                let row = Rect {
                    x: content_x,
                    y,
                    w: content_w,
                    h: cfg.row_height,
                };
                if index == model.selected {
                    fill_rect(canvas, width, height, row, selected_background);
                } else if model.hovered_row == Some(index) {
                    fill_rect(canvas, width, height, row, hover_background);
                }

                let primary = crate::modules::monitor::popup::MonitorPopupModel::primary_text(monitor);
                let detail = crate::modules::monitor::popup::MonitorPopupModel::detail_text(monitor);
                let top_h = (cfg.row_height / 2).max(1);
                self.draw_text_content(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: row.x + 6,
                        y: row.y,
                        w: (row.w - 12).max(1),
                        h: top_h,
                    },
                    &primary,
                    &cfg.style,
                    0,
                    0,
                )?;
                self.draw_text_content(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: row.x + 6,
                        y: row.y + top_h,
                        w: (row.w - 12).max(1),
                        h: (row.h - top_h).max(1),
                    },
                    &detail,
                    &detail_style,
                    0,
                    0,
                )?;
                y += cfg.row_height;
            }
        }

        let _ = y;

        Ok(())
    }

    #[cfg(mhypr_module = "monitor")]
    pub fn draw_monitor_context(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::monitor::popup::MonitorPopupModel,
        panel_x: f64,
        panel_y: f64,
        panel_width: i32,
        row_height: i32,
        hovered_action: Option<usize>,
    ) -> Result<()> {
        canvas.fill(0);
        let cfg = &model.config;
        let panel = Rect {
            x: panel_x.round() as i32,
            y: panel_y.round() as i32,
            w: panel_width,
            h: row_height.saturating_mul(9),
        };
        let background = cfg.style.background_rgba()?;
        let border = cfg.border_rgba()?;
        let separator = cfg.separator_rgba()?;
        let hover_background = cfg.hover_background_rgba()?;
        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let actions = [
            ("Focus", "Focus this display"),
            ("Scale -", "Decrease scale by 0.25"),
            ("Scale +", "Increase scale by 0.25"),
            ("Left", "Place automatically to the left"),
            ("Up", "Place automatically above"),
            ("Down", "Place automatically below"),
            ("Right", "Place automatically to the right"),
            ("Mode -", "Switch to previous display mode"),
            ("Mode +", "Switch to next display mode"),
        ];
        let mut detail_style = cfg.style.clone();
        detail_style.font_size = (cfg.style.font_size - 1.0).max(9.0);
        for (index, (label, description)) in actions.iter().enumerate() {
            let row = Rect {
                x: panel.x,
                y: panel.y + index as i32 * row_height,
                w: panel.w,
                h: row_height,
            };
            if hovered_action == Some(index) {
                fill_rect(canvas, width, height, row, hover_background);
            }
            if index > 0 {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: row.x,
                        y: row.y,
                        w: row.w,
                        h: 1,
                    },
                    separator,
                );
            }
            let label_w = 88;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: row.x + 10,
                    y: row.y,
                    w: label_w,
                    h: row.h,
                },
                label,
                &cfg.style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: row.x + label_w,
                    y: row.y,
                    w: (row.w - label_w - 10).max(1),
                    h: row.h,
                },
                description,
                &detail_style,
                0,
                0,
            )?;
        }
        Ok(())
    }

}
