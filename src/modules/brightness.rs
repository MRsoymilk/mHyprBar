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
    #[serde(default = "default_text_gap")]
    text_gap: i32,
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
        let low = rasterize_svg("brightness-low.svg", ICON_LOW_SVG)?;
        let high = rasterize_svg("brightness-high.svg", ICON_HIGH_SVG)?;
        let bounds = shared_alpha_bounds(&[&low, &high], "brightness")?;

        Ok(Self {
            low: crop_to_bounds(&low, bounds)?,
            high: crop_to_bounds(&high, bounds)?,
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
    pub text_gap: i32,
    pub fill: [u8; 4],
}

pub struct BrightnessModule {
    config: BrightnessConfig,
    root: PathBuf,
    state: BrightnessState,
    icons: BrightnessIcons,
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
        let fill = config::parse_rgba(&config.fill)?;

        Ok(Self {
            config,
            root,
            state,
            icons,
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
            text_gap: self.config.text_gap,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AlphaBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
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

    Ok(BrightnessIcon {
        pixels: Arc::from(output.stdout),
        width: RASTER_SIZE,
        height: RASTER_SIZE,
    })
}

fn alpha_bounds(icon: &BrightnessIcon) -> Option<AlphaBounds> {
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

fn shared_alpha_bounds(icons: &[&BrightnessIcon], group: &str) -> Result<AlphaBounds> {
    ensure!(
        !icons.is_empty(),
        "{group} SVG icon group must not be empty"
    );

    let width = icons[0].width;
    let height = icons[0].height;
    let mut shared: Option<AlphaBounds> = None;

    for icon in icons {
        ensure!(
            icon.width == width && icon.height == height,
            "{group} SVG icons must share the same raster canvas"
        );
        let bounds = alpha_bounds(icon)
            .with_context(|| format!("{group} SVG icon has no visible pixels"))?;
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

    shared.with_context(|| format!("{group} SVG icon group has no visible pixels"))
}

fn crop_to_bounds(icon: &BrightnessIcon, bounds: AlphaBounds) -> Result<BrightnessIcon> {
    ensure!(
        bounds.min_x >= 0
            && bounds.min_y >= 0
            && bounds.max_x < icon.width
            && bounds.max_y < icon.height,
        "brightness SVG shared bounds are outside the raster canvas"
    );

    let cropped_width = bounds.max_x - bounds.min_x + 1;
    let cropped_height = bounds.max_y - bounds.min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);

    for y in bounds.min_y..=bounds.max_y {
        let start = ((y * icon.width + bounds.min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&icon.pixels[start..end]);
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

fn default_text_gap() -> i32 {
    4
}

fn default_fill() -> String {
    "#FFD84A".into()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        AlphaBounds, BrightnessIcon, BrightnessIconKind, alpha_bounds, crop_to_bounds, icon_kind,
        shared_alpha_bounds,
    };

    #[test]
    fn shared_bounds_preserve_relative_icon_size() {
        let mut large = vec![0_u8; 8 * 8 * 4];
        let mut small = vec![0_u8; 8 * 8 * 4];
        for y in 1..7 {
            for x in 1..7 {
                large[((y * 8 + x) * 4 + 3) as usize] = 255;
            }
        }
        for y in 2..6 {
            for x in 2..6 {
                small[((y * 8 + x) * 4 + 3) as usize] = 255;
            }
        }

        let large = BrightnessIcon {
            pixels: Arc::from(large),
            width: 8,
            height: 8,
        };
        let small = BrightnessIcon {
            pixels: Arc::from(small),
            width: 8,
            height: 8,
        };
        let shared = shared_alpha_bounds(&[&large, &small], "test").expect("shared bounds");
        assert_eq!(
            shared,
            AlphaBounds {
                min_x: 1,
                min_y: 1,
                max_x: 6,
                max_y: 6
            }
        );

        let cropped_small = crop_to_bounds(&small, shared).expect("crop small");
        assert_eq!((cropped_small.width, cropped_small.height), (6, 6));
        assert_eq!(
            alpha_bounds(&cropped_small).expect("small visible bounds"),
            AlphaBounds {
                min_x: 1,
                min_y: 1,
                max_x: 4,
                max_y: 4
            }
        );
    }

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
