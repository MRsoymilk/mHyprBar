use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::{GpuVisual, backend::GpuProcess};
use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct GpuPopupConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_padding")]
    pub padding: i32,
    #[serde(default = "default_title_height")]
    pub title_height: i32,
    #[serde(default = "default_row_height")]
    pub row_height: i32,
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_max_processes")]
    pub max_processes: usize,
    #[serde(default = "default_bar_width")]
    pub bar_width: i32,
    #[serde(default = "default_bar_background")]
    pub bar_background: String,
    #[serde(default = "default_utilization_fill")]
    pub utilization_fill: String,
    #[serde(default = "default_memory_fill")]
    pub memory_fill: String,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct GpuPopupFile {
    #[serde(default)]
    popup: GpuPopupConfig,
}

impl Default for GpuPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            title_height: default_title_height(),
            row_height: default_row_height(),
            refresh_ms: default_refresh_ms(),
            max_processes: default_max_processes(),
            bar_width: default_bar_width(),
            bar_background: default_bar_background(),
            utilization_fill: default_utilization_fill(),
            memory_fill: default_memory_fill(),
            border: default_border(),
            separator: default_separator(),
            style: default_popup_style(),
        }
    }
}

impl GpuPopupConfig {
    pub fn load() -> Result<Self> {
        let file: GpuPopupFile = config::load_module("gpu")?;
        let cfg = file.popup;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "gpu popup width must be greater than zero");
        ensure!(self.padding >= 0, "gpu popup padding must not be negative");
        ensure!(
            self.title_height > 0,
            "gpu popup title_height must be greater than zero"
        );
        ensure!(
            self.row_height > 0,
            "gpu popup row_height must be greater than zero"
        );
        ensure!(
            self.refresh_ms > 0,
            "gpu popup refresh_ms must be greater than zero"
        );
        ensure!(
            self.max_processes > 0,
            "gpu popup max_processes must be greater than zero"
        );
        ensure!(
            self.bar_width > 0,
            "gpu popup bar_width must be greater than zero"
        );
        self.style.validate()?;
        let _ = self.bar_background_rgba()?;
        let _ = self.utilization_fill_rgba()?;
        let _ = self.memory_fill_rgba()?;
        let _ = self.border_rgba()?;
        let _ = self.separator_rgba()?;
        Ok(())
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_ms)
    }

    pub fn bar_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_background)
    }

    pub fn utilization_fill_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.utilization_fill)
    }

    pub fn memory_fill_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.memory_fill)
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn separator_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.separator)
    }
}

#[derive(Clone, Debug)]
pub struct GpuPopupSnapshot {
    pub backend_name: String,
    pub vendor: String,
    pub name: String,
    pub utilization_percent: Option<f32>,
    pub memory_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub temperature_c: Option<f32>,
    pub power_w: Option<f32>,
}

impl From<&GpuVisual> for GpuPopupSnapshot {
    fn from(visual: &GpuVisual) -> Self {
        Self {
            backend_name: visual.backend_name.clone(),
            vendor: visual.vendor.clone(),
            name: visual.name.clone(),
            utilization_percent: visual.utilization_percent,
            memory_percent: visual.memory_percent,
            memory_used_bytes: visual.memory_used_bytes,
            memory_total_bytes: visual.memory_total_bytes,
            temperature_c: visual.temperature_c,
            power_w: visual.power_w,
        }
    }
}

pub struct GpuPopupModel {
    pub config: GpuPopupConfig,
    pub snapshot: GpuPopupSnapshot,
    pub processes: Vec<GpuProcess>,
}

impl GpuPopupModel {
    pub fn new(config: GpuPopupConfig, visual: &GpuVisual, processes: Vec<GpuProcess>) -> Self {
        Self {
            config,
            snapshot: visual.into(),
            processes,
        }
    }

    pub fn update(&mut self, visual: &GpuVisual, processes: Vec<GpuProcess>) {
        self.snapshot = visual.into();
        self.processes = processes;
    }

    pub fn panel_height(&self) -> i32 {
        let process_rows = self.processes.len().max(1) as i32;
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(self.config.row_height.saturating_mul(5))
            .saturating_add(1)
            .saturating_add(self.config.row_height)
            .saturating_add(self.config.row_height.saturating_mul(process_rows))
    }

    pub fn backend_text(&self) -> String {
        format!("{} · {}", self.snapshot.vendor, self.snapshot.backend_name)
    }

    pub fn utilization_text(&self) -> String {
        format_percent(self.snapshot.utilization_percent)
    }

    pub fn memory_text(&self) -> String {
        match (
            self.snapshot.memory_used_bytes,
            self.snapshot.memory_total_bytes,
            self.snapshot.memory_percent,
        ) {
            (Some(used), Some(total), Some(percent)) if total > 0 => {
                format!(
                    "{} / {} · {percent:.0}%",
                    format_bytes(used),
                    format_bytes(total)
                )
            }
            (_, _, percent) => format_percent(percent),
        }
    }

    pub fn temperature_text(&self) -> String {
        self.snapshot
            .temperature_c
            .map(|value| format!("{value:.0} °C"))
            .unwrap_or_else(|| "--".into())
    }

    pub fn power_text(&self) -> String {
        self.snapshot
            .power_w
            .map(|value| format!("{value:.1} W"))
            .unwrap_or_else(|| "--".into())
    }

    pub fn process_gpu_text(process: &GpuProcess) -> String {
        format_percent(process.gpu_percent)
    }

    pub fn process_memory_percent_text(process: &GpuProcess) -> String {
        format_percent(process.memory_percent)
    }

    pub fn process_memory_text(process: &GpuProcess) -> String {
        process
            .memory_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "--".into())
    }
}

fn format_percent(value: Option<f32>) -> String {
    value
        .map(|value| format!("{:.0}%", value.clamp(0.0, 100.0)))
        .unwrap_or_else(|| "--%".into())
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else {
        format!("{:.0} MiB", bytes / MIB)
    }
}

fn default_enabled() -> bool {
    true
}

fn default_width() -> i32 {
    560
}

fn default_padding() -> i32 {
    12
}

fn default_title_height() -> i32 {
    34
}

fn default_row_height() -> i32 {
    30
}

fn default_refresh_ms() -> u64 {
    1_000
}

fn default_max_processes() -> usize {
    8
}

fn default_bar_width() -> i32 {
    140
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

fn default_border() -> String {
    "#3C414A".into()
}

fn default_separator() -> String {
    "#353A42".into()
}

fn default_popup_style() -> ModuleStyle {
    ModuleStyle {
        foreground: "#F2F2F2".into(),
        background: "#17191DEB".into(),
        font_family: "sans-serif".into(),
        font_size: 12.0,
        padding_x: 0,
        padding_y: 0,
        min_width: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{GpuPopupModel, format_bytes};
    use crate::modules::gpu::GpuVisual;

    fn visual() -> GpuVisual {
        GpuVisual {
            backend_name: "nvidia-smi".into(),
            vendor: "NVIDIA".into(),
            name: "NVIDIA GeForce GTX 1060".into(),
            utilization_percent: Some(25.0),
            memory_percent: Some(50.0),
            memory_used_bytes: Some(3 * 1024 * 1024 * 1024),
            memory_total_bytes: Some(6 * 1024 * 1024 * 1024),
            temperature_c: Some(52.0),
            power_w: Some(20.5),
            icon_scale: 0.8,
            icon_gap: 4,
            icon_pixels: std::sync::Arc::from(vec![255_u8; 4]),
            icon_width: 1,
            icon_height: 1,
            bar_width: 42,
            bar_height: 6,
            row_gap: 2,
            text_gap: 3,
            bar_background: [0; 4],
            utilization_fill: [0; 4],
            memory_fill: [0; 4],
        }
    }

    #[test]
    fn formats_gpu_popup_values() {
        let model = GpuPopupModel {
            config: Default::default(),
            snapshot: (&visual()).into(),
            processes: Vec::new(),
        };
        assert_eq!(model.backend_text(), "NVIDIA · nvidia-smi");
        assert_eq!(model.utilization_text(), "25%");
        assert_eq!(model.memory_text(), "3.0 GiB / 6.0 GiB · 50%");
        assert_eq!(model.temperature_text(), "52 °C");
        assert_eq!(model.power_text(), "20.5 W");
        assert!(model.panel_height() > 0);
        assert_eq!(format_bytes(512 * 1024 * 1024), "512 MiB");
    }
}
