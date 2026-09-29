use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::{
    config::{self, ModuleStyle},
    gpu::{GpuBackend, GpuStats, create_backend},
};

pub const NAME: &str = "gpu";
pub const CONFIG_FILE: &str = "modules/gpu.toml";

#[derive(Debug, Deserialize)]
struct GpuConfig {
    #[serde(default = "default_backend")]
    backend: String,
    #[serde(default = "default_device")]
    device: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
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
    bar_background: [u8; 4],
    utilization_fill: [u8; 4],
    memory_fill: [u8; 4],
    revision: u64,
}

impl GpuModule {
    pub fn load() -> Result<Self> {
        let config: GpuConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let _ = crate::gpu_popup::GpuPopupConfig::load()?;

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

    fn gpu_processes(&mut self, limit: usize) -> Result<Vec<crate::gpu::GpuProcess>> {
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

fn validate_config(config: &GpuConfig) -> Result<()> {
    ensure!(
        config.interval_ms > 0,
        "gpu interval_ms must be greater than zero"
    );
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
    use crate::gpu::GpuVendor;

    #[test]
    fn vendor_labels_are_stable() {
        assert_eq!(GpuVendor::Nvidia.label(), "NVIDIA");
        assert_eq!(GpuVendor::Amd.label(), "AMD");
        assert_eq!(GpuVendor::Intel.label(), "Intel");
    }
}
