use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "brightness";
pub const CONFIG_FILE: &str = "modules/brightness.toml";

#[derive(Debug, Deserialize)]
struct BrightnessConfig {
    #[serde(default = "default_device")]
    device: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_step_percent")]
    step_percent: u32,
    #[serde(default = "default_min_percent")]
    min_percent: u32,
    #[serde(default = "default_icon_size")]
    icon_size: i32,
    #[serde(default = "default_bar_width")]
    bar_width: i32,
    #[serde(default = "default_bar_height")]
    bar_height: i32,
    #[serde(default = "default_text_gap")]
    text_gap: i32,
    #[serde(default = "default_bar_background")]
    bar_background: String,
    #[serde(default = "default_fill")]
    fill: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BrightnessState {
    current: u64,
    max: u64,
    percent: u32,
}

#[derive(Clone)]
pub struct BrightnessVisual {
    pub percent: u32,
    pub icon_size: i32,
    pub bar_width: i32,
    pub bar_height: i32,
    pub text_gap: i32,
    pub bar_background: [u8; 4],
    pub fill: [u8; 4],
}

pub struct BrightnessModule {
    config: BrightnessConfig,
    root: PathBuf,
    state: BrightnessState,
    bar_background: [u8; 4],
    fill: [u8; 4],
    revision: u64,
}

impl BrightnessModule {
    pub fn load() -> Result<Self> {
        let config: BrightnessConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let root = resolve_backlight_root(&config.device)?;
        let state = read_state(&root)?;
        let bar_background = config::parse_rgba(&config.bar_background)?;
        let fill = config::parse_rgba(&config.fill)?;

        Ok(Self {
            config,
            root,
            state,
            bar_background,
            fill,
            revision: 0,
        })
    }

    fn adjust(&mut self, direction: i32) -> Result<bool> {
        if direction == 0 {
            return Ok(false);
        }
        let delta = self.config.step_percent as i32 * direction.signum();
        let next =
            (self.state.percent as i32 + delta).clamp(self.config.min_percent as i32, 100) as u32;
        set_percent(&self.root, next)?;
        self.state = read_state(&self.root)?;
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }
}

impl StatusModule for BrightnessModule {
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
        let next = read_state(&self.root)?;
        if next != self.state {
            self.state = next;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(String::new())
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Brightness(BrightnessVisual {
            percent: self.state.percent,
            icon_size: self.config.icon_size,
            bar_width: self.config.bar_width,
            bar_height: self.config.bar_height,
            text_gap: self.config.text_gap,
            bar_background: self.bar_background,
            fill: self.fill,
        })
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn scroll(&mut self, direction: i32) -> Result<bool> {
        self.adjust(direction)
    }
}

fn validate_config(config: &BrightnessConfig) -> Result<()> {
    ensure!(
        config.interval_ms > 0,
        "brightness interval_ms must be greater than zero"
    );
    ensure!(
        !config.device.trim().is_empty(),
        "brightness device must not be empty"
    );
    ensure!(
        config.step_percent > 0 && config.step_percent <= 100,
        "brightness step_percent must be in 1..=100"
    );
    ensure!(
        config.min_percent <= 100,
        "brightness min_percent must be <= 100"
    );
    ensure!(
        config.icon_size > 4 && config.bar_width > 0 && config.bar_height > 0,
        "brightness visual dimensions must be positive"
    );
    ensure!(
        config.text_gap >= 0,
        "brightness text_gap must not be negative"
    );
    config.style.validate()?;
    Ok(())
}

fn resolve_backlight_root(device: &str) -> Result<PathBuf> {
    let base = Path::new("/sys/class/backlight");
    if device != "auto" {
        let root = base.join(device);
        ensure!(
            root.join("brightness").exists() && root.join("max_brightness").exists(),
            "brightness device {device} is unavailable"
        );
        return Ok(root);
    }

    let mut candidates = fs::read_dir(base)
        .context("failed to enumerate /sys/class/backlight")?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|root| root.join("brightness").exists() && root.join("max_brightness").exists())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .context("no backlight device found")
}

fn read_state(root: &Path) -> Result<BrightnessState> {
    let current = read_u64(root.join("brightness"))?;
    let max = read_u64(root.join("max_brightness"))?;
    ensure!(max > 0, "backlight max_brightness is zero");
    let percent = ((current as f64 / max as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u32;
    Ok(BrightnessState {
        current,
        max,
        percent,
    })
}

fn set_percent(root: &Path, percent: u32) -> Result<()> {
    let max = read_u64(root.join("max_brightness"))?;
    ensure!(max > 0, "backlight max_brightness is zero");
    let value = ((max as u128 * percent as u128 + 50) / 100).clamp(1, max as u128) as u64;
    fs::write(root.join("brightness"), value.to_string())
        .context("failed to write backlight brightness")
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

fn default_interval_ms() -> u64 {
    750
}

fn default_step_percent() -> u32 {
    5
}

fn default_min_percent() -> u32 {
    5
}

fn default_icon_size() -> i32 {
    14
}

fn default_bar_width() -> i32 {
    32
}

fn default_bar_height() -> i32 {
    4
}

fn default_text_gap() -> i32 {
    4
}

fn default_bar_background() -> String {
    "#303030".into()
}

fn default_fill() -> String {
    "#FFD84A".into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_math_is_bounded() {
        let max = 120_000_u64;
        let current = 60_000_u64;
        let percent = ((current as f64 / max as f64) * 100.0).round() as u32;
        assert_eq!(percent, 50);
    }
}
