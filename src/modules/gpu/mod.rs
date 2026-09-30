pub mod backend;
pub mod popup;

use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use self::backend::{GpuBackend, GpuStats, create_backend};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "gpu";
pub const CONFIG_FILE: &str = "modules/gpu.toml";

const ICON_GPU_SVG: &[u8] = include_bytes!("../../../res/gpu/gpu.svg");

#[derive(Debug, Deserialize)]
struct GpuConfig {
    #[serde(default = "default_backend")]
    backend: String,
    #[serde(default = "default_device")]
    device: String,
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
    #[serde(default = "default_utilization_fill")]
    utilization_fill: String,
    #[serde(default = "default_memory_fill")]
    memory_fill: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone)]
struct GpuIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone)]
pub struct GpuVisual {
    pub backend_name: String,
    pub vendor: String,
    pub name: String,
    pub utilization_percent: Option<f32>,
    pub memory_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub temperature_c: Option<f32>,
    pub power_w: Option<f32>,
    pub icon_scale: f32,
    pub icon_gap: i32,
    pub icon_pixels: Arc<[u8]>,
    pub icon_width: i32,
    pub icon_height: i32,
    pub bar_width: i32,
    pub bar_height: i32,
    pub row_gap: i32,
    pub text_gap: i32,
    pub bar_background: [u8; 4],
    pub utilization_fill: [u8; 4],
    pub memory_fill: [u8; 4],
}

pub struct GpuModule {
    config: GpuConfig,
    backend: Box<dyn GpuBackend>,
    stats: GpuStats,
    icon: GpuIcon,
    bar_background: [u8; 4],
    utilization_fill: [u8; 4],
    memory_fill: [u8; 4],
    revision: u64,
}

impl GpuModule {
    pub fn load() -> Result<Self> {
        let config: GpuConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let _ = crate::modules::gpu::popup::GpuPopupConfig::load()?;

        let icon = rasterize_svg("gpu.svg", ICON_GPU_SVG)?;
        let mut backend = create_backend(&config.backend, &config.device)?;
        let stats = backend.sample()?;
        eprintln!(
            "mhyprbar: gpu backend={} vendor={} device={} temp={} power={}",
            backend.backend_name(),
            stats.vendor.label(),
            stats.name,
            stats
                .temperature_c
                .map(|value| format!("{value:.0}C"))
                .unwrap_or_else(|| "--".into()),
            stats
                .power_w
                .map(|value| format!("{value:.1}W"))
                .unwrap_or_else(|| "--".into())
        );

        Ok(Self {
            bar_background: config::parse_rgba(&config.bar_background)?,
            utilization_fill: config::parse_rgba(&config.utilization_fill)?,
            memory_fill: config::parse_rgba(&config.memory_fill)?,
            config,
            backend,
            stats,
            icon,
            revision: 0,
        })
    }
}

impl StatusModule for GpuModule {
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
        self.stats = self.backend.sample()?;
        self.revision = self.revision.wrapping_add(1);
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn gpu_processes(&mut self, limit: usize) -> Result<Vec<crate::modules::gpu::backend::GpuProcess>> {
        self.backend.processes(limit)
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Gpu(GpuVisual {
            backend_name: self.backend.backend_name().to_owned(),
            vendor: self.stats.vendor.label().to_owned(),
            name: self.stats.name.clone(),
            utilization_percent: self.stats.utilization_percent,
            memory_percent: self.stats.memory_percent(),
            memory_used_bytes: self.stats.memory_used_bytes,
            memory_total_bytes: self.stats.memory_total_bytes,
            temperature_c: self.stats.temperature_c,
            power_w: self.stats.power_w,
            icon_scale: self.config.icon_scale,
            icon_gap: self.config.icon_gap,
            icon_pixels: Arc::clone(&self.icon.pixels),
            icon_width: self.icon.width,
            icon_height: self.icon.height,
            bar_width: self.config.bar_width,
            bar_height: self.config.bar_height,
            row_gap: self.config.row_gap,
            text_gap: self.config.text_gap,
            bar_background: self.bar_background,
            utilization_fill: self.utilization_fill,
            memory_fill: self.memory_fill,
        })
    }
}

fn rasterize_svg(name: &str, svg_bytes: &[u8]) -> Result<GpuIcon> {
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
        "gpu SVG rasterization failed for {name}"
    );

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "gpu SVG rasterizer returned {} bytes for {RASTER_SIZE}x{RASTER_SIZE}, expected {expected}: {name}",
        output.stdout.len()
    );

    crop_transparent_margin(name, &output.stdout, RASTER_SIZE, RASTER_SIZE)
}

fn crop_transparent_margin(name: &str, pixels: &[u8], width: i32, height: i32) -> Result<GpuIcon> {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = -1;
    let mut max_y = -1;

    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            if pixels[offset + 3] == 0 {
                continue;
            }
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    ensure!(
        max_x >= min_x && max_y >= min_y,
        "gpu SVG has no visible pixels: {name}"
    );

    let cropped_width = max_x - min_x + 1;
    let cropped_height = max_y - min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);

    for y in min_y..=max_y {
        let start = ((y * width + min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&pixels[start..end]);
    }

    Ok(GpuIcon {
        pixels: Arc::from(cropped),
        width: cropped_width,
        height: cropped_height,
    })
}

fn validate_config(config: &GpuConfig) -> Result<()> {
    ensure!(
        config.interval_ms > 0,
        "gpu interval_ms must be greater than zero"
    );
    ensure!(
        config.icon_scale.is_finite() && (0.1..=1.0).contains(&config.icon_scale),
        "gpu icon_scale must be in 0.1..=1.0"
    );
    ensure!(config.icon_gap >= 0, "gpu icon_gap must not be negative");
    ensure!(
        config.bar_width > 0,
        "gpu bar_width must be greater than zero"
    );
    ensure!(
        config.bar_height > 0,
        "gpu bar_height must be greater than zero"
    );
    ensure!(config.row_gap >= 0, "gpu row_gap must not be negative");
    ensure!(config.text_gap >= 0, "gpu text_gap must not be negative");
    config.style.validate()?;
    Ok(())
}

fn default_backend() -> String {
    "auto".into()
}

fn default_device() -> String {
    "auto".into()
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_icon_scale() -> f32 {
    0.8
}

fn default_icon_gap() -> i32 {
    4
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

fn default_utilization_fill() -> String {
    "#76B900".into()
}

fn default_memory_fill() -> String {
    "#4EA1FF".into()
}

#[cfg(test)]
mod tests {
    use crate::modules::gpu::backend::GpuVendor;

    #[test]
    fn vendor_labels_are_stable() {
        assert_eq!(GpuVendor::Nvidia.label(), "NVIDIA");
        assert_eq!(GpuVendor::Amd.label(), "AMD");
        assert_eq!(GpuVendor::Intel.label(), "Intel");
    }
}
