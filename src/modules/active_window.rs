use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::StatusModule;
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
    #[serde(default)]
    style: ModuleStyle,
}

pub struct ActiveWindowModule {
    config: ActiveWindowConfig,
}

impl ActiveWindowModule {
    pub fn load() -> Result<Self> {
        let config: ActiveWindowConfig = config::load_module(NAME)?;
        ensure!(
            config.max_chars > 0,
            "active_window max_chars must be greater than zero"
        );
        config.style.validate()?;
        Ok(Self { config })
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
        Ok(self.format_text(&active.class, &active.title))
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
