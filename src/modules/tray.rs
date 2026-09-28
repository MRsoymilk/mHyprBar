use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "tray";
pub const CONFIG_FILE: &str = "modules/tray.toml";

#[derive(Clone, Debug, Deserialize)]
pub struct TrayConfig {
    #[serde(default = "default_icon_size")]
    pub icon_size: i32,
    #[serde(default = "default_spacing")]
    pub spacing: i32,
    #[serde(default)]
    pub show_passive: bool,
    #[serde(default)]
    pub tooltip: TooltipConfig,
    #[serde(default)]
    pub style: ModuleStyle,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TooltipConfig {
    #[serde(default = "default_tooltip_enabled")]
    pub enabled: bool,
    #[serde(default = "default_tooltip_delay_ms")]
    pub delay_ms: u64,
    #[serde(default = "default_tooltip_offset")]
    pub offset: i32,
    #[serde(default = "default_tooltip_max_chars")]
    pub max_chars: usize,
    #[serde(default = "default_tooltip_style")]
    pub style: ModuleStyle,
}

impl Default for TooltipConfig {
    fn default() -> Self {
        Self {
            enabled: default_tooltip_enabled(),
            delay_ms: default_tooltip_delay_ms(),
            offset: default_tooltip_offset(),
            max_chars: default_tooltip_max_chars(),
            style: default_tooltip_style(),
        }
    }
}

impl TrayConfig {
    pub fn load() -> Result<Self> {
        let config: Self = config::load_module(NAME)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.icon_size > 0,
            "tray icon_size must be greater than zero"
        );
        ensure!(self.spacing >= 0, "tray spacing must not be negative");
        ensure!(
            self.tooltip.offset >= 0,
            "tray tooltip offset must not be negative"
        );
        ensure!(
            self.tooltip.max_chars > 0,
            "tray tooltip max_chars must be greater than zero"
        );
        self.tooltip.style.validate()?;
        self.style.validate()
    }
}

pub struct TrayModule {
    config: TrayConfig,
}

impl TrayModule {
    pub fn load() -> Result<Self> {
        Ok(Self {
            config: TrayConfig::load()?,
        })
    }
}

impl StatusModule for TrayModule {
    fn name(&self) -> &'static str {
        NAME
    }

    fn interval(&self) -> Option<Duration> {
        None
    }

    fn style(&self) -> &ModuleStyle {
        &self.config.style
    }

    fn sample(&mut self) -> Result<String> {
        Ok(String::new())
    }
}

fn default_icon_size() -> i32 {
    18
}

fn default_spacing() -> i32 {
    6
}

fn default_tooltip_enabled() -> bool {
    true
}

fn default_tooltip_delay_ms() -> u64 {
    350
}

fn default_tooltip_offset() -> i32 {
    6
}

fn default_tooltip_max_chars() -> usize {
    96
}

fn default_tooltip_style() -> ModuleStyle {
    ModuleStyle {
        foreground: "#F2F2F2".into(),
        background: "#202020EE".into(),
        font_family: "sans-serif".into(),
        font_size: 12.0,
        padding_x: 8,
        padding_y: 5,
        min_width: 0,
    }
}
