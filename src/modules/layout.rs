use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::{
    config::{self, ModuleStyle},
    hyprland,
};

pub const NAME: &str = "layout";
pub const CONFIG_FILE: &str = "modules/layout.toml";

#[derive(Debug, Deserialize)]
struct LayoutConfig {
    #[serde(default = "default_layouts")]
    layouts: Vec<String>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_icon_width")]
    icon_width: i32,
    #[serde(default = "default_icon_height")]
    icon_height: i32,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone)]
pub struct LayoutVisual {
    pub name: String,
    pub icon_width: i32,
    pub icon_height: i32,
}

pub struct LayoutModule {
    config: LayoutConfig,
    current: String,
    revision: u64,
}

impl LayoutModule {
    pub fn load() -> Result<Self> {
        let config: LayoutConfig = config::load_module(NAME)?;
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
            config.icon_width > 4 && config.icon_height > 4,
            "layout icon dimensions must be greater than 4"
        );
        config.style.validate()?;

        Ok(Self {
            config,
            current: String::new(),
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
        ModuleVisual::Layout(LayoutVisual {
            name: self.current.clone(),
            icon_width: self.config.icon_width,
            icon_height: self.config.icon_height,
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

fn default_layouts() -> Vec<String> {
    vec!["dwindle".into(), "master".into()]
}

fn default_interval_ms() -> u64 {
    750
}

fn default_icon_width() -> i32 {
    22
}

fn default_icon_height() -> i32 {
    16
}

#[cfg(test)]
mod tests {
    use super::next_layout;

    #[test]
    fn cycles_layouts_in_both_directions() {
        let layouts = vec!["dwindle".into(), "master".into(), "monocle".into()];
        assert_eq!(next_layout(&layouts, "dwindle", 1), "master");
        assert_eq!(next_layout(&layouts, "dwindle", -1), "monocle");
        assert_eq!(next_layout(&layouts, "unknown", 1), "dwindle");
    }
}
