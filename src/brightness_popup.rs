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
    #[serde(default = "default_min_percent")]
    min_percent: u32,
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

#[derive(Clone, Copy, Debug)]
pub struct BrightnessRowGeometry {
    pub row_y: i32,
    pub row_h: i32,
    pub slider_x: i32,
    pub slider_y: i32,
    pub slider_w: i32,
    pub slider_h: i32,
}

pub struct BrightnessPopupModel {
    pub config: BrightnessPopupConfig,
    pub devices: Vec<BrightnessDeviceRow>,
    pub min_percent: u32,
    requested_device: String,
}

impl BrightnessPopupModel {
    pub fn new() -> Result<Self> {
        let file: BrightnessPopupFile = config::load_module("brightness")?;
        file.popup.validate()?;
        ensure!(
            file.min_percent <= 100,
            "brightness min_percent must be <= 100"
        );
        let mut model = Self {
            config: file.popup,
            devices: Vec::new(),
            min_percent: file.min_percent,
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

    pub fn row_geometry(&self, index: usize) -> Option<BrightnessRowGeometry> {
        if index >= self.devices.len() {
            return None;
        }
        let row_y = self
            .config
            .padding
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(self.config.row_height.saturating_mul(index as i32));
        let row_h = self.config.row_height;
        let inset = 8;
        let slider_h = 5;
        let slider_x = self.config.padding.saturating_add(inset);
        let slider_w = self
            .config
            .width
            .saturating_sub(self.config.padding.saturating_mul(2))
            .saturating_sub(inset.saturating_mul(2))
            .max(1);
        let slider_y = row_y + row_h - slider_h - 6;
        Some(BrightnessRowGeometry {
            row_y,
            row_h,
            slider_x,
            slider_y,
            slider_w,
            slider_h,
        })
    }

    pub fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        if x < self.config.padding as f64 || x >= (self.config.width - self.config.padding) as f64 {
            return None;
        }
        let rows_y = self.config.padding + self.config.title_height + 1;
        let local_y = y - rows_y as f64;
        if local_y < 0.0 {
            return None;
        }
        let index = (local_y / self.config.row_height as f64).floor() as usize;
        (index < self.devices.len()).then_some(index)
    }

    pub fn brightness_at(&self, x: f64, y: f64) -> Option<(usize, u32)> {
        let index = self.row_at(x, y)?;
        let g = self.row_geometry(index)?;
        let hit_top = g.slider_y.saturating_sub(8);
        let hit_bottom = g.slider_y.saturating_add(g.slider_h).saturating_add(8);
        if y < hit_top as f64 || y >= hit_bottom as f64 {
            return None;
        }
        if x < g.slider_x as f64 || x > (g.slider_x + g.slider_w) as f64 {
            return None;
        }
        Some((index, self.percent_for_x(index, x)?))
    }

    pub fn percent_for_x(&self, index: usize, x: f64) -> Option<u32> {
        let g = self.row_geometry(index)?;
        let ratio = ((x - g.slider_x as f64) / g.slider_w.max(1) as f64).clamp(0.0, 1.0);
        let span = 100_u32.saturating_sub(self.min_percent);
        Some(self.min_percent + (ratio * span as f64).round() as u32)
    }

    pub fn set_percent(&mut self, index: usize, percent: u32) -> Result<bool> {
        let percent = percent.clamp(self.min_percent, 100);
        let (name, max, current_percent) = self
            .devices
            .get(index)
            .map(|device| (device.name.clone(), device.max, device.percent))
            .ok_or_else(|| anyhow::anyhow!("brightness device row {index} is unavailable"))?;
        if current_percent == percent {
            return Ok(false);
        }

        let raw = ((max as u128 * percent as u128 + 50) / 100).clamp(1, max as u128) as u64;
        let root = Path::new("/sys/class/backlight").join(&name);
        fs::write(root.join("brightness"), raw.to_string())
            .with_context(|| format!("failed to set brightness for {name}"))?;
        if let Some(device) = self.devices.get_mut(index) {
            device.current = raw;
            device.percent = percent;
        }
        Ok(true)
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
fn default_min_percent() -> u32 {
    5
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

#[cfg(test)]
mod tests {
    use super::{BrightnessDeviceRow, BrightnessPopupConfig, BrightnessPopupModel};

    fn interactive_model() -> BrightnessPopupModel {
        BrightnessPopupModel {
            config: BrightnessPopupConfig::default(),
            devices: vec![BrightnessDeviceRow {
                name: "test_backlight".into(),
                kind: "raw".into(),
                current: 50,
                max: 100,
                percent: 50,
                active: true,
            }],
            min_percent: 5,
            requested_device: "auto".into(),
        }
    }

    #[test]
    fn popup_slider_maps_minimum_to_full_range() {
        let model = interactive_model();
        let geometry = model.row_geometry(0).unwrap();
        let y = (geometry.slider_y + geometry.slider_h / 2) as f64;

        assert_eq!(
            model.brightness_at(geometry.slider_x as f64, y),
            Some((0, 5))
        );
        assert_eq!(
            model.brightness_at((geometry.slider_x + geometry.slider_w) as f64, y),
            Some((0, 100))
        );
    }
}
