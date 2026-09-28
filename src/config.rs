use std::{collections::BTreeMap, env, fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct BarConfig {
    #[serde(default = "default_position")]
    pub position: String,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default)]
    pub margin_top: i32,
    #[serde(default)]
    pub margin_right: i32,
    #[serde(default)]
    pub margin_bottom: i32,
    #[serde(default)]
    pub margin_left: i32,
    #[serde(default = "default_exclusive_zone")]
    pub exclusive_zone: bool,
    #[serde(default = "default_bar_background")]
    pub background: String,
    #[serde(default)]
    pub left: Vec<String>,
    #[serde(default)]
    pub center: Vec<String>,
    #[serde(default)]
    pub right: Vec<String>,
    #[serde(default)]
    pub workspaces: WorkspacesConfig,
}

impl BarConfig {
    pub fn load() -> Result<Self> {
        let path = config_root()?.join("bar.toml");
        let source = fs::read_to_string(&path)
            .with_context(|| format!("failed to read bar config {}", path.display()))?;
        let config: Self = toml::from_str(&source)
            .with_context(|| format!("invalid bar config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            matches!(self.position.as_str(), "top" | "bottom"),
            "bar position must be \"top\" or \"bottom\""
        );
        anyhow::ensure!(self.height > 0, "bar height must be greater than zero");
        anyhow::ensure!(self.margin_top >= 0, "bar margin_top must not be negative");
        anyhow::ensure!(
            self.margin_right >= 0,
            "bar margin_right must not be negative"
        );
        anyhow::ensure!(
            self.margin_bottom >= 0,
            "bar margin_bottom must not be negative"
        );
        anyhow::ensure!(
            self.margin_left >= 0,
            "bar margin_left must not be negative"
        );
        let _ = self.background_rgba()?;
        self.workspaces.validate()?;
        Ok(())
    }

    pub fn background_rgba(&self) -> Result<[u8; 4]> {
        parse_rgba(&self.background).with_context(|| "invalid bar background color")
    }

    pub fn module_order(&self) -> impl Iterator<Item = &str> {
        self.left
            .iter()
            .chain(self.center.iter())
            .chain(self.right.iter())
            .map(String::as_str)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkspacesConfig {
    #[serde(default = "default_workspace_count")]
    pub count: u32,
    #[serde(default = "default_workspace_width")]
    pub width: i32,
    #[serde(default)]
    pub gap: i32,
    #[serde(default = "default_workspace_margin_right")]
    pub margin_right: i32,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_workspace_text")]
    pub text: String,
    #[serde(default = "default_workspace_empty")]
    pub empty_background: String,
    #[serde(default = "default_workspace_occupied")]
    pub occupied_background: String,
    #[serde(default = "default_workspace_active")]
    pub active_background: String,
    #[serde(default)]
    pub monitor_slots: BTreeMap<String, u32>,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self {
            count: default_workspace_count(),
            width: default_workspace_width(),
            gap: 0,
            margin_right: default_workspace_margin_right(),
            font_family: default_font_family(),
            font_size: default_font_size(),
            text: default_workspace_text(),
            empty_background: default_workspace_empty(),
            occupied_background: default_workspace_occupied(),
            active_background: default_workspace_active(),
            monitor_slots: BTreeMap::new(),
        }
    }
}

impl WorkspacesConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.count > 0, "workspace count must be greater than zero");
        anyhow::ensure!(self.width > 0, "workspace width must be greater than zero");
        anyhow::ensure!(self.gap >= 0, "workspace gap must not be negative");
        anyhow::ensure!(
            self.margin_right >= 0,
            "workspace margin_right must not be negative"
        );
        anyhow::ensure!(
            !self.font_family.is_empty(),
            "workspace font_family must not be empty"
        );
        anyhow::ensure!(
            self.font_size > 0.0,
            "workspace font_size must be greater than zero"
        );
        let _ = self.text_rgba()?;
        let _ = self.empty_rgba()?;
        let _ = self.occupied_rgba()?;
        let _ = self.active_rgba()?;
        Ok(())
    }

    pub fn slot_for(&self, monitor_name: &str, hypr_monitor_id: i32) -> u32 {
        self.monitor_slots
            .get(monitor_name)
            .copied()
            .unwrap_or(hypr_monitor_id.max(0) as u32)
    }

    pub fn global_workspace_id(
        &self,
        monitor_name: &str,
        hypr_monitor_id: i32,
        local_workspace: u32,
    ) -> i32 {
        let slot = self.slot_for(monitor_name, hypr_monitor_id);
        (slot.saturating_mul(self.count) + local_workspace) as i32
    }

    pub fn strip_width(&self) -> i32 {
        let count = self.count as i32;
        count
            .saturating_mul(self.width)
            .saturating_add(count.saturating_sub(1).saturating_mul(self.gap))
            .saturating_add(self.margin_right)
    }

    pub fn local_workspace_at_x(&self, x: f64) -> Option<u32> {
        if x < 0.0 {
            return None;
        }
        let cell = self.width.saturating_add(self.gap);
        if cell <= 0 {
            return None;
        }
        let index = (x as i32) / cell;
        if index < 0 || index >= self.count as i32 {
            return None;
        }
        let within = (x as i32) % cell;
        if within >= self.width {
            return None;
        }
        Some(index as u32 + 1)
    }

    pub fn text_rgba(&self) -> Result<[u8; 4]> {
        parse_rgba(&self.text).with_context(|| "invalid workspace text color")
    }

    pub fn empty_rgba(&self) -> Result<[u8; 4]> {
        parse_rgba(&self.empty_background)
            .with_context(|| "invalid workspace empty background color")
    }

    pub fn occupied_rgba(&self) -> Result<[u8; 4]> {
        parse_rgba(&self.occupied_background)
            .with_context(|| "invalid workspace occupied background color")
    }

    pub fn active_rgba(&self) -> Result<[u8; 4]> {
        parse_rgba(&self.active_background)
            .with_context(|| "invalid workspace active background color")
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModuleStyle {
    #[serde(default = "default_foreground")]
    pub foreground: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_padding_x")]
    pub padding_x: i32,
    #[serde(default = "default_padding_y")]
    pub padding_y: i32,
    #[serde(default)]
    pub min_width: i32,
}

impl Default for ModuleStyle {
    fn default() -> Self {
        Self {
            foreground: default_foreground(),
            background: default_background(),
            font_family: default_font_family(),
            font_size: default_font_size(),
            padding_x: default_padding_x(),
            padding_y: default_padding_y(),
            min_width: 0,
        }
    }
}

impl ModuleStyle {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.foreground.is_empty(),
            "style foreground must not be empty"
        );
        anyhow::ensure!(
            !self.background.is_empty(),
            "style background must not be empty"
        );
        anyhow::ensure!(
            !self.font_family.is_empty(),
            "style font_family must not be empty"
        );
        anyhow::ensure!(
            self.font_size > 0.0,
            "style font_size must be greater than zero"
        );
        anyhow::ensure!(self.padding_x >= 0, "style padding_x must not be negative");
        anyhow::ensure!(self.padding_y >= 0, "style padding_y must not be negative");
        anyhow::ensure!(self.min_width >= 0, "style min_width must not be negative");
        let _ = self.foreground_rgba()?;
        let _ = self.background_rgba()?;
        Ok(())
    }

    pub fn foreground_rgba(&self) -> Result<[u8; 4]> {
        parse_rgba(&self.foreground).with_context(|| "invalid module foreground color")
    }

    pub fn background_rgba(&self) -> Result<[u8; 4]> {
        parse_rgba(&self.background).with_context(|| "invalid module background color")
    }
}

pub fn config_root() -> Result<PathBuf> {
    if let Some(path) = env::var_os("MHYPRBAR_CONFIG_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("mhyprbar"));
    }
    let home = env::var_os("HOME").context("neither XDG_CONFIG_HOME nor HOME is set")?;
    Ok(PathBuf::from(home).join(".config/mhyprbar"))
}

pub fn module_config_path(name: &str) -> Result<PathBuf> {
    Ok(config_root()?.join("modules").join(format!("{name}.toml")))
}

pub fn load_module<T>(name: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let path = module_config_path(name)?;
    let source = fs::read_to_string(&path)
        .with_context(|| format!("failed to read module config {}", path.display()))?;
    toml::from_str(&source).with_context(|| format!("invalid module config {}", path.display()))
}

pub(crate) fn parse_rgba(value: &str) -> Result<[u8; 4]> {
    if value == "transparent" {
        return Ok([0, 0, 0, 0]);
    }

    let hex = value
        .strip_prefix('#')
        .with_context(|| format!("color {value:?} must start with '#'"))?;
    let (rgb, alpha) = match hex.len() {
        6 => (hex, 255),
        8 => (&hex[..6], parse_byte(&hex[6..8])?),
        _ => anyhow::bail!("color {value:?} must use #RRGGBB or #RRGGBBAA"),
    };

    Ok([
        parse_byte(&rgb[0..2])?,
        parse_byte(&rgb[2..4])?,
        parse_byte(&rgb[4..6])?,
        alpha,
    ])
}

fn parse_byte(value: &str) -> Result<u8> {
    u8::from_str_radix(value, 16).with_context(|| format!("invalid hex byte {value:?}"))
}

fn default_position() -> String {
    "top".into()
}

fn default_height() -> u32 {
    30
}

fn default_exclusive_zone() -> bool {
    true
}

fn default_bar_background() -> String {
    "#181818EE".into()
}

fn default_workspace_count() -> u32 {
    9
}

fn default_workspace_width() -> i32 {
    28
}

fn default_workspace_margin_right() -> i32 {
    8
}

fn default_workspace_text() -> String {
    "#F2F2F2".into()
}

fn default_workspace_empty() -> String {
    "transparent".into()
}

fn default_workspace_occupied() -> String {
    "#383838".into()
}

fn default_workspace_active() -> String {
    "#666666".into()
}

fn default_foreground() -> String {
    "#f2f2f2".into()
}

fn default_background() -> String {
    "transparent".into()
}

fn default_font_family() -> String {
    "sans-serif".into()
}

fn default_font_size() -> f32 {
    13.0
}

fn default_padding_x() -> i32 {
    8
}

fn default_padding_y() -> i32 {
    4
}

#[cfg(test)]
mod tests {
    use super::WorkspacesConfig;

    #[test]
    fn workspace_ids_follow_monitor_slots() {
        let mut config = WorkspacesConfig::default();
        assert_eq!(config.global_workspace_id("eDP-1", 0, 1), 1);
        assert_eq!(config.global_workspace_id("DP-1", 1, 1), 10);
        assert_eq!(config.global_workspace_id("DP-1", 1, 9), 18);

        config.monitor_slots.insert("DP-1".into(), 3);
        assert_eq!(config.global_workspace_id("DP-1", 1, 1), 28);
        assert_eq!(config.global_workspace_id("DP-1", 1, 9), 36);
    }

    #[test]
    fn workspace_click_hit_respects_width_and_gap() {
        let mut config = WorkspacesConfig::default();
        config.width = 28;
        config.gap = 2;

        assert_eq!(config.local_workspace_at_x(0.0), Some(1));
        assert_eq!(config.local_workspace_at_x(27.9), Some(1));
        assert_eq!(config.local_workspace_at_x(28.0), None);
        assert_eq!(config.local_workspace_at_x(29.9), None);
        assert_eq!(config.local_workspace_at_x(30.0), Some(2));
        assert_eq!(config.local_workspace_at_x(267.9), Some(9));
        assert_eq!(config.local_workspace_at_x(268.0), None);
        assert_eq!(config.local_workspace_at_x(270.0), None);
    }
}
