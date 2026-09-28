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
    pub style: ModuleStyle,
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
