use super::*;

pub struct ModuleHit<'a> {
    pub name: &'a str,
    pub offset_x: i32,
}

pub fn workspace_at_x(
    x: f64,
    workspace_visible: bool,
    config: &BarConfig,
    modules: &ModuleManager,
) -> Option<u32> {
    if !workspace_visible || x < 0.0 {
        return None;
    }
    let left_width = group_width(&config.left, modules, config.height as i32);
    config
        .workspaces
        .local_workspace_at_x(x - left_width as f64)
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
    let bar_height = config.height as i32;

    hit_group(x, left_x, &config.left, modules, bar_height)
        .or_else(|| hit_group(x, center_x, &config.center, modules, bar_height))
        .or_else(|| hit_group(x, right_x, &config.right, modules, bar_height))
}

pub(super) fn group_origins(
    width: u32,
    workspace_width: i32,
    config: &BarConfig,
    modules: &ModuleManager,
) -> (i32, i32, i32) {
    let bar_height = config.height as i32;
    let left_width = group_width(&config.left, modules, bar_height);
    let center_width = group_width(&config.center, modules, bar_height);
    let right_width = group_width(&config.right, modules, bar_height);

    let left_x = 0;
    let left_end = left_width.saturating_add(workspace_width);
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

pub(super) fn hit_group<'a>(
    x: f64,
    start_x: i32,
    names: &'a [String],
    modules: &ModuleManager,
    bar_height: i32,
) -> Option<ModuleHit<'a>> {
    let mut cursor = start_x;
    for name in names {
        let Some(view) = modules.view(name) else {
            continue;
        };
        let width = module_width(&view, bar_height);
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

pub(super) fn group_width(names: &[String], modules: &ModuleManager, bar_height: i32) -> i32 {
    names
        .iter()
        .filter_map(|name| modules.view(name))
        .map(|view| module_width(&view, bar_height))
        .fold(0_i32, i32::saturating_add)
}

#[cfg(mhypr_module = "cpu")]
pub(super) fn cpu_icon_slot_size(
    cpu: &crate::modules::cpu::CpuVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> i32 {
    let available = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(1);
    ((available as f32 * cpu.icon_scale).round() as i32).clamp(1, available)
}

#[cfg(mhypr_module = "audio")]
pub(super) fn audio_icon_slot_size(
    audio: &crate::modules::audio::AudioVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> i32 {
    let available = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(1);
    ((available as f32 * audio.icon_scale).round() as i32).clamp(1, available)
}

#[cfg(mhypr_module = "brightness")]
pub(super) fn brightness_icon_slot_size(
    brightness: &crate::modules::brightness::BrightnessVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> i32 {
    let available = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(1);
    ((available as f32 * brightness.icon_scale).round() as i32).clamp(1, available)
}

#[cfg(mhypr_module = "layout")]
pub(super) fn layout_icon_slot_size(
    layout: &crate::modules::layout::LayoutVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> i32 {
    let available = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(1);
    ((available as f32 * layout.icon_scale).round() as i32).clamp(1, available)
}

#[cfg(mhypr_module = "disk")]
pub(super) fn disk_icon_slot_size(
    disk: &crate::modules::disk::DiskVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> i32 {
    let available = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(1);
    ((available as f32 * disk.icon_scale).round() as i32).clamp(1, available)
}

#[cfg(mhypr_module = "gpu")]
pub(super) fn gpu_icon_dimensions(
    gpu: &crate::modules::gpu::GpuVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> (i32, i32) {
    let available_h = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(1);
    let draw_h = ((available_h as f32 * gpu.icon_scale).round() as i32).clamp(1, available_h);
    let draw_w = if gpu.icon_height > 0 {
        ((draw_h as i64 * gpu.icon_width as i64 + gpu.icon_height as i64 / 2)
            / gpu.icon_height as i64)
            .max(1)
            .min(i32::MAX as i64) as i32
    } else {
        draw_h
    };
    (draw_w, draw_h)
}

#[cfg(mhypr_module = "monitor")]
pub(super) fn monitor_icon_slot_size(
    monitor: &crate::modules::monitor::MonitorVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> i32 {
    let available = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(1);
    ((available as f32 * monitor.icon_scale).round() as i32).clamp(1, available)
}

#[cfg(mhypr_module = "memory")]
pub(super) fn memory_icon_slot_size(
    memory: &crate::modules::memory::MemoryVisual,
    style: &ModuleStyle,
    bar_height: i32,
) -> i32 {
    let content_h = bar_height
        .saturating_sub(style.padding_y.max(0).saturating_mul(2))
        .max(2);
    let gap = memory.row_gap.min(content_h.saturating_sub(2)).max(0);
    let row_h = ((content_h - gap) / 2).max(1);
    ((row_h as f32 * memory.icon_scale).round() as i32).clamp(1, row_h)
}

pub(super) fn module_width(view: &ModuleView<'_>, bar_height: i32) -> i32 {
    if let Some(width) = view.width_override {
        return width.max(0);
    }

    #[cfg(mhypr_module = "active_window")]
    if let ModuleVisual::ActiveWindow(active_window) = &view.visual {
        let style = view.style;
        let text_width = if view.text.is_empty() {
            0
        } else {
            estimate_text_width(view.text, style)
        };
        let icon_width = active_window
            .icon
            .as_ref()
            .map(|_| active_window.icon_size.max(1))
            .unwrap_or(0);
        if icon_width == 0 && text_width == 0 {
            return 0;
        }
        let gap = if icon_width > 0 && text_width > 0 {
            active_window.icon_gap
        } else {
            0
        };
        let content = icon_width.saturating_add(gap).saturating_add(text_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "audio")]
    if let ModuleVisual::Audio(audio) = &view.visual {
        let style = view.style;
        let percent_width = estimate_text_width("150%", style);
        let icon_slot = audio_icon_slot_size(audio, style, bar_height);
        let content = icon_slot
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
        let icon_slot = brightness_icon_slot_size(brightness, style, bar_height);
        let content = icon_slot
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
        let content = if layout.icon_pixels.is_some() {
            layout_icon_slot_size(layout, style, bar_height)
        } else {
            estimate_text_width(&layout.name, style)
        };
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
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
        let icon_slot = cpu_icon_slot_size(cpu, style, bar_height);
        let following = graph_width.saturating_add(gap).saturating_add(text_width);
        let icon_gap = if following > 0 { cpu.icon_gap } else { 0 };
        let content = icon_slot.saturating_add(icon_gap).saturating_add(following);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "gpu")]
    if let ModuleVisual::Gpu(gpu) = &view.visual {
        let style = view.style;
        let (icon_width, _) = gpu_icon_dimensions(gpu, style, bar_height);
        let percent_width = estimate_text_width("100%", style);
        let content = icon_width
            .saturating_add(gpu.icon_gap)
            .saturating_add(gpu.bar_width)
            .saturating_add(gpu.text_gap)
            .saturating_add(percent_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "monitor")]
    if let ModuleVisual::Monitor(monitor) = &view.visual {
        let style = view.style;
        let icon_slot = monitor_icon_slot_size(monitor, style, bar_height);
        let content = icon_slot
            .saturating_add(monitor.icon_gap)
            .saturating_add(monitor.topology_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "disk")]
    if let ModuleVisual::Disk(disk) = &view.visual {
        let style = view.style;
        let icon_slot = disk_icon_slot_size(disk, style, bar_height);
        let content = icon_slot
            .saturating_add(disk.icon_gap)
            .saturating_add(disk.bar_width);
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "menu")]
    if let ModuleVisual::Menu(menu) = &view.visual {
        let style = view.style;
        let content_h = bar_height
            .saturating_sub(style.padding_y.max(0).saturating_mul(2))
            .max(1);
        let icon_size = ((content_h as f32 * menu.icon_scale).round() as i32)
            .clamp(1, content_h);
        return style
            .min_width
            .max(icon_size.saturating_add(style.padding_x.max(0).saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "network")]
    if let ModuleVisual::Network(_) = &view.visual {
        let style = view.style;
        let mut row_style = style.clone();
        row_style.font_size = (style.font_size - 1.0).max(8.0);
        // Reserve enough space for the widest rate representation up front.
        // Runtime B/s, K/s, M/s and G/s changes must not resize the whole
        // right-side module group and make neighboring modules jump.
        let content = estimate_text_width("↓ 999.9G/s", &row_style)
            .max(estimate_text_width("↑ 999.9G/s", &row_style));
        return style
            .min_width
            .max(content.saturating_add(style.padding_x.saturating_mul(2)))
            .max(1);
    }

    #[cfg(mhypr_module = "memory")]
    if let ModuleVisual::Memory(memory) = &view.visual {
        let style = view.style;
        let icon_slot = memory_icon_slot_size(memory, style, bar_height);
        let percent_width = estimate_text_width("100%", style);
        let content = icon_slot
            .saturating_add(memory.icon_gap)
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

pub(super) fn estimate_text_width(text: &str, style: &ModuleStyle) -> i32 {
    let em = text
        .chars()
        .map(|ch| if ch.is_ascii() { 0.62_f32 } else { 1.0_f32 })
        .sum::<f32>();
    (em * style.font_size).ceil() as i32
}
