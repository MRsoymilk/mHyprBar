use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct BrightnessPopupConfig {
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
    #[serde(default = "default_refresh_interval_ms")]
    refresh_interval_ms: u64,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_active_background")]
    pub active_background: String,
    #[serde(default = "default_bar_background")]
    pub bar_background: String,
    #[serde(default = "default_bar_fill")]
    pub bar_fill: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct BrightnessPopupFile {
    #[serde(default = "default_device")]
    device: String,
    #[serde(default)]
    popup: BrightnessPopupConfig,
}

impl Default for BrightnessPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            title_height: default_title_height(),
            row_height: default_row_height(),
            refresh_interval_ms: default_refresh_interval_ms(),
            border: default_border(),
            separator: default_separator(),
            active_background: default_active_background(),
            bar_background: default_bar_background(),
            bar_fill: default_bar_fill(),
            style: default_popup_style(),
        }
    }
}

impl BrightnessPopupConfig {
    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "brightness popup width must be positive");
        ensure!(
            self.padding >= 0,
            "brightness popup padding must not be negative"
        );
        ensure!(
            self.title_height > 0,
            "brightness popup title_height must be positive"
        );
        ensure!(
            self.row_height > 0,
            "brightness popup row_height must be positive"
        );
        ensure!(
            self.refresh_interval_ms > 0,
            "brightness popup refresh_interval_ms must be positive"
        );
        self.style.validate()?;
        let _ = self.border_rgba()?;
        let _ = self.separator_rgba()?;
        let _ = self.active_background_rgba()?;
        let _ = self.bar_background_rgba()?;
        let _ = self.bar_fill_rgba()?;
        Ok(())
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_interval_ms)
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn separator_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.separator)
    }

    pub fn active_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.active_background)
    }

    pub fn bar_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_background)
    }

    pub fn bar_fill_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_fill)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrightnessDeviceRow {
    pub name: String,
    pub kind: String,
    pub current: u64,
    pub max: u64,
    pub percent: u32,
    pub active: bool,
}

pub struct BrightnessPopupModel {
    pub config: BrightnessPopupConfig,
    pub devices: Vec<BrightnessDeviceRow>,
    requested_device: String,
}

impl BrightnessPopupModel {
    pub fn new() -> Result<Self> {
        let file: BrightnessPopupFile = config::load_module("brightness")?;
        file.popup.validate()?;
        let mut model = Self {
            config: file.popup,
            devices: Vec::new(),
            requested_device: file.device,
        };
        model.refresh()?;
        Ok(model)
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.devices = read_devices(&self.requested_device)?;
        Ok(())
    }

    pub fn panel_height(&self) -> i32 {
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(
                self.config
                    .row_height
                    .saturating_mul(self.devices.len().max(1) as i32),
            )
    }
}

fn read_devices(requested_device: &str) -> Result<Vec<BrightnessDeviceRow>> {
    let base = Path::new("/sys/class/backlight");
    let mut roots = fs::read_dir(base)
        .context("failed to enumerate /sys/class/backlight")?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|root| root.join("brightness").exists() && root.join("max_brightness").exists())
        .collect::<Vec<_>>();
    roots.sort();

    let active_name = if requested_device == "auto" {
        roots
            .first()
            .and_then(|root| root.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned()
    } else {
        requested_device.to_owned()
    };

    let mut devices = Vec::new();
    for root in roots {
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("backlight")
            .to_owned();
        let current = read_u64(root.join("brightness"))?;
        let max = read_u64(root.join("max_brightness"))?;
        if max == 0 {
            continue;
        }
        let percent = ((current as f64 / max as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u32;
        let kind = fs::read_to_string(root.join("type"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        devices.push(BrightnessDeviceRow {
            active: name == active_name,
            name,
            kind,
            current,
            max,
            percent,
        });
    }
    Ok(devices)
}

fn read_u64(path: PathBuf) -> Result<u64> {
    fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .trim()
        .parse::<u64>()
        .with_context(|| format!("invalid numeric value in {}", path.display()))
}

fn default_device() -> String {
    "auto".into()
}
fn default_enabled() -> bool {
    true
}
fn default_width() -> i32 {
    520
}
fn default_padding() -> i32 {
    10
}
fn default_title_height() -> i32 {
    30
}
fn default_row_height() -> i32 {
    54
}
fn default_refresh_interval_ms() -> u64 {
    1000
}
fn default_border() -> String {
    "#3C414A".into()
}
fn default_separator() -> String {
    "#353A42".into()
}
fn default_active_background() -> String {
    "#3B3520".into()
}
fn default_bar_background() -> String {
    "#30343A".into()
}
fn default_bar_fill() -> String {
    "#FFD84A".into()
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
