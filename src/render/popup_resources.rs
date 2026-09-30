use super::*;
#[allow(unused_imports)]
use super::{layout::estimate_text_width, primitives::*};

impl Renderer {
    #[cfg(mhypr_module = "gpu")]
    pub fn draw_gpu_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::gpu::popup::GpuPopupModel,
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
        let utilization_fill = cfg.utilization_fill_rgba()?;
        let memory_fill = cfg.memory_fill_rgba()?;

        fill_rect(canvas, width, height, panel, background);
        draw_rect_border(canvas, width, height, panel, border, 1);

        let pad = cfg.padding;
        let mut title_style = cfg.style.clone();
        title_style.font_size = (cfg.style.font_size + 1.0).max(cfg.style.font_size);
        let title_rect = Rect {
            x: panel.x + pad,
            y: panel.y + pad,
            w: (panel.w - pad * 2).max(1),
            h: cfg.title_height,
        };
        self.draw_text_content(
            canvas,
            width,
            height,
            title_rect,
            &model.snapshot.name,
            &title_style,
            0,
            0,
        )?;

        let mut y = title_rect.y.saturating_add(cfg.title_height);
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

        let rows = [
            ("Backend", model.backend_text(), None),
            (
                "GPU Load",
                model.utilization_text(),
                model
                    .snapshot
                    .utilization_percent
                    .map(|value| (value, utilization_fill)),
            ),
            (
                "VRAM",
                model.memory_text(),
                model
                    .snapshot
                    .memory_percent
                    .map(|value| (value, memory_fill)),
            ),
            ("Temperature", model.temperature_text(), None),
            ("Power", model.power_text(), None),
        ];

        let label_w = 92;
        let value_w = 154;
        for (label, value, bar) in rows {
            let row = Rect {
                x: panel.x + pad,
                y,
                w: (panel.w - pad * 2).max(1),
                h: cfg.row_height,
            };
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x: row.x,
                    y: row.y,
                    w: label_w.min(row.w).max(1),
                    h: row.h,
                },
                label,
                &cfg.style,
                0,
                0,
            )?;

            if let Some((percent, fill)) = bar {
                let bar_w = cfg
                    .bar_width
                    .min(row.w.saturating_sub(label_w + value_w + 12))
                    .max(1);
                let bar_h = (row.h - 14).clamp(4, 10);
                let bar_rect = Rect {
                    x: row.x + label_w,
                    y: row.y + (row.h - bar_h) / 2,
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
                self.draw_text_content(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: bar_rect.x + bar_rect.w + 8,
                        y: row.y,
                        w: (row.x + row.w - (bar_rect.x + bar_rect.w + 8)).max(1),
                        h: row.h,
                    },
                    &value,
                    &cfg.style,
                    0,
                    0,
                )?;
            } else {
                self.draw_text_content(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: row.x + label_w,
                        y: row.y,
                        w: (row.w - label_w).max(1),
                        h: row.h,
                    },
                    &value,
                    &cfg.style,
                    0,
                    0,
                )?;
            }

            y = y.saturating_add(cfg.row_height);
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

        let content_x = panel.x + pad;
        let content_w = (panel.w - pad * 2).max(1);
        let pid_w = 58;
        let type_w = 48;
        let gpu_w = 50;
        let mem_w = 50;
        let vram_w = 76;
        let process_w = (content_w - pid_w - type_w - gpu_w - mem_w - vram_w).max(1);

        let header_cells = [
            ("PID", pid_w),
            ("Type", type_w),
            ("GPU", gpu_w),
            ("MEM", mem_w),
            ("VRAM", vram_w),
            ("Process", process_w),
        ];
        let mut x = content_x;
        for (label, cell_w) in header_cells {
            self.draw_text_content(
                canvas,
                width,
                height,
                Rect {
                    x,
                    y,
                    w: cell_w,
                    h: cfg.row_height,
                },
                label,
                &cfg.style,
                0,
                0,
            )?;
            x = x.saturating_add(cell_w);
        }
        y = y.saturating_add(cfg.row_height);

        if model.processes.is_empty() {
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
                "No GPU processes",
                &cfg.style,
                0,
                0,
            )?;
        } else {
            for process in &model.processes {
                let values = [
                    (process.pid.to_string(), pid_w),
                    (process.kind.clone(), type_w),
                    (
                        crate::modules::gpu::popup::GpuPopupModel::process_gpu_text(process),
                        gpu_w,
                    ),
                    (
                        crate::modules::gpu::popup::GpuPopupModel::process_memory_percent_text(process),
                        mem_w,
                    ),
                    (
                        crate::modules::gpu::popup::GpuPopupModel::process_memory_text(process),
                        vram_w,
                    ),
                    (process.name.clone(), process_w),
                ];
                let mut x = content_x;
                for (value, cell_w) in values {
                    self.draw_text_content(
                        canvas,
                        width,
                        height,
                        Rect {
                            x,
                            y,
                            w: cell_w,
                            h: cfg.row_height,
                        },
                        &value,
                        &cfg.style,
                        0,
                        0,
                    )?;
                    x = x.saturating_add(cell_w);
                }
                y = y.saturating_add(cfg.row_height);
            }
        }

        Ok(())
    }

    #[cfg(mhypr_module = "cpu")]
    pub fn draw_cpu_popup(
        &mut self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        model: &crate::modules::cpu::popup::CpuPopupModel,
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
            let bar_fill = cpu_usage_color(
                model.warn_percent,
                model.graph_low,
                model.graph_mid,
                model.graph_high,
                usage / 100.0,
            );
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
        model: &crate::modules::disk::popup::DiskPopupModel,
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
                    crate::modules::disk::popup::format_bytes(stats.used_bytes()),
                    crate::modules::disk::popup::format_bytes(stats.total_bytes)
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
        model: &crate::modules::memory::popup::MemoryPopupModel,
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
                    crate::modules::memory::popup::format_kib(used),
                    crate::modules::memory::popup::format_kib(total)
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
            w: (panel.w - pad * 2).max(0),
            h: row_h,
        };
        let content_w = header.w.max(0);
        let pid_w = 40.min(content_w);
        let gap = 4;
        let pid_name_gap = gap.min(content_w.saturating_sub(pid_w));
        let name_x = header
            .x
            .saturating_add(pid_w)
            .saturating_add(pid_name_gap);
        let row_right = header.x.saturating_add(content_w);
        // Keep every right-side cell strictly inside the highlighted row.  The
        // small guard also absorbs glyph overhang from the text renderer.
        let right_guard = 2.min(content_w);
        let mut right = row_right.saturating_sub(right_guard);
        let mut reserve_right = |desired: i32| {
            let available = right.saturating_sub(name_x).max(0);
            let w = desired.max(0).min(available);
            let x = right.saturating_sub(w);
            right = x;
            (x, w.min(row_right.saturating_sub(x).max(0)))
        };

        let (proc_bar_x, proc_bar_w) = reserve_right(cfg.bar_width.min(48));
        let _ = reserve_right(gap);
        let (swap_pct_x, swap_pct_w) = reserve_right(48);
        let _ = reserve_right(gap);
        let (swap_x, swap_w) = reserve_right(48);
        let _ = reserve_right(gap);
        let (pct_x, pct_w) = reserve_right(44);
        let _ = reserve_right(gap);
        let (rss_x, rss_w) = reserve_right(48);
        let name_gap = gap.min(right.saturating_sub(name_x).max(0));
        let name_w = right.saturating_sub(name_x).saturating_sub(name_gap);

        for (x, w, text) in [
            (header.x, pid_w, "PID"),
            (name_x, name_w, "Name"),
            (rss_x, rss_w, "RSS"),
            (pct_x, pct_w, "%MEM"),
            (swap_x, swap_w, "Swap"),
            (swap_pct_x, swap_pct_w, "%SWAP"),
        ] {
            if w <= 0 {
                continue;
            }
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
                w: header.w,
                h: row_h,
            };
            if model.hovered_process == Some(index) {
                fill_rect(canvas, width, height, row, hover);
            }

            let pid_text = process.pid.to_string();
            let rss_text = crate::modules::memory::popup::format_kib(process.rss_kib);
            let pct_text = format!("{:.1}%", process.percent);
            let swap_text = crate::modules::memory::popup::format_kib(process.swap_kib);
            let swap_pct_text = format!("{:.1}%", process.swap_percent);
            for (x, w, text) in [
                (row.x, pid_w, pid_text.as_str()),
                (name_x, name_w, process.name.as_str()),
                (rss_x, rss_w, rss_text.as_str()),
                (pct_x, pct_w, pct_text.as_str()),
                (swap_x, swap_w, swap_text.as_str()),
                (swap_pct_x, swap_pct_w, swap_pct_text.as_str()),
            ] {
                if w <= 0 {
                    continue;
                }
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

            let bar_h = (row_h - 12).max(6);
            let mem_h = (bar_h / 2).max(2);
            let mem_bar = Rect {
                x: proc_bar_x,
                y: y + (row_h - bar_h) / 2,
                w: proc_bar_w,
                h: mem_h,
            };
            let swap_bar = Rect {
                x: proc_bar_x,
                y: mem_bar.y + mem_h,
                w: proc_bar_w,
                h: (bar_h - mem_h).max(2),
            };
            for (bar_rect, percent, fill) in [
                (mem_bar, process.percent, memory_fill),
                (swap_bar, process.swap_percent, swap_fill),
            ] {
                fill_rect(canvas, width, height, bar_rect, bar_bg);
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: bar_rect.x,
                        y: bar_rect.y,
                        w: ((bar_rect.w as f32 * percent.clamp(0.0, 100.0) / 100.0).round()
                            as i32)
                            .clamp(0, bar_rect.w),
                        h: bar_rect.h,
                    },
                    fill,
                );
            }

            y = y.saturating_add(row_h);
        }

        Ok(())
    }

}
