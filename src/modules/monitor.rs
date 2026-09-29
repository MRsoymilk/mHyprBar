use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::{
    config::{self, ModuleStyle},
    hyprland::{self, MonitorInfo},
};

pub const NAME: &str = "monitor";
pub const CONFIG_FILE: &str = "modules/monitor.toml";

const ICON_MONITOR_SVG: &[u8] = include_bytes!("../../res/monitor/monitor.svg");

#[derive(Debug, Deserialize)]
struct MonitorConfig {
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_icon_scale")]
    icon_scale: f32,
    #[serde(default = "default_icon_gap")]
    icon_gap: i32,
    #[serde(default = "default_topology_width")]
    topology_width: i32,
    #[serde(default = "default_topology_height")]
    topology_height: i32,
    #[serde(default = "default_dot_size")]
    dot_size: i32,
    #[serde(default = "default_dot_color")]
    dot_color: String,
    #[serde(default = "default_focused_dot_color")]
    focused_dot_color: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone)]
struct MonitorIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone, Debug)]
pub struct MonitorDot {
    pub x: f32,
    pub y: f32,
    pub focused: bool,
}

#[derive(Clone)]
pub struct MonitorVisual {
    pub icon_scale: f32,
    pub icon_gap: i32,
    pub icon_pixels: Arc<[u8]>,
    pub icon_width: i32,
    pub icon_height: i32,
    pub topology_width: i32,
    pub topology_height: i32,
    pub dot_size: i32,
    pub dot_color: [u8; 4],
    pub focused_dot_color: [u8; 4],
    pub dots: Vec<MonitorDot>,
}

pub struct MonitorModule {
    config: MonitorConfig,
    icon: MonitorIcon,
    monitors: Vec<MonitorInfo>,
    dot_color: [u8; 4],
    focused_dot_color: [u8; 4],
    revision: u64,
}

impl MonitorModule {
    pub fn load() -> Result<Self> {
        let config: MonitorConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let _ = crate::monitor_popup::MonitorPopupConfig::load()?;

        Ok(Self {
            dot_color: config::parse_rgba(&config.dot_color)?,
            focused_dot_color: config::parse_rgba(&config.focused_dot_color)?,
            icon: rasterize_svg("monitor.svg", ICON_MONITOR_SVG)?,
            monitors: Vec::new(),
            config,
            revision: 0,
        })
    }
}

impl StatusModule for MonitorModule {
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
        let monitors = hyprland::monitor_infos()?;
        if monitors != self.monitors {
            self.monitors = monitors;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Monitor(MonitorVisual {
            icon_scale: self.config.icon_scale,
            icon_gap: self.config.icon_gap,
            icon_pixels: Arc::clone(&self.icon.pixels),
            icon_width: self.icon.width,
            icon_height: self.icon.height,
            topology_width: self.config.topology_width,
            topology_height: self.config.topology_height,
            dot_size: self.config.dot_size,
            dot_color: self.dot_color,
            focused_dot_color: self.focused_dot_color,
            dots: topology_dots(&self.monitors),
        })
    }
}

fn topology_dots(monitors: &[MonitorInfo]) -> Vec<MonitorDot> {
    if monitors.is_empty() {
        return Vec::new();
    }
    if monitors.len() == 1 {
        return vec![MonitorDot {
            x: 0.5,
            y: 0.5,
            focused: monitors[0].focused,
        }];
    }

    let min_x = monitors
        .iter()
        .map(|m| m.x as f64)
        .fold(f64::INFINITY, f64::min);
    let min_y = monitors
        .iter()
        .map(|m| m.y as f64)
        .fold(f64::INFINITY, f64::min);
    let max_x = monitors
        .iter()
        .map(|m| m.x as f64 + m.logical_width())
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = monitors
        .iter()
        .map(|m| m.y as f64 + m.logical_height())
        .fold(f64::NEG_INFINITY, f64::max);
    let span_x = (max_x - min_x).max(1.0);
    let span_y = (max_y - min_y).max(1.0);

    monitors
        .iter()
        .map(|m| MonitorDot {
            x: (((m.x as f64 + m.logical_width() / 2.0 - min_x) / span_x) as f32).clamp(0.0, 1.0),
            y: (((m.y as f64 + m.logical_height() / 2.0 - min_y) / span_y) as f32).clamp(0.0, 1.0),
            focused: m.focused,
        })
        .collect()
}

fn rasterize_svg(name: &str, svg_bytes: &[u8]) -> Result<MonitorIcon> {
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
    let stdout = svg.stdout.take().context("failed to capture rsvg output")?;
    let mut svg_stdin = svg.stdin.take().context("failed to open rsvg stdin")?;
    let magick = Command::new("magick")
        .arg("png:-")
        .arg("rgba:-")
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch magick for {name}"))?;
    svg_stdin.write_all(svg_bytes)?;
    drop(svg_stdin);
    ensure!(svg.wait()?.success(), "monitor SVG conversion failed");
    let output = magick.wait_with_output()?;
    ensure!(output.status.success(), "monitor SVG rasterization failed");

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "invalid monitor SVG raster size"
    );

    crop_transparent_margin(&output.stdout, RASTER_SIZE, RASTER_SIZE)
}

fn crop_transparent_margin(pixels: &[u8], width: i32, height: i32) -> Result<MonitorIcon> {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = -1;
    let mut max_y = -1;
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            if pixels[offset + 3] != 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    ensure!(
        max_x >= min_x && max_y >= min_y,
        "monitor SVG has no visible pixels"
    );
    let cropped_width = max_x - min_x + 1;
    let cropped_height = max_y - min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);
    for y in min_y..=max_y {
        let start = ((y * width + min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&pixels[start..end]);
    }
    Ok(MonitorIcon {
        pixels: Arc::from(cropped),
        width: cropped_width,
        height: cropped_height,
    })
}

fn validate_config(config: &MonitorConfig) -> Result<()> {
    ensure!(
        config.interval_ms > 0,
        "monitor interval_ms must be greater than zero"
    );
    ensure!(
        config.icon_scale.is_finite() && (0.1..=1.0).contains(&config.icon_scale),
        "monitor icon_scale must be in 0.1..=1.0"
    );
    ensure!(
        config.icon_gap >= 0,
        "monitor icon_gap must not be negative"
    );
    ensure!(
        config.topology_width > 0,
        "monitor topology_width must be positive"
    );
    ensure!(
        config.topology_height > 0,
        "monitor topology_height must be positive"
    );
    ensure!(config.dot_size > 0, "monitor dot_size must be positive");
    config.style.validate()?;
    Ok(())
}

fn default_interval_ms() -> u64 {
    1000
}
fn default_icon_scale() -> f32 {
    0.72
}
fn default_icon_gap() -> i32 {
    4
}
fn default_topology_width() -> i32 {
    26
}
fn default_topology_height() -> i32 {
    18
}
fn default_dot_size() -> i32 {
    4
}
fn default_dot_color() -> String {
    "#A0A0A0".into()
}
fn default_focused_dot_color() -> String {
    "#F2F2F2".into()
}

#[cfg(test)]
mod tests {
    use super::topology_dots;
    use crate::hyprland::MonitorInfo;

    fn monitor(name: &str, x: i32, y: i32, focused: bool) -> MonitorInfo {
        MonitorInfo {
            id: 0,
            name: name.into(),
            description: String::new(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            x,
            y,
            scale: 1.0,
            focused,
            dpms_status: true,
            active_workspace: 1,
            available_modes: Vec::new(),
        }
    }

    #[test]
    fn topology_preserves_relative_monitor_positions() {
        let dots = topology_dots(&[
            monitor("left", -1920, 0, false),
            monitor("right", 0, 0, true),
            monitor("bottom-left", -1920, 1080, false),
        ]);
        assert_eq!(dots.len(), 3);
        assert!(dots[0].x < dots[1].x);
        assert!(dots[2].y > dots[0].y);
        assert!(dots[1].focused);
    }
}
