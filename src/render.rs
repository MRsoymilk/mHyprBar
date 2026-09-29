use anyhow::Result;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

#[cfg(mhypr_module = "tray")]
use crate::tray_popup::{TrayPopupHit, TrayPopupModel, TrayPopupRect};
use crate::{
    config::{BarConfig, ModuleStyle, WorkspacesConfig},
    hyprland::{MonitorState, Snapshot},
    modules::{ModuleManager, ModuleView, ModuleVisual},
    tray::TrayState,
};

pub struct Renderer {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

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

        let workspace_width =
            self.draw_workspaces(canvas, width, height, &config.workspaces, monitor, snapshot)?;
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

            let x = (local as i32 - 1).saturating_mul(style.width.saturating_add(style.gap));
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
        }

        Ok(style.strip_width())
    }

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

    #[cfg(mhypr_module = "battery")]
    pub fn draw_battery_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::battery_popup::BatteryPopupModel,
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
        model: &crate::clock_popup::ClockPopupModel,
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
        if model.mode == crate::clock_popup::ClockPopupMode::Months {
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

        if model.mode == crate::clock_popup::ClockPopupMode::Months {
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

    #[cfg(mhypr_module = "cpu")]
    pub fn draw_cpu_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::cpu_popup::CpuPopupModel,
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
        let bar_bg = cfg.bar_background_rgba()?;
        let bar_fill = cfg.bar_fill_rgba()?;
        let hover = cfg.hover_rgba()?;

        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let row_h = cfg.row_height;
        let pad = cfg.padding;
        let mut y = panel.y.saturating_add(pad);

        self.draw_cpu_popup_text(
            canvas,
            width,
            height,
            Rect {
                x: panel.x + pad,
                y,
                w: panel.w - pad * 2,
                h: row_h,
            },
            "CPU cores",
            &cfg.style,
        )?;
        y = y.saturating_add(row_h);

        for core in &model.cores {
            let usage = core.usage.unwrap_or(0.0).clamp(0.0, 100.0);
            let label_rect = Rect {
                x: panel.x + pad,
                y,
                w: 52,
                h: row_h,
            };
            self.draw_cpu_popup_text(canvas, width, height, label_rect, &core.name, &cfg.style)?;

            let percent_rect = Rect {
                x: panel.x + pad + 54,
                y,
                w: 44,
                h: row_h,
            };
            let percent = core
                .usage
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| "--%".into());
            self.draw_cpu_popup_text(canvas, width, height, percent_rect, &percent, &cfg.style)?;

            let bar_w = cfg
                .bar_width
                .min(panel.w.saturating_sub(pad * 2 + 104))
                .max(1);
            let bar_h = (row_h - 8).max(4);
            let bar_rect = Rect {
                x: panel.x + panel.w - pad - bar_w,
                y: y + (row_h - bar_h) / 2,
                w: bar_w,
                h: bar_h,
            };
            fill_rect(canvas, width, height, bar_rect, bar_bg);
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: bar_rect.x,
                    y: bar_rect.y,
                    w: ((bar_rect.w as f32 * usage / 100.0).round() as i32).clamp(0, bar_rect.w),
                    h: bar_rect.h,
                },
                bar_fill,
            );
            y = y.saturating_add(row_h);
        }

        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: panel.x + pad,
                y,
                w: (panel.w - pad * 2).max(0),
                h: 1,
            },
            separator,
        );
        y = y.saturating_add(1);

        let header = Rect {
            x: panel.x + pad,
            y,
            w: panel.w - pad * 2,
            h: row_h,
        };
        self.draw_cpu_popup_text(
            canvas,
            width,
            height,
            Rect {
                x: header.x,
                y,
                w: 48,
                h: row_h,
            },
            "PID",
            &cfg.style,
        )?;
        self.draw_cpu_popup_text(
            canvas,
            width,
            height,
            Rect {
                x: header.x + 52,
                y,
                w: header.w - 108,
                h: row_h,
            },
            "Name",
            &cfg.style,
        )?;
        self.draw_cpu_popup_text(
            canvas,
            width,
            height,
            Rect {
                x: header.x + header.w - 52,
                y,
                w: 52,
                h: row_h,
            },
            "%CPU",
            &cfg.style,
        )?;
        y = y.saturating_add(row_h);

        for (index, process) in model.processes.iter().enumerate() {
            let row = Rect {
                x: panel.x + pad,
                y,
                w: panel.w - pad * 2,
                h: row_h,
            };
            if model.hovered_process == Some(index) {
                fill_rect(canvas, width, height, row, hover);
            }
            self.draw_cpu_popup_text(
                canvas,
                width,
                height,
                Rect {
                    x: row.x,
                    y,
                    w: 48,
                    h: row_h,
                },
                &process.pid.to_string(),
                &cfg.style,
            )?;
            self.draw_cpu_popup_text(
                canvas,
                width,
                height,
                Rect {
                    x: row.x + 52,
                    y,
                    w: row.w - 108,
                    h: row_h,
                },
                &process.name,
                &cfg.style,
            )?;
            self.draw_cpu_popup_text(
                canvas,
                width,
                height,
                Rect {
                    x: row.x + row.w - 52,
                    y,
                    w: 52,
                    h: row_h,
                },
                &format!("{:.1}", process.cpu),
                &cfg.style,
            )?;
            y = y.saturating_add(row_h);
        }

        Ok(())
    }

    #[cfg(mhypr_module = "cpu")]
    fn draw_cpu_popup_text(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        rect: Rect,
        text: &str,
        style: &ModuleStyle,
    ) -> Result<()> {
        self.draw_text_content(canvas, width, height, rect, text, style, 0, 0)
    }

    #[cfg(mhypr_module = "disk")]
    pub fn draw_disk_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::disk_popup::DiskPopupModel,
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
        let hover = cfg.hover_rgba()?;
        let bar_bg = cfg.bar_background_rgba()?;
        let bar_fill = cfg.bar_fill_rgba()?;
        let bar_border = cfg.bar_border_rgba()?;

        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let row_h = cfg.row_height;
        let pad = cfg.padding;
        let mount_w = 92;
        let gap = 8;
        let bar_w = cfg.bar_width.min((panel.w - pad * 2 - 230).max(1));
        let used_w = 104;
        let pct_w = 48;
        let mount_x = panel.x + pad;
        let bar_x = mount_x + mount_w + gap;
        let used_x = bar_x + bar_w + gap;
        let pct_x = used_x + used_w + gap;
        let mut y = panel.y + pad;

        for (x, w, label) in [
            (mount_x, mount_w, "Path"),
            (bar_x, bar_w, "Usage"),
            (used_x, used_w, "Used / Total"),
            (pct_x, pct_w, "Used"),
        ] {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect { x, y, w, h: row_h },
                label,
                &cfg.style,
                0,
                0,
            )?;
        }
        y = y.saturating_add(row_h);

        for (index, stats) in model.rows.iter().enumerate() {
            let row = Rect {
                x: panel.x + pad,
                y,
                w: panel.w - pad * 2,
                h: row_h,
            };
            if model.hovered_row == Some(index) {
                fill_rect(canvas, width, height, row, hover);
            }

            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: mount_x,
                    y,
                    w: mount_w,
                    h: row_h,
                },
                &stats.mount,
                &cfg.style,
                0,
                0,
            )?;

            let bar_h = (row_h - 12).max(6);
            let bar = Rect {
                x: bar_x,
                y: y + (row_h - bar_h) / 2,
                w: bar_w,
                h: bar_h,
            };
            fill_rect(canvas, width, height, bar, bar_bg);
            let inner = Rect {
                x: bar.x + 1,
                y: bar.y + 1,
                w: (bar.w - 2).max(0),
                h: (bar.h - 2).max(0),
            };
            let fill_w = ((inner.w as f32 * stats.percent().clamp(0.0, 100.0) / 100.0).round()
                as i32)
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
                    bar_fill,
                );
            }
            draw_rect_border(canvas, width, height, bar, bar_border, 1);

            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: used_x,
                    y,
                    w: used_w,
                    h: row_h,
                },
                &format!(
                    "{} / {}",
                    crate::disk_popup::format_bytes(stats.used_bytes()),
                    crate::disk_popup::format_bytes(stats.total_bytes)
                ),
                &cfg.style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: pct_x,
                    y,
                    w: pct_w,
                    h: row_h,
                },
                &format!("{:.0}%", stats.percent()),
                &cfg.style,
                0,
                0,
            )?;

            y = y.saturating_add(row_h);
        }

        Ok(())
    }

    #[cfg(mhypr_module = "memory")]
    pub fn draw_memory_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::memory_popup::MemoryPopupModel,
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
        let bar_bg = cfg.bar_background_rgba()?;
        let memory_fill = cfg.memory_fill_rgba()?;
        let swap_fill = cfg.swap_fill_rgba()?;
        let hover = cfg.hover_rgba()?;

        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let row_h = cfg.row_height;
        let pad = cfg.padding;
        let mut y = panel.y.saturating_add(pad);
        let summaries = [
            (
                "Memory",
                model.stats.used_kib(),
                model.stats.total_kib,
                model.stats.memory_percent(),
                memory_fill,
            ),
            (
                "Swap",
                model.stats.swap_used_kib(),
                model.stats.swap_total_kib,
                model.stats.swap_percent(),
                swap_fill,
            ),
        ];

        for (label, used, total, percent, fill) in summaries {
            let row = Rect {
                x: panel.x + pad,
                y,
                w: panel.w - pad * 2,
                h: row_h,
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: row.x,
                    y,
                    w: 56,
                    h: row_h,
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
                    x: row.x + 60,
                    y,
                    w: 118,
                    h: row_h,
                },
                &format!(
                    "{} / {}",
                    crate::memory_popup::format_kib(used),
                    crate::memory_popup::format_kib(total)
                ),
                &cfg.style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: row.x + 182,
                    y,
                    w: 48,
                    h: row_h,
                },
                &format!("{percent:.0}%"),
                &cfg.style,
                0,
                0,
            )?;

            let bar_w = cfg.bar_width.min((row.w - 238).max(1));
            let bar_h = (row_h - 10).max(4);
            let bar_rect = Rect {
                x: row.x + row.w - bar_w,
                y: y + (row_h - bar_h) / 2,
                w: bar_w,
                h: bar_h,
            };
            fill_rect(canvas, width, height, bar_rect, bar_bg);
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
            y = y.saturating_add(row_h);
        }

        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: panel.x + pad,
                y,
                w: (panel.w - pad * 2).max(0),
                h: 1,
            },
            separator,
        );
        y = y.saturating_add(1);

        let header = Rect {
            x: panel.x + pad,
            y,
            w: panel.w - pad * 2,
            h: row_h,
        };
        let pid_w = 44;
        let gap = 4;
        let name_w = 140;
        let rss_w = 70;
        let pct_w = 52;
        let fixed = pid_w + gap + name_w + gap + rss_w + gap + pct_w + gap;
        let proc_bar_w = cfg.bar_width.min((header.w - fixed).max(1));
        let name_x = header.x + pid_w + gap;
        let rss_x = name_x + name_w + gap;
        let pct_x = rss_x + rss_w + gap;
        let proc_bar_x = pct_x + pct_w + gap;

        for (x, w, text) in [
            (header.x, pid_w, "PID"),
            (name_x, name_w, "Name"),
            (rss_x, rss_w, "RSS"),
            (pct_x, pct_w, "%MEM"),
        ] {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect { x, y, w, h: row_h },
                text,
                &cfg.style,
                0,
                0,
            )?;
        }
        y = y.saturating_add(row_h);

        for (index, process) in model.processes.iter().enumerate() {
            let row = Rect {
                x: panel.x + pad,
                y,
                w: panel.w - pad * 2,
                h: row_h,
            };
            if model.hovered_process == Some(index) {
                fill_rect(canvas, width, height, row, hover);
            }

            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: row.x,
                    y,
                    w: pid_w,
                    h: row_h,
                },
                &process.pid.to_string(),
                &cfg.style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: name_x,
                    y,
                    w: name_w,
                    h: row_h,
                },
                &process.name,
                &cfg.style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: rss_x,
                    y,
                    w: rss_w,
                    h: row_h,
                },
                &crate::memory_popup::format_kib(process.rss_kib),
                &cfg.style,
                0,
                0,
            )?;
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: pct_x,
                    y,
                    w: pct_w,
                    h: row_h,
                },
                &format!("{:.1}%", process.percent),
                &cfg.style,
                0,
                0,
            )?;

            let bar_h = (row_h - 12).max(4);
            let bar_rect = Rect {
                x: proc_bar_x,
                y: y + (row_h - bar_h) / 2,
                w: proc_bar_w,
                h: bar_h,
            };
            fill_rect(canvas, width, height, bar_rect, bar_bg);
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: bar_rect.x,
                    y: bar_rect.y,
                    w: ((bar_rect.w as f32 * process.percent.clamp(0.0, 100.0) / 100.0).round()
                        as i32)
                        .clamp(0, bar_rect.w),
                    h: bar_rect.h,
                },
                memory_fill,
            );

            y = y.saturating_add(row_h);
        }

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
        let estimated = (text.chars().count() as f32 * style.font_size * 0.62).ceil() as i32;
        let text_x = rect
            .x
            .saturating_add((rect.w.saturating_sub(estimated)).max(0) / 2);
        let text_w = rect.w.max(1) as f32;
        let line_height = (height as f32).max(style.font_size);

        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.font_size, line_height));
        buffer.set_size(Some(text_w), Some(height as f32));
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
                    rect.y.saturating_add(y),
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
            let module_width = module_width(&view);
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
                    self.draw_disk(canvas, width, height, rect, &view, disk);
                }
                #[cfg(mhypr_module = "memory")]
                ModuleVisual::Memory(memory) => {
                    self.draw_memory(canvas, width, height, rect, &view, memory)?;
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
        let icon_size = audio.icon_size.min(content_h).max(7);
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

        let body_h = (icon_size / 3).max(3);
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: icon_x,
                y: icon_y + (icon_size - body_h) / 2,
                w: 4,
                h: body_h,
            },
            color,
        );
        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: icon_x + 4,
                y: icon_y + 2,
                w: 4,
                h: (icon_size - 4).max(3),
            },
            color,
        );

        if audio.muted {
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: icon_x + 10,
                    y: icon_y + 3,
                    w: 2,
                    h: (icon_size - 6).max(3),
                },
                color,
            );
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: icon_x + 8,
                    y: icon_y + icon_size / 2 - 1,
                    w: 6,
                    h: 2,
                },
                color,
            );
        } else {
            if audio.percent > 0 {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: icon_x + 10,
                        y: icon_y + icon_size / 3,
                        w: 1,
                        h: (icon_size / 3).max(3),
                    },
                    color,
                );
            }
            if audio.percent >= 50 {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: icon_x + 13,
                        y: icon_y + 2,
                        w: 1,
                        h: (icon_size - 4).max(4),
                    },
                    color,
                );
            }
        }

        let bar_x = icon_x
            .saturating_add(icon_size)
            .saturating_add(audio.text_gap);
        let bar_y = rect.y + (rect.h - audio.bar_height) / 2;
        let bar = Rect {
            x: bar_x,
            y: bar_y,
            w: audio.bar_width,
            h: audio.bar_height,
        };
        fill_rect(canvas, width, height, bar, audio.bar_background);
        let fill_w = ((audio.bar_width as f32 * audio.percent.min(100) as f32 / 100.0).round()
            as i32)
            .clamp(0, audio.bar_width);
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
                color,
            );
        }

        let text_x = bar_x
            .saturating_add(audio.bar_width)
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
        let icon_size = brightness.icon_size.min(content_h).max(7);
        let icon_x = rect.x.saturating_add(padding_x);
        let icon_y = rect
            .y
            .saturating_add(padding_y)
            .saturating_add((content_h - icon_size) / 2);
        let cx = icon_x + icon_size / 2;
        let cy = icon_y + icon_size / 2;
        let core = 5;

        fill_rect(
            canvas,
            width,
            height,
            Rect {
                x: cx - core / 2,
                y: cy - core / 2,
                w: core,
                h: core,
            },
            brightness.fill,
        );
        for ray in [
            Rect {
                x: cx,
                y: icon_y,
                w: 1,
                h: 3,
            },
            Rect {
                x: cx,
                y: icon_y + icon_size - 3,
                w: 1,
                h: 3,
            },
            Rect {
                x: icon_x,
                y: cy,
                w: 3,
                h: 1,
            },
            Rect {
                x: icon_x + icon_size - 3,
                y: cy,
                w: 3,
                h: 1,
            },
        ] {
            fill_rect(canvas, width, height, ray, brightness.fill);
        }

        let bar_x = icon_x
            .saturating_add(icon_size)
            .saturating_add(brightness.text_gap);
        let bar = Rect {
            x: bar_x,
            y: rect.y + (rect.h - brightness.bar_height) / 2,
            w: brightness.bar_width,
            h: brightness.bar_height,
        };
        fill_rect(canvas, width, height, bar, brightness.bar_background);
        let fill_w = ((brightness.bar_width as f32 * brightness.percent.min(100) as f32 / 100.0)
            .round() as i32)
            .clamp(0, brightness.bar_width);
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
                brightness.fill,
            );
        }

        let text_x = bar_x
            .saturating_add(brightness.bar_width)
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
        let icon_h = layout.icon_height.min(content_h).max(5);
        let icon = Rect {
            x: rect.x.saturating_add(padding_x),
            y: rect
                .y
                .saturating_add(padding_y)
                .saturating_add((content_h - icon_h) / 2),
            w: layout.icon_width,
            h: icon_h,
        };
        let color = view.style.foreground_rgba()?;
        draw_rect_border(canvas, width, height, icon, color, 1);

        let name = layout.name.to_ascii_lowercase();
        if name.contains("master") {
            let split_x = icon.x + (icon.w * 3 / 5);
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: split_x,
                    y: icon.y + 1,
                    w: 1,
                    h: (icon.h - 2).max(1),
                },
                color,
            );
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: split_x + 1,
                    y: icon.y + icon.h / 2,
                    w: (icon.x + icon.w - split_x - 2).max(1),
                    h: 1,
                },
                color,
            );
        } else if name.contains("scroll") {
            for numerator in [1, 2] {
                let split_x = icon.x + icon.w * numerator / 3;
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: split_x,
                        y: icon.y + 1,
                        w: 1,
                        h: (icon.h - 2).max(1),
                    },
                    color,
                );
            }
        } else if name.contains("monocle") {
            draw_rect_border(
                canvas,
                width,
                height,
                Rect {
                    x: icon.x + 3,
                    y: icon.y + 3,
                    w: (icon.w - 6).max(1),
                    h: (icon.h - 6).max(1),
                },
                color,
                1,
            );
        } else {
            let split_x = icon.x + icon.w / 2;
            let split_y = icon.y + icon.h / 2;
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: split_x,
                    y: icon.y + 1,
                    w: 1,
                    h: (icon.h - 2).max(1),
                },
                color,
            );
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: split_x + 1,
                    y: split_y,
                    w: (icon.x + icon.w - split_x - 2).max(1),
                    h: 1,
                },
                color,
            );
            fill_rect(
                canvas,
                width,
                height,
                Rect {
                    x: split_x + (icon.w - icon.w / 2) / 2,
                    y: split_y + 1,
                    w: 1,
                    h: (icon.y + icon.h - split_y - 2).max(1),
                },
                color,
            );
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

        let mut cursor_x = rect.x.saturating_add(padding_x);
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
    ) {
        let padding_x = view.style.padding_x.max(0);
        let padding_y = view.style.padding_y.max(0);
        let content_h = rect.h.saturating_sub(padding_y.saturating_mul(2)).max(1);
        let bar_h = disk.bar_height.min(content_h).max(1);
        let bar = Rect {
            x: rect.x.saturating_add(padding_x),
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
        let label_w = estimate_text_width("M", style).max(1);
        let percent_w = estimate_text_width("100%", style).max(1);
        let start_x = rect.x.saturating_add(padding_x);
        let bar_x = start_x
            .saturating_add(label_w)
            .saturating_add(memory.text_gap);
        let percent_x = bar_x
            .saturating_add(memory.bar_width)
            .saturating_add(memory.text_gap);

        for (index, (label, percent, fill)) in [
            ("M", memory.memory_percent, memory.memory_fill),
            ("S", memory.swap_percent, memory.swap_fill),
        ]
        .into_iter()
        .enumerate()
        {
            let row_y = rect
                .y
                .saturating_add(padding_y)
                .saturating_add(index as i32 * (row_h + gap));

            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: start_x,
                    y: row_y,
                    w: label_w,
                    h: row_h,
                },
                label,
                style,
                0,
                0,
            )?;

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
    fn draw_text_content(
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
}

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

pub struct ModuleHit<'a> {
    pub name: &'a str,
    pub offset_x: i32,
}

pub fn module_at_x<'a>(
    x: f64,
    width: u32,
    workspace_visible: bool,
    config: &'a BarConfig,
    modules: &ModuleManager,
) -> Option<ModuleHit<'a>> {
    if x < 0.0 {
        return None;
    }

    let workspace_width = if workspace_visible {
        config.workspaces.strip_width()
    } else {
        0
    };
    let (left_x, center_x, right_x) = group_origins(width, workspace_width, config, modules);

    hit_group(x, left_x, &config.left, modules)
        .or_else(|| hit_group(x, center_x, &config.center, modules))
        .or_else(|| hit_group(x, right_x, &config.right, modules))
}

fn group_origins(
    width: u32,
    workspace_width: i32,
    config: &BarConfig,
    modules: &ModuleManager,
) -> (i32, i32, i32) {
    let left_width = group_width(&config.left, modules);
    let center_width = group_width(&config.center, modules);
    let right_width = group_width(&config.right, modules);

    let left_x = workspace_width;
    let left_end = left_x.saturating_add(left_width);
    let right_x = (width as i32 - right_width).max(0);
    let centered_x = (width as i32 - center_width) / 2;
    let center_max = right_x.saturating_sub(center_width);
    let center_x = if center_max >= left_end {
        centered_x.clamp(left_end, center_max)
    } else {
        left_end
    };

    (left_x, center_x, right_x)
}

fn hit_group<'a>(
    x: f64,
    start_x: i32,
    names: &'a [String],
    modules: &ModuleManager,
) -> Option<ModuleHit<'a>> {
    let mut cursor = start_x;
    for name in names {
        let Some(view) = modules.view(name) else {
            continue;
        };
        let width = module_width(&view);
        if x >= cursor as f64 && x < cursor.saturating_add(width) as f64 {
            return Some(ModuleHit {
                name: name.as_str(),
                offset_x: (x - cursor as f64) as i32,
            });
        }
        cursor = cursor.saturating_add(width);
    }
    None
}

fn group_width(names: &[String], modules: &ModuleManager) -> i32 {
    names
        .iter()
        .filter_map(|name| modules.view(name))
        .map(|view| module_width(&view))
        .fold(0_i32, i32::saturating_add)
}

fn module_width(view: &ModuleView<'_>) -> i32 {
    if let Some(width) = view.width_override {
        return width.max(0);
    }

    #[cfg(mhypr_module = "audio")]
    if let ModuleVisual::Audio(audio) = &view.visual {
        let style = view.style;
        let percent_width = estimate_text_width("150%", style);
        let content = audio
            .icon_size
            .saturating_add(audio.text_gap)
            .saturating_add(audio.bar_width)
            .saturating_add(audio.text_gap)
            .saturating_add(percent_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "battery")]
    if let ModuleVisual::Battery(battery) = &view.visual {
        let style = view.style;
        let percent_width = estimate_text_width("100%", style);
        let icon_width = battery
            .icon_width
            .saturating_add(1)
            .saturating_add(battery.icon_tip_width)
            .saturating_add(battery.text_gap);
        let content = icon_width.saturating_add(percent_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "brightness")]
    if let ModuleVisual::Brightness(brightness) = &view.visual {
        let style = view.style;
        let percent_width = estimate_text_width("100%", style);
        let content = brightness
            .icon_size
            .saturating_add(brightness.text_gap)
            .saturating_add(brightness.bar_width)
            .saturating_add(brightness.text_gap)
            .saturating_add(percent_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "clock")]
    if let ModuleVisual::Clock(clock) = &view.visual {
        let style = view.style;
        let mut time_style = style.clone();
        time_style.font_size = clock.time_font_size;
        let mut date_style = style.clone();
        date_style.font_size = clock.date_font_size;
        let time_width = estimate_text_width(&clock.time_text, &time_style);
        let date_width = estimate_text_width(&clock.date_text, &date_style);
        let content = time_width.max(date_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "layout")]
    if let ModuleVisual::Layout(layout) = &view.visual {
        let style = view.style;
        return style
            .min_width
            .max(
                layout
                    .icon_width
                    .saturating_add(style.padding_x.saturating_mul(2)),
            )
            .max(1);
    }

    #[cfg(mhypr_module = "cpu")]
    if let ModuleVisual::Cpu(cpu) = &view.visual {
        let style = view.style;
        let graph_width = if cpu.graph_enabled {
            cpu.graph_width
        } else {
            0
        };
        let text_width = if view.text.is_empty() {
            0
        } else {
            estimate_text_width(view.text, style)
        };
        let gap = if graph_width > 0 && text_width > 0 {
            cpu.text_gap
        } else {
            0
        };
        let content = graph_width.saturating_add(gap).saturating_add(text_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "disk")]
    if let ModuleVisual::Disk(disk) = &view.visual {
        let style = view.style;
        return style
            .min_width
            .max(
                disk.bar_width
                    .saturating_add(style.padding_x.saturating_mul(2)),
            )
            .max(1);
    }

    #[cfg(mhypr_module = "memory")]
    if let ModuleVisual::Memory(memory) = &view.visual {
        let style = view.style;
        let label_width = estimate_text_width("M", style);
        let percent_width = estimate_text_width("100%", style);
        let content = label_width
            .saturating_add(memory.text_gap)
            .saturating_add(memory.bar_width)
            .saturating_add(memory.text_gap)
            .saturating_add(percent_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    if view.text.is_empty() {
        return 0;
    }
    let style = view.style;
    let text_width = estimate_text_width(view.text, style);
    style
        .min_width
        .max(text_width.saturating_add(style.padding_x.saturating_mul(2)))
        .max(1)
}

fn estimate_text_width(text: &str, style: &ModuleStyle) -> i32 {
    let em = text
        .chars()
        .map(|ch| if ch.is_ascii() { 0.62_f32 } else { 1.0_f32 })
        .sum::<f32>();
    (em * style.font_size).ceil() as i32
}

#[cfg(mhypr_module = "battery")]
fn battery_level_color(battery: &crate::modules::battery::BatteryVisual) -> [u8; 4] {
    if battery.charging {
        return battery.charging_color;
    }
    if battery.capacity >= battery.high_percent {
        battery.high_color
    } else if battery.capacity >= battery.medium_percent {
        battery.medium_color
    } else if battery.capacity >= battery.low_percent {
        battery.low_color
    } else {
        battery.critical_color
    }
}

#[cfg(mhypr_module = "battery")]
fn draw_battery_bolt(canvas: &mut [u8], width: u32, height: u32, inner: Rect, color: [u8; 4]) {
    let cx = inner.x + inner.w / 2;
    let top = inner.y + 1;
    let bottom = inner.y + inner.h - 1;
    if bottom <= top {
        return;
    }

    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: cx,
            y: top,
            w: 2,
            h: ((inner.h - 2) / 2).max(2),
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: cx - 2,
            y: inner.y + inner.h / 2 - 1,
            w: 5,
            h: 2,
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: cx - 2,
            y: inner.y + inner.h / 2,
            w: 2,
            h: (bottom - (inner.y + inner.h / 2)).max(2),
        },
        color,
    );
}

#[cfg(mhypr_module = "cpu")]
fn cpu_graph_color(cpu: &crate::modules::cpu::CpuVisual, level: f32) -> [u8; 4] {
    let level = level.clamp(0.0, 1.0);
    let warn = (cpu.warn_percent / 100.0).clamp(0.55, 0.95);
    let mid_start = (warn * 0.55).clamp(0.25, warn);

    if level <= mid_start {
        cpu.graph_low
    } else if level <= warn {
        let span = (warn - mid_start).max(f32::EPSILON);
        lerp_rgba(cpu.graph_low, cpu.graph_mid, (level - mid_start) / span)
    } else {
        let span = (1.0 - warn).max(f32::EPSILON);
        lerp_rgba(cpu.graph_mid, cpu.graph_high, (level - warn) / span)
    }
}

#[cfg(mhypr_module = "cpu")]
fn lerp_rgba(from: [u8; 4], to: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| ((a as f32 + (b as f32 - a as f32) * t).round()) as u8;
    [
        mix(from[0], to[0]),
        mix(from[1], to[1]),
        mix(from[2], to[2]),
        mix(from[3], to[3]),
    ]
}

#[allow(clippy::too_many_arguments)]
fn draw_native_argb_pixmap(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    target_size: i32,
    source_width: i32,
    source_height: i32,
    pixels: &[u8],
) {
    if target_size <= 0 || source_width <= 0 || source_height <= 0 {
        return;
    }
    let sw = source_width as usize;
    let sh = source_height as usize;
    if pixels.len() < sw.saturating_mul(sh).saturating_mul(4) {
        return;
    }

    let (draw_w, draw_h) = if source_width >= source_height {
        (
            target_size,
            ((source_height as i64 * target_size as i64) / source_width as i64)
                .max(1)
                .min(target_size as i64) as i32,
        )
    } else {
        (
            ((source_width as i64 * target_size as i64) / source_height as i64)
                .max(1)
                .min(target_size as i64) as i32,
            target_size,
        )
    };
    let x0 = x.saturating_add((target_size - draw_w) / 2);
    let y0 = y.saturating_add((target_size - draw_h) / 2);

    for dy in 0..draw_h {
        let sy = (dy as i64 * source_height as i64 / draw_h as i64) as usize;
        for dx in 0..draw_w {
            let sx = (dx as i64 * source_width as i64 / draw_w as i64) as usize;
            let offset = (sy * sw + sx) * 4;
            let (r, g, b, a) = if cfg!(target_endian = "little") {
                (
                    pixels[offset + 2],
                    pixels[offset + 1],
                    pixels[offset],
                    pixels[offset + 3],
                )
            } else {
                (
                    pixels[offset + 1],
                    pixels[offset + 2],
                    pixels[offset + 3],
                    pixels[offset],
                )
            };
            blend_pixel_rgba(
                canvas,
                width,
                height,
                x0.saturating_add(dx),
                y0.saturating_add(dy),
                [r, g, b, a],
            );
        }
    }
}

fn blend_pixel_rgba(canvas: &mut [u8], width: u32, height: u32, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let offset = ((y as u32 * width + x as u32) * 4) as usize;
    blend_at(&mut canvas[offset..offset + 4], rgba);
}

#[cfg(any(
    mhypr_module = "audio",
    mhypr_module = "battery",
    mhypr_module = "brightness",
    mhypr_module = "clock",
    mhypr_module = "cpu",
    mhypr_module = "disk",
    mhypr_module = "layout",
    mhypr_module = "memory"
))]
fn draw_rect_border(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    color: [u8; 4],
    border_width: i32,
) {
    if border_width <= 0 || rect.w <= 0 || rect.h <= 0 {
        return;
    }
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: border_width,
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x,
            y: rect.y + rect.h - border_width,
            w: rect.w,
            h: border_width,
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x,
            y: rect.y,
            w: border_width,
            h: rect.h,
        },
        color,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x + rect.w - border_width,
            y: rect.y,
            w: border_width,
            h: rect.h,
        },
        color,
    );
}

#[cfg(mhypr_module = "tray")]
fn popup_rect(rect: TrayPopupRect) -> Rect {
    Rect {
        x: rect.x.round() as i32,
        y: rect.y.round() as i32,
        w: rect.w.round().max(0.0) as i32,
        h: rect.h.round().max(0.0) as i32,
    }
}

#[cfg(mhypr_module = "tray")]
fn draw_popup_panel(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    rect: TrayPopupRect,
    background: [u8; 4],
    border: [u8; 4],
    border_width: i32,
) {
    let rect_i = popup_rect(rect);
    fill_rect(canvas, width, height, rect_i, background);
    if border_width <= 0 {
        return;
    }
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect_i.x,
            y: rect_i.y,
            w: rect_i.w,
            h: border_width,
        },
        border,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect_i.x,
            y: rect_i.y + rect_i.h - border_width,
            w: rect_i.w,
            h: border_width,
        },
        border,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect_i.x,
            y: rect_i.y,
            w: border_width,
            h: rect_i.h,
        },
        border,
    );
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect_i.x + rect_i.w - border_width,
            y: rect_i.y,
            w: border_width,
            h: rect_i.h,
        },
        border,
    );
}

#[cfg(mhypr_module = "tray")]
fn draw_popup_separator(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    rect: TrayPopupRect,
    color: [u8; 4],
    inset: i32,
) {
    let rect = popup_rect(rect);
    fill_rect(
        canvas,
        width,
        height,
        Rect {
            x: rect.x + inset,
            y: rect.y,
            w: (rect.w - inset.saturating_mul(2)).max(0),
            h: 1,
        },
        color,
    );
}

fn fill_rect(canvas: &mut [u8], width: u32, height: u32, rect: Rect, rgba: [u8; 4]) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }

    let x0 = rect.x.clamp(0, width as i32) as u32;
    let y0 = rect.y.clamp(0, height as i32) as u32;
    let x1 = rect.x.saturating_add(rect.w).clamp(0, width as i32) as u32;
    let y1 = rect.y.saturating_add(rect.h).clamp(0, height as i32) as u32;

    for y in y0..y1 {
        for x in x0..x1 {
            blend_rgba(canvas, width, x, y, rgba);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blend_block(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: Color,
) {
    for yy in 0..h as i32 {
        for xx in 0..w as i32 {
            blend_pixel(canvas, width, height, x + xx, y + yy, color);
        }
    }
}

fn blend_pixel(canvas: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }

    let offset = ((y as u32 * width + x as u32) * 4) as usize;
    blend_at(
        &mut canvas[offset..offset + 4],
        [color.r(), color.g(), color.b(), color.a()],
    );
}

fn blend_rgba(canvas: &mut [u8], width: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let offset = ((y * width + x) * 4) as usize;
    blend_at(&mut canvas[offset..offset + 4], rgba);
}

fn blend_at(dst: &mut [u8], rgba: [u8; 4]) {
    let alpha = rgba[3] as u16;
    let inv = 255 - alpha;

    let dst_b = dst[0] as u16;
    let dst_g = dst[1] as u16;
    let dst_r = dst[2] as u16;
    let dst_a = dst[3] as u16;

    dst[0] = ((rgba[2] as u16 * alpha + dst_b * inv) / 255) as u8;
    dst[1] = ((rgba[1] as u16 * alpha + dst_g * inv) / 255) as u8;
    dst[2] = ((rgba[0] as u16 * alpha + dst_r * inv) / 255) as u8;
    dst[3] = (alpha + dst_a * inv / 255).min(255) as u8;
}
