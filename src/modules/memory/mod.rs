pub mod popup;

use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "memory";
pub const CONFIG_FILE: &str = "modules/memory.toml";

const ICON_MEMORY_SVG: &[u8] = include_bytes!("../../../res/memory/memory.svg");
const ICON_SWAP_SVG: &[u8] = include_bytes!("../../../res/memory/swap.svg");

#[derive(Debug, Deserialize)]
struct MemoryConfig {
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_icon_scale")]
    icon_scale: f32,
    #[serde(default = "default_icon_gap")]
    icon_gap: i32,
    #[serde(default = "default_bar_width")]
    bar_width: i32,
    #[serde(default = "default_bar_height")]
    bar_height: i32,
    #[serde(default = "default_row_gap")]
    row_gap: i32,
    #[serde(default = "default_text_gap")]
    text_gap: i32,
    #[serde(default = "default_bar_background")]
    bar_background: String,
    #[serde(default = "default_memory_fill")]
    memory_fill: String,
    #[serde(default = "default_swap_fill")]
    swap_fill: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MemoryStats {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

impl MemoryStats {
    pub fn used_kib(self) -> u64 {
        self.total_kib.saturating_sub(self.available_kib)
    }

    pub fn swap_used_kib(self) -> u64 {
        self.swap_total_kib.saturating_sub(self.swap_free_kib)
    }

    pub fn memory_percent(self) -> f32 {
        percent(self.used_kib(), self.total_kib)
    }

    pub fn swap_percent(self) -> f32 {
        percent(self.swap_used_kib(), self.swap_total_kib)
    }
}

#[derive(Clone)]
struct MemoryIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone)]
struct MemoryIcons {
    memory: MemoryIcon,
    swap: MemoryIcon,
}

impl MemoryIcons {
    fn load() -> Result<Self> {
        let memory = rasterize_svg("memory.svg", ICON_MEMORY_SVG)?;
        let swap = rasterize_svg("swap.svg", ICON_SWAP_SVG)?;
        let bounds = shared_alpha_bounds(&[&memory, &swap])?;

        Ok(Self {
            memory: crop_to_bounds(&memory, bounds)?,
            swap: crop_to_bounds(&swap, bounds)?,
        })
    }
}

#[derive(Clone)]
pub struct MemoryVisual {
    pub memory_percent: f32,
    pub swap_percent: f32,
    pub icon_scale: f32,
    pub icon_gap: i32,
    pub memory_icon_pixels: Arc<[u8]>,
    pub swap_icon_pixels: Arc<[u8]>,
    pub icon_width: i32,
    pub icon_height: i32,
    pub bar_width: i32,
    pub bar_height: i32,
    pub row_gap: i32,
    pub text_gap: i32,
    pub bar_background: [u8; 4],
    pub memory_fill: [u8; 4],
    pub swap_fill: [u8; 4],
}

pub struct MemoryModule {
    config: MemoryConfig,
    stats: MemoryStats,
    icons: MemoryIcons,
    bar_background: [u8; 4],
    memory_fill: [u8; 4],
    swap_fill: [u8; 4],
    revision: u64,
}

impl MemoryModule {
    pub fn load() -> Result<Self> {
        let config: MemoryConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "memory interval_ms must be greater than zero"
        );
        ensure!(
            config.icon_scale.is_finite() && (0.1..=1.0).contains(&config.icon_scale),
            "memory icon_scale must be in 0.1..=1.0"
        );
        ensure!(config.icon_gap >= 0, "memory icon_gap must not be negative");
        ensure!(
            config.bar_width > 0,
            "memory bar_width must be greater than zero"
        );
        ensure!(
            config.bar_height > 0,
            "memory bar_height must be greater than zero"
        );
        ensure!(config.row_gap >= 0, "memory row_gap must not be negative");
        ensure!(config.text_gap >= 0, "memory text_gap must not be negative");
        config.style.validate()?;
        let _ = crate::modules::memory::popup::MemoryPopupConfig::load()?;

        let icons = MemoryIcons::load()?;
        let bar_background = config::parse_rgba(&config.bar_background)?;
        let memory_fill = config::parse_rgba(&config.memory_fill)?;
        let swap_fill = config::parse_rgba(&config.swap_fill)?;

        Ok(Self {
            config,
            stats: MemoryStats {
                total_kib: 0,
                available_kib: 0,
                swap_total_kib: 0,
                swap_free_kib: 0,
            },
            icons,
            bar_background,
            memory_fill,
            swap_fill,
            revision: 0,
        })
    }
}

impl StatusModule for MemoryModule {
    fn name(&self) -> &'static str {
        NAME
    }

    fn interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.config.interval_ms))
    }

    fn style(&self) -> &ModuleStyle {
        &self.config.style
    }

    fn sample(&mut self) -> Result<String> {
        self.stats = read_stats()?;
        self.revision = self.revision.wrapping_add(1);
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Memory(MemoryVisual {
            memory_percent: self.stats.memory_percent(),
            swap_percent: self.stats.swap_percent(),
            icon_scale: self.config.icon_scale,
            icon_gap: self.config.icon_gap,
            memory_icon_pixels: Arc::clone(&self.icons.memory.pixels),
            swap_icon_pixels: Arc::clone(&self.icons.swap.pixels),
            icon_width: self.icons.memory.width,
            icon_height: self.icons.memory.height,
            bar_width: self.config.bar_width,
            bar_height: self.config.bar_height,
            row_gap: self.config.row_gap,
            text_gap: self.config.text_gap,
            bar_background: self.bar_background,
            memory_fill: self.memory_fill,
            swap_fill: self.swap_fill,
        })
    }
}

pub(crate) fn read_stats() -> Result<MemoryStats> {
    let content = fs::read_to_string("/proc/meminfo").context("failed to read /proc/meminfo")?;
    let mut total = None;
    let mut available = None;
    let mut swap_total = None;
    let mut swap_free = None;

    for line in content.lines() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("MemTotal:") => total = parse_value(fields.next()),
            Some("MemAvailable:") => available = parse_value(fields.next()),
            Some("SwapTotal:") => swap_total = parse_value(fields.next()),
            Some("SwapFree:") => swap_free = parse_value(fields.next()),
            _ => {}
        }
    }

    let stats = MemoryStats {
        total_kib: total.context("MemTotal missing from /proc/meminfo")?,
        available_kib: available.context("MemAvailable missing from /proc/meminfo")?,
        swap_total_kib: swap_total.unwrap_or(0),
        swap_free_kib: swap_free.unwrap_or(0),
    };
    ensure!(stats.total_kib > 0, "MemTotal must be greater than zero");
    Ok(stats)
}

fn parse_value(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.parse::<u64>().ok())
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (100.0 * used as f64 / total as f64) as f32
    }
}

#[derive(Clone, Copy, Debug)]
struct AlphaBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

fn rasterize_svg(name: &str, svg_bytes: &[u8]) -> Result<MemoryIcon> {
    const RASTER_SIZE: i32 = 96;

    let mut svg = Command::new("rsvg-convert")
        .arg("--width")
        .arg(RASTER_SIZE.to_string())
        .arg("--height")
        .arg(RASTER_SIZE.to_string())
        .arg("--keep-aspect-ratio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch rsvg-convert for {name}"))?;

    let stdout = svg
        .stdout
        .take()
        .context("failed to capture rsvg-convert output")?;
    let mut svg_stdin = svg
        .stdin
        .take()
        .context("failed to open rsvg-convert stdin")?;

    let magick = Command::new("magick")
        .arg("png:-")
        .arg("rgba:-")
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch magick for {name}"))?;

    svg_stdin
        .write_all(svg_bytes)
        .with_context(|| format!("failed to feed SVG data for {name}"))?;
    drop(svg_stdin);

    let status = svg
        .wait()
        .with_context(|| format!("failed to wait for rsvg-convert for {name}"))?;
    let output = magick
        .wait_with_output()
        .with_context(|| format!("failed to wait for magick for {name}"))?;

    ensure!(
        status.success() && output.status.success(),
        "memory SVG rasterization failed for {name}"
    );

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "memory SVG rasterizer returned {} bytes for {RASTER_SIZE}x{RASTER_SIZE}, expected {expected}: {name}",
        output.stdout.len()
    );

    Ok(MemoryIcon {
        pixels: Arc::from(output.stdout),
        width: RASTER_SIZE,
        height: RASTER_SIZE,
    })
}

fn alpha_bounds(icon: &MemoryIcon) -> Option<AlphaBounds> {
    let mut min_x = icon.width;
    let mut min_y = icon.height;
    let mut max_x = -1;
    let mut max_y = -1;

    for y in 0..icon.height {
        for x in 0..icon.width {
            let offset = ((y * icon.width + x) * 4) as usize;
            if icon.pixels[offset + 3] == 0 {
                continue;
            }
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    (max_x >= min_x && max_y >= min_y).then_some(AlphaBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

fn shared_alpha_bounds(icons: &[&MemoryIcon]) -> Result<AlphaBounds> {
    ensure!(!icons.is_empty(), "memory SVG icon group must not be empty");

    let width = icons[0].width;
    let height = icons[0].height;
    let mut shared: Option<AlphaBounds> = None;

    for icon in icons {
        ensure!(
            icon.width == width && icon.height == height,
            "memory SVG icons must share the same raster canvas"
        );
        let bounds = alpha_bounds(icon).context("memory SVG icon has no visible pixels")?;
        shared = Some(match shared {
            None => bounds,
            Some(current) => AlphaBounds {
                min_x: current.min_x.min(bounds.min_x),
                min_y: current.min_y.min(bounds.min_y),
                max_x: current.max_x.max(bounds.max_x),
                max_y: current.max_y.max(bounds.max_y),
            },
        });
    }

    shared.context("memory SVG icon group has no visible pixels")
}

fn crop_to_bounds(icon: &MemoryIcon, bounds: AlphaBounds) -> Result<MemoryIcon> {
    ensure!(
        bounds.min_x >= 0
            && bounds.min_y >= 0
            && bounds.max_x < icon.width
            && bounds.max_y < icon.height,
        "memory SVG shared bounds are outside the raster canvas"
    );

    let cropped_width = bounds.max_x - bounds.min_x + 1;
    let cropped_height = bounds.max_y - bounds.min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);

    for y in bounds.min_y..=bounds.max_y {
        let start = ((y * icon.width + bounds.min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&icon.pixels[start..end]);
    }

    Ok(MemoryIcon {
        pixels: Arc::from(cropped),
        width: cropped_width,
        height: cropped_height,
    })
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_icon_scale() -> f32 {
    1.0
}

fn default_icon_gap() -> i32 {
    3
}

fn default_bar_width() -> i32 {
    42
}

fn default_bar_height() -> i32 {
    6
}

fn default_row_gap() -> i32 {
    2
}

fn default_text_gap() -> i32 {
    3
}

fn default_bar_background() -> String {
    "#303030".into()
}

fn default_memory_fill() -> String {
    "#4EA1FF".into()
}

fn default_swap_fill() -> String {
    "#A56BFF".into()
}

#[cfg(test)]
mod tests {
    use super::read_stats;

    #[test]
    fn reads_memory_and_swap_stats() {
        let stats = read_stats().expect("memory stats");
        assert!(stats.total_kib > 0);
        assert!(stats.available_kib <= stats.total_kib);
        assert!(stats.swap_free_kib <= stats.swap_total_kib || stats.swap_total_kib == 0);
    }
}
