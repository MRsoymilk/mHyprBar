mod icon;
pub mod popup;

use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use self::icon::{WindowIcon, resolve_window_icon};
use crate::{
    config::{self, ModuleStyle},
    hyprland,
};

pub const NAME: &str = "active_window";
pub const CONFIG_FILE: &str = "modules/active_window.toml";

#[derive(Debug, Deserialize)]
struct ActiveWindowConfig {
    #[serde(default = "default_max_chars")]
    max_chars: usize,
    #[serde(default)]
    show_class: bool,
    #[serde(default = "default_separator")]
    separator: String,
    #[serde(default)]
    empty_text: String,
    #[serde(default = "default_icon_size")]
    icon_size: i32,
    #[serde(default = "default_icon_gap")]
    icon_gap: i32,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone, Debug)]
pub struct ActiveWindowVisual {
    pub icon: Option<WindowIcon>,
    pub icon_size: i32,
    pub icon_gap: i32,
}

pub struct ActiveWindowModule {
    config: ActiveWindowConfig,
    visual: ActiveWindowVisual,
    icon_key: String,
    revision: u64,
}

impl ActiveWindowModule {
    pub fn load() -> Result<Self> {
        let config: ActiveWindowConfig = config::load_module(NAME)?;
        ensure!(
            config.max_chars > 0,
            "active_window max_chars must be greater than zero"
        );
        ensure!(
            config.icon_size > 0,
            "active_window icon_size must be greater than zero"
        );
        ensure!(
            config.icon_gap >= 0,
            "active_window icon_gap must not be negative"
        );
        config.style.validate()?;
        let visual = ActiveWindowVisual {
            icon: None,
            icon_size: config.icon_size,
            icon_gap: config.icon_gap,
        };
        Ok(Self {
            config,
            visual,
            icon_key: String::new(),
            revision: 0,
        })
    }

    fn format_text(&self, class: &str, title: &str) -> String {
        let title = title.trim();
        let class = class.trim();
        let text = if title.is_empty() {
            self.config.empty_text.clone()
        } else if self.config.show_class && !class.is_empty() {
            format!("{class}{}{}", self.config.separator, title)
        } else {
            title.to_owned()
        };
        truncate_chars(&text, self.config.max_chars)
    }
}

impl StatusModule for ActiveWindowModule {
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
        let active = hyprland::active_window()?;
        let icon_key = format!(
            "{}\u{1f}{}",
            active.class.to_ascii_lowercase(),
            active.initial_class.to_ascii_lowercase()
        );
        if icon_key != self.icon_key {
            self.visual.icon =
                if active.class.trim().is_empty() && active.initial_class.trim().is_empty() {
                    None
                } else {
                    resolve_window_icon(&active.class, &active.initial_class, self.config.icon_size)
                };
            self.icon_key = icon_key;
            self.revision = self.revision.wrapping_add(1);
        }

        let class = if active.class.trim().is_empty() {
            &active.initial_class
        } else {
            &active.class
        };
        Ok(self.format_text(class, &active.title))
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::ActiveWindow(self.visual.clone())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() && max_chars > 1 {
        let mut shortened: String = value.chars().take(max_chars - 1).collect();
        shortened.push('…');
        shortened
    } else {
        prefix
    }
}

fn default_max_chars() -> usize {
    80
}

fn default_separator() -> String {
    " · ".into()
}

fn default_icon_size() -> i32 {
    18
}

fn default_icon_gap() -> i32 {
    6
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncates_utf8_by_characters() {
        assert_eq!(truncate_chars("abcdef", 4), "abc…");
        assert_eq!(truncate_chars("你好世界", 3), "你好…");
        assert_eq!(truncate_chars("abc", 3), "abc");
    }
}
