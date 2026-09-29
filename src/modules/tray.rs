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
    pub menu: MenuConfig,
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

#[derive(Clone, Debug, Deserialize)]
pub struct MenuConfig {
    #[serde(default = "default_menu_width")]
    pub width: i32,
    #[serde(default = "default_menu_item_height")]
    pub item_height: i32,
    #[serde(default = "default_menu_padding_x")]
    pub padding_x: i32,
    #[serde(default = "default_menu_border_width")]
    pub border_width: i32,
    #[serde(default = "default_menu_separator_inset")]
    pub separator_inset: i32,
    #[serde(default = "default_menu_indicator")]
    pub indicator: String,
    #[serde(default = "default_menu_hover")]
    pub hover_background: String,
    #[serde(default = "default_menu_border")]
    pub border: String,
    #[serde(default = "default_menu_separator")]
    pub separator: String,
    #[serde(default = "default_menu_style")]
    pub style: ModuleStyle,
}

impl Default for MenuConfig {
    fn default() -> Self {
        Self {
            width: default_menu_width(),
            item_height: default_menu_item_height(),
            padding_x: default_menu_padding_x(),
            border_width: default_menu_border_width(),
            separator_inset: default_menu_separator_inset(),
            indicator: default_menu_indicator(),
            hover_background: default_menu_hover(),
            border: default_menu_border(),
            separator: default_menu_separator(),
            style: default_menu_style(),
        }
    }
}

impl MenuConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "tray menu width must be greater than zero");
        ensure!(
            self.item_height > 0,
            "tray menu item_height must be greater than zero"
        );
        ensure!(
            self.padding_x >= 0,
            "tray menu padding_x must not be negative"
        );
        ensure!(
            self.border_width >= 0,
            "tray menu border_width must not be negative"
        );
        ensure!(
            self.separator_inset >= 0,
            "tray menu separator_inset must not be negative"
        );
        ensure!(
            !self.indicator.is_empty(),
            "tray menu indicator must not be empty"
        );
        self.style.validate()?;
        let _ = config::parse_rgba(&self.hover_background)?;
        let _ = config::parse_rgba(&self.border)?;
        let _ = config::parse_rgba(&self.separator)?;
        Ok(())
    }

    pub fn hover_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.hover_background)
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn separator_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.separator)
    }
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
        self.menu.validate()?;
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

fn default_menu_width() -> i32 {
    280
}

fn default_menu_item_height() -> i32 {
    30
}

fn default_menu_padding_x() -> i32 {
    10
}

fn default_menu_border_width() -> i32 {
    1
}

fn default_menu_separator_inset() -> i32 {
    8
}

fn default_menu_indicator() -> String {
    "›".into()
}

fn default_menu_hover() -> String {
    "#3A3A3AF0".into()
}

fn default_menu_border() -> String {
    "#626262".into()
}

fn default_menu_separator() -> String {
    "#626262".into()
}

fn default_menu_style() -> ModuleStyle {
    ModuleStyle {
        foreground: "#F2F2F2".into(),
        background: "#202020F2".into(),
        font_family: "sans-serif".into(),
        font_size: 13.0,
        padding_x: 0,
        padding_y: 0,
        min_width: 0,
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
    2
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
