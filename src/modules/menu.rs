use std::{process::Command, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "menu";
pub const CONFIG_FILE: &str = "modules/menu.toml";

#[derive(Debug, Deserialize)]
struct MenuConfig {
    #[serde(default = "default_label")]
    label: String,
    #[serde(default = "default_command")]
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct MenuModule {
    config: MenuConfig,
}

impl MenuModule {
    pub fn load() -> Result<Self> {
        let config: MenuConfig = config::load_module(NAME)?;
        ensure!(!config.label.is_empty(), "menu label must not be empty");
        ensure!(!config.command.is_empty(), "menu command must not be empty");
        config.style.validate()?;
        Ok(Self { config })
    }
}

impl StatusModule for MenuModule {
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
        Ok(self.config.label.clone())
    }

    fn activate(&mut self) -> Result<bool> {
        Command::new(&self.config.command)
            .args(&self.config.args)
            .spawn()
            .with_context(|| format!("failed to launch {}", self.config.command))?;
        Ok(false)
    }
}

fn default_label() -> String {
    "Menu".into()
}

fn default_command() -> String {
    "mhyprmenu".into()
}

#[cfg(test)]
mod tests {
    use super::{MenuConfig, MenuModule};
    use crate::{config::ModuleStyle, modules::StatusModule};

    #[test]
    fn activates_configured_command_without_shell() {
        let mut module = MenuModule {
            config: MenuConfig {
                label: "Menu".into(),
                command: "/bin/true".into(),
                args: Vec::new(),
                style: ModuleStyle::default(),
            },
        };
        assert!(!module.activate().unwrap());
    }
}
