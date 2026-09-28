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
                #[cfg(mhypr_module = "cpu")]
                ModuleVisual::Cpu(cpu) => {
                    self.draw_cpu(canvas, width, height, rect, &view, cpu)?;
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
