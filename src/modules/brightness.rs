use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "brightness";
pub const CONFIG_FILE: &str = "modules/brightness.toml";

const ICON_LOW_SVG: &[u8] = include_bytes!("../../res/brightness/brightness-low.svg");
const ICON_HIGH_SVG: &[u8] = include_bytes!("../../res/brightness/brightness-high.svg");

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
    #[serde(default = "default_icon_scale")]
    icon_scale: f32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrightnessIconKind {
    Low,
    High,
}

#[derive(Clone)]
struct BrightnessIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone)]
struct BrightnessIcons {
    low: BrightnessIcon,
    high: BrightnessIcon,
}

impl BrightnessIcons {
    fn load() -> Result<Self> {
        Ok(Self {
            low: rasterize_svg("brightness-low.svg", ICON_LOW_SVG)?,
            high: rasterize_svg("brightness-high.svg", ICON_HIGH_SVG)?,
        })
    }

    fn icon(&self, kind: BrightnessIconKind) -> BrightnessIcon {
        match kind {
            BrightnessIconKind::Low => self.low.clone(),
            BrightnessIconKind::High => self.high.clone(),
        }
    }
}

#[derive(Clone)]
pub struct BrightnessVisual {
    pub percent: u32,
    pub icon_scale: f32,
    pub icon_pixels: Arc<[u8]>,
    pub icon_width: i32,
    pub icon_height: i32,
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
    icons: BrightnessIcons,
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
        let icons = BrightnessIcons::load()?;
        let bar_background = config::parse_rgba(&config.bar_background)?;
        let fill = config::parse_rgba(&config.fill)?;

        Ok(Self {
            config,
            root,
            state,
            icons,
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
        let icon = self.icons.icon(icon_kind(self.state.percent));
        ModuleVisual::Brightness(BrightnessVisual {
            percent: self.state.percent,
            icon_scale: self.config.icon_scale,
            icon_pixels: icon.pixels,
            icon_width: icon.width,
            icon_height: icon.height,
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

fn icon_kind(percent: u32) -> BrightnessIconKind {
    if percent < 50 {
        BrightnessIconKind::Low
    } else {
        BrightnessIconKind::High
    }
}

fn rasterize_svg(name: &str, svg_bytes: &[u8]) -> Result<BrightnessIcon> {
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

    if !status.success() || !output.status.success() {
        anyhow::bail!("brightness SVG rasterization failed for {name}");
    }

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "brightness SVG rasterizer returned {} bytes for {RASTER_SIZE}x{RASTER_SIZE}, expected {expected}: {name}",
        output.stdout.len()
    );

    crop_transparent_margin(name, &output.stdout, RASTER_SIZE, RASTER_SIZE)
}

fn crop_transparent_margin(
    name: &str,
    pixels: &[u8],
    width: i32,
    height: i32,
) -> Result<BrightnessIcon> {
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
        "brightness SVG has no visible pixels: {name}"
    );

    let cropped_width = max_x - min_x + 1;
    let cropped_height = max_y - min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);

    for y in min_y..=max_y {
        let start = ((y * width + min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&pixels[start..end]);
    }

    Ok(BrightnessIcon {
        pixels: Arc::from(cropped),
        width: cropped_width,
        height: cropped_height,
    })
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
        config.bar_width > 0 && config.bar_height > 0,
        "brightness bar dimensions must be positive"
    );
    ensure!(
        config.icon_scale.is_finite() && (0.1..=1.0).contains(&config.icon_scale),
        "brightness icon_scale must be in 0.1..=1.0"
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

fn default_icon_scale() -> f32 {
    1.0
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
    use super::{BrightnessIconKind, icon_kind};

    #[test]
    fn chooses_svg_icon_by_brightness() {
        assert_eq!(icon_kind(0), BrightnessIconKind::Low);
        assert_eq!(icon_kind(49), BrightnessIconKind::Low);
        assert_eq!(icon_kind(50), BrightnessIconKind::High);
        assert_eq!(icon_kind(100), BrightnessIconKind::High);
    }

    #[test]
    fn percent_math_is_bounded() {
        let max = 120_000_u64;
        let current = 60_000_u64;
        let percent = ((current as f64 / max as f64) * 100.0).round() as u32;
        assert_eq!(percent, 50);
    }
}
