use anyhow::Result;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

use crate::{
    config::{BarConfig, ModuleStyle, WorkspacesConfig},
    hyprland::{MonitorState, Snapshot},
    modules::{ModuleManager, ModuleView},
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

            if name == "tray" {
                if let Some(tray) = tray {
                    self.draw_tray(canvas, width, height, rect, tray, &view);
                }
            } else {
                self.draw_text(canvas, width, height, rect, &view)?;
            }
            x = x.saturating_add(module_width);
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
        let style = view.style;
        let foreground = style.foreground_rgba()?;
        let color = Color::rgba(foreground[0], foreground[1], foreground[2], foreground[3]);
        let padding_x = style.padding_x.max(0);
        let padding_y = style.padding_y.max(0);
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
        buffer.set_text(view.text, &attrs, Shaping::Advanced, None);

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
