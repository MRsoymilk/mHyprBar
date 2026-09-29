use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::{
    config::{self, ModuleStyle},
    hyprland,
};

pub const NAME: &str = "layout";
pub const CONFIG_FILE: &str = "modules/layout.toml";

const ICON_DWINDLE_PNG: &[u8] = include_bytes!("../../res/layout/layout-dwindle.png");
const ICON_MASTER_PNG: &[u8] = include_bytes!("../../res/layout/layout-master.png");
const ICON_SCROLLING_PNG: &[u8] = include_bytes!("../../res/layout/layout-scrolling.png");
const ICON_MONOCLE_PNG: &[u8] = include_bytes!("../../res/layout/layout-monocle.png");

#[derive(Debug, Deserialize)]
struct LayoutConfig {
    #[serde(default = "default_layouts")]
    layouts: Vec<String>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_icon_scale")]
    icon_scale: f32,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutIconKind {
    Dwindle,
    Master,
    Scrolling,
    Monocle,
}

#[derive(Clone)]
struct LayoutIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone)]
struct LayoutIcons {
    dwindle: LayoutIcon,
    master: LayoutIcon,
    scrolling: LayoutIcon,
    monocle: LayoutIcon,
}

impl LayoutIcons {
    fn load() -> Result<Self> {
        let dwindle = rasterize_png("layout-dwindle.png", ICON_DWINDLE_PNG)?;
        let master = rasterize_png("layout-master.png", ICON_MASTER_PNG)?;
        let scrolling = rasterize_png("layout-scrolling.png", ICON_SCROLLING_PNG)?;
        let monocle = rasterize_png("layout-monocle.png", ICON_MONOCLE_PNG)?;
        let bounds = shared_alpha_bounds(&[&dwindle, &master, &scrolling, &monocle], "layout")?;

        Ok(Self {
            dwindle: crop_to_bounds(&dwindle, bounds)?,
            master: crop_to_bounds(&master, bounds)?,
            scrolling: crop_to_bounds(&scrolling, bounds)?,
            monocle: crop_to_bounds(&monocle, bounds)?,
        })
    }

    fn icon(&self, kind: LayoutIconKind) -> LayoutIcon {
        match kind {
            LayoutIconKind::Dwindle => self.dwindle.clone(),
            LayoutIconKind::Master => self.master.clone(),
            LayoutIconKind::Scrolling => self.scrolling.clone(),
            LayoutIconKind::Monocle => self.monocle.clone(),
        }
    }
}

#[derive(Clone)]
pub struct LayoutVisual {
    pub name: String,
    pub tooltip: String,
    pub icon_scale: f32,
    pub icon_pixels: Option<Arc<[u8]>>,
    pub icon_width: i32,
    pub icon_height: i32,
}

pub struct LayoutModule {
    config: LayoutConfig,
    current: String,
    icons: LayoutIcons,
    revision: u64,
}

impl LayoutModule {
    pub fn load() -> Result<Self> {
        let config: LayoutConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let icons = LayoutIcons::load()?;

        Ok(Self {
            config,
            current: String::new(),
            icons,
            revision: 0,
        })
    }

    fn cycle(&mut self, direction: i32) -> Result<bool> {
        let next = next_layout(&self.config.layouts, &self.current, direction);
        hyprland::set_active_layout(&next)?;
        self.current = next;
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }
}

impl StatusModule for LayoutModule {
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
        let next = hyprland::active_layout()?;
        if next != self.current {
            self.current = next;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(String::new())
    }

    fn visual(&self) -> ModuleVisual {
        let icon = layout_icon_kind(&self.current).map(|kind| self.icons.icon(kind));
        ModuleVisual::Layout(LayoutVisual {
            name: self.current.clone(),
            tooltip: layout_display_name(&self.current),
            icon_scale: self.config.icon_scale,
            icon_pixels: icon.as_ref().map(|icon| Arc::clone(&icon.pixels)),
            icon_width: icon.as_ref().map_or(1, |icon| icon.width),
            icon_height: icon.as_ref().map_or(1, |icon| icon.height),
        })
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn activate(&mut self) -> Result<bool> {
        self.cycle(1)
    }

    fn scroll(&mut self, direction: i32) -> Result<bool> {
        if direction == 0 {
            return Ok(false);
        }
        self.cycle(direction.signum())
    }
}

fn validate_config(config: &LayoutConfig) -> Result<()> {
    ensure!(
        config.interval_ms > 0,
        "layout interval_ms must be greater than zero"
    );
    ensure!(
        !config.layouts.is_empty(),
        "layout layouts must contain at least one layout"
    );
    ensure!(
        config
            .layouts
            .iter()
            .all(|layout| !layout.trim().is_empty()),
        "layout names must not be empty"
    );
    ensure!(
        config.icon_scale.is_finite() && (0.1..=1.0).contains(&config.icon_scale),
        "layout icon_scale must be in 0.1..=1.0"
    );
    config.style.validate()?;
    Ok(())
}

fn layout_icon_kind(name: &str) -> Option<LayoutIconKind> {
    match name.trim().to_ascii_lowercase().as_str() {
        "dwindle" => Some(LayoutIconKind::Dwindle),
        "master" => Some(LayoutIconKind::Master),
        "scrolling" => Some(LayoutIconKind::Scrolling),
        "monocle" => Some(LayoutIconKind::Monocle),
        _ => None,
    }
}

pub fn layout_display_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "dwindle" => "Dwindle".into(),
        "master" => "Master".into(),
        "scrolling" => "Scrolling".into(),
        "monocle" => "Monocle".into(),
        _ if name.trim().is_empty() => "Layout".into(),
        _ => name.trim().to_owned(),
    }
}

fn next_layout(layouts: &[String], current: &str, direction: i32) -> String {
    let len = layouts.len();
    let Some(index) = layouts.iter().position(|layout| layout == current) else {
        return layouts[0].clone();
    };

    if direction >= 0 {
        layouts[(index + 1) % len].clone()
    } else {
        layouts[(index + len - 1) % len].clone()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AlphaBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

fn rasterize_png(name: &str, png_bytes: &[u8]) -> Result<LayoutIcon> {
    const RASTER_SIZE: i32 = 96;

    let mut magick = Command::new("magick")
        .arg("png:-")
        .arg("-resize")
        .arg(format!("{RASTER_SIZE}x{RASTER_SIZE}!"))
        .arg("-depth")
        .arg("8")
        .arg("-alpha")
        .arg("on")
        .arg("rgba:-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch magick for {name}"))?;

    let mut stdin = magick.stdin.take().context("failed to open magick stdin")?;
    stdin
        .write_all(png_bytes)
        .with_context(|| format!("failed to feed PNG data for {name}"))?;
    drop(stdin);

    let output = magick
        .wait_with_output()
        .with_context(|| format!("failed to wait for magick for {name}"))?;
    if !output.status.success() {
        bail!("layout PNG rasterization failed for {name}");
    }

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "layout PNG rasterizer returned {} bytes for {RASTER_SIZE}x{RASTER_SIZE}, expected {expected}: {name}",
        output.stdout.len()
    );

    Ok(LayoutIcon {
        pixels: Arc::from(output.stdout),
        width: RASTER_SIZE,
        height: RASTER_SIZE,
    })
}

fn alpha_bounds(icon: &LayoutIcon) -> Option<AlphaBounds> {
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

fn shared_alpha_bounds(icons: &[&LayoutIcon], group: &str) -> Result<AlphaBounds> {
    ensure!(!icons.is_empty(), "{group} icon group must not be empty");

    let width = icons[0].width;
    let height = icons[0].height;
    let mut shared: Option<AlphaBounds> = None;

    for icon in icons {
        ensure!(
            icon.width == width && icon.height == height,
            "{group} icons must share the same raster canvas"
        );
        let bounds =
            alpha_bounds(icon).with_context(|| format!("{group} icon has no visible pixels"))?;
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

    shared.with_context(|| format!("{group} icon group has no visible pixels"))
}

fn crop_to_bounds(icon: &LayoutIcon, bounds: AlphaBounds) -> Result<LayoutIcon> {
    ensure!(
        bounds.min_x >= 0
            && bounds.min_y >= 0
            && bounds.max_x < icon.width
            && bounds.max_y < icon.height,
        "layout shared bounds are outside the raster canvas"
    );

    let cropped_width = bounds.max_x - bounds.min_x + 1;
    let cropped_height = bounds.max_y - bounds.min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);

    for y in bounds.min_y..=bounds.max_y {
        let start = ((y * icon.width + bounds.min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&icon.pixels[start..end]);
    }

    Ok(LayoutIcon {
        pixels: Arc::from(cropped),
        width: cropped_width,
        height: cropped_height,
    })
}

fn default_layouts() -> Vec<String> {
    vec![
        "dwindle".into(),
        "master".into(),
        "scrolling".into(),
        "monocle".into(),
    ]
}

fn default_interval_ms() -> u64 {
    750
}

fn default_icon_scale() -> f32 {
    0.8
}

#[cfg(test)]
mod tests {
    use super::{LayoutIconKind, layout_display_name, layout_icon_kind, next_layout};

    #[test]
    fn cycles_all_builtin_layouts_in_both_directions() {
        let layouts = vec![
            "dwindle".into(),
            "master".into(),
            "scrolling".into(),
            "monocle".into(),
        ];
        assert_eq!(next_layout(&layouts, "dwindle", 1), "master");
        assert_eq!(next_layout(&layouts, "master", 1), "scrolling");
        assert_eq!(next_layout(&layouts, "scrolling", 1), "monocle");
        assert_eq!(next_layout(&layouts, "monocle", 1), "dwindle");
        assert_eq!(next_layout(&layouts, "dwindle", -1), "monocle");
        assert_eq!(next_layout(&layouts, "unknown", 1), "dwindle");
    }

    #[test]
    fn maps_all_builtin_layout_icons() {
        assert_eq!(layout_icon_kind("dwindle"), Some(LayoutIconKind::Dwindle));
        assert_eq!(layout_icon_kind("master"), Some(LayoutIconKind::Master));
        assert_eq!(
            layout_icon_kind("scrolling"),
            Some(LayoutIconKind::Scrolling)
        );
        assert_eq!(layout_icon_kind("monocle"), Some(LayoutIconKind::Monocle));
        assert_eq!(layout_icon_kind("lua:columns"), None);
        assert_eq!(layout_display_name("scrolling"), "Scrolling");
        assert_eq!(layout_display_name("lua:columns"), "lua:columns");
    }
}
