use super::*;

#[cfg(mhypr_module = "battery")]
pub(super) fn battery_level_color(battery: &crate::modules::battery::BatteryVisual) -> [u8; 4] {
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
pub(super) fn draw_battery_bolt(canvas: &mut [u8], width: u32, height: u32, inner: Rect, color: [u8; 4]) {
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
pub(super) fn cpu_graph_color(cpu: &crate::modules::cpu::CpuVisual, level: f32) -> [u8; 4] {
    cpu_usage_color(
        cpu.warn_percent,
        cpu.graph_low,
        cpu.graph_mid,
        cpu.graph_high,
        level,
    )
}

#[cfg(mhypr_module = "cpu")]
pub(crate) fn cpu_usage_color(
    warn_percent: f32,
    low: [u8; 4],
    mid: [u8; 4],
    high: [u8; 4],
    level: f32,
) -> [u8; 4] {
    let level = level.clamp(0.0, 1.0);
    let warn = (warn_percent / 100.0).clamp(0.55, 0.95);
    let mid_start = (warn * 0.55).clamp(0.25, warn);

    if level <= mid_start {
        low
    } else if level <= warn {
        let span = (warn - mid_start).max(f32::EPSILON);
        lerp_rgba(low, mid, (level - mid_start) / span)
    } else {
        let span = (1.0 - warn).max(f32::EPSILON);
        lerp_rgba(mid, high, (level - warn) / span)
    }
}

#[cfg(mhypr_module = "cpu")]
pub(super) fn lerp_rgba(from: [u8; 4], to: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| ((a as f32 + (b as f32 - a as f32) * t).round()) as u8;
    [
        mix(from[0], to[0]),
        mix(from[1], to[1]),
        mix(from[2], to[2]),
        mix(from[3], to[3]),
    ]
}

#[cfg(mhypr_module = "menu")]
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_rgba_pixmap(
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
            let rgba = [
                pixels[offset],
                pixels[offset + 1],
                pixels[offset + 2],
                pixels[offset + 3],
            ];
            if rgba[3] == 0 {
                continue;
            }
            blend_pixel_rgba(
                canvas,
                width,
                height,
                x0.saturating_add(dx),
                y0.saturating_add(dy),
                rgba,
            );
        }
    }
}

#[cfg(any(
    mhypr_module = "audio",
    mhypr_module = "brightness",
    mhypr_module = "cpu",
    mhypr_module = "disk",
    mhypr_module = "gpu",
    mhypr_module = "layout",
    mhypr_module = "memory",
    mhypr_module = "monitor"
))]
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tinted_rgba_mask(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    target_size: i32,
    source_width: i32,
    source_height: i32,
    pixels: &[u8],
    color: [u8; 4],
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
            let source_alpha = pixels[offset + 3] as u16;
            if source_alpha == 0 {
                continue;
            }
            let alpha = ((source_alpha * color[3] as u16) / 255) as u8;
            blend_pixel_rgba(
                canvas,
                width,
                height,
                x0.saturating_add(dx),
                y0.saturating_add(dy),
                [color[0], color[1], color[2], alpha],
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_native_argb_pixmap(
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

pub(super) fn blend_pixel_rgba(canvas: &mut [u8], width: u32, height: u32, x: i32, y: i32, rgba: [u8; 4]) {
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
pub(super) fn draw_rect_border(
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
pub(super) fn popup_rect(rect: TrayPopupRect) -> Rect {
    Rect {
        x: rect.x.round() as i32,
        y: rect.y.round() as i32,
        w: rect.w.round().max(0.0) as i32,
        h: rect.h.round().max(0.0) as i32,
    }
}

#[cfg(mhypr_module = "tray")]
pub(super) fn draw_popup_panel(
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
pub(super) fn draw_popup_separator(
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

#[cfg(mhypr_module = "monitor")]
pub(super) fn fill_circle(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    diameter: i32,
    rgba: [u8; 4],
) {
    let diameter = diameter.max(1);
    let radius = diameter as f32 / 2.0;
    let start_x = cx - diameter / 2;
    let start_y = cy - diameter / 2;
    for y in 0..diameter {
        for x in 0..diameter {
            let dx = x as f32 + 0.5 - radius;
            let dy = y as f32 + 0.5 - radius;
            if dx * dx + dy * dy <= radius * radius {
                fill_rect(
                    canvas,
                    width,
                    height,
                    Rect {
                        x: start_x + x,
                        y: start_y + y,
                        w: 1,
                        h: 1,
                    },
                    rgba,
                );
            }
        }
    }
}

pub(super) fn fill_rect(canvas: &mut [u8], width: u32, height: u32, rect: Rect, rgba: [u8; 4]) {
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
pub(super) fn blend_block(
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

pub(super) fn blend_pixel(canvas: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }

    let offset = ((y as u32 * width + x as u32) * 4) as usize;
    blend_at(
        &mut canvas[offset..offset + 4],
        [color.r(), color.g(), color.b(), color.a()],
    );
}

pub(super) fn blend_rgba(canvas: &mut [u8], width: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let offset = ((y * width + x) * 4) as usize;
    blend_at(&mut canvas[offset..offset + 4], rgba);
}

pub(super) fn blend_at(dst: &mut [u8], rgba: [u8; 4]) {
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
