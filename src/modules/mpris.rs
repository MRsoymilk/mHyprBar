use std::{
    process::{Command, Output},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "mpris";
pub const CONFIG_FILE: &str = "modules/mpris.toml";

#[derive(Debug, Deserialize)]
struct MprisConfig {
    #[serde(default = "default_player")]
    player: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_max_chars")]
    max_chars: usize,
    #[serde(default = "default_separator")]
    separator: String,
    #[serde(default)]
    show_status: bool,
    #[serde(default)]
    empty_text: String,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct MprisModule {
    config: MprisConfig,
}

impl MprisModule {
    pub fn load() -> Result<Self> {
        let config: MprisConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "mpris interval_ms must be greater than zero"
        );
        ensure!(
            config.max_chars > 0,
            "mpris max_chars must be greater than zero"
        );
        config.style.validate()?;
        Ok(Self { config })
    }

    fn playerctl(&self, args: &[&str]) -> Result<Output> {
        let mut command = Command::new("playerctl");
        if self.config.player != "auto" {
            command.arg("--player").arg(&self.config.player);
        }
        command
            .args(args)
            .output()
            .context("failed to execute playerctl")
    }
}

impl StatusModule for MprisModule {
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
        let status_output = self.playerctl(&["status"])?;
        if !status_output.status.success() {
            return Ok(self.config.empty_text.clone());
        }
        let status = String::from_utf8_lossy(&status_output.stdout)
            .trim()
            .to_owned();

        let metadata_output = self.playerctl(&["metadata", "--format", "{{artist}}\t{{title}}"])?;
        if !metadata_output.status.success() {
            return Ok(self.config.empty_text.clone());
        }

        let metadata = String::from_utf8_lossy(&metadata_output.stdout);
        let (artist, title) = parse_metadata(&metadata);
        let core = match (artist.is_empty(), title.is_empty()) {
            (true, true) => self.config.empty_text.clone(),
            (true, false) => title.to_owned(),
            (false, true) => artist.to_owned(),
            (false, false) => format!("{artist}{}{title}", self.config.separator),
        };

        let text = if self.config.show_status && !core.is_empty() {
            format!("{} {}", status_symbol(&status), core)
        } else {
            core
        };

        Ok(truncate_chars(&text, self.config.max_chars))
    }
}

fn parse_metadata(value: &str) -> (&str, &str) {
    let line = value.trim_end_matches(['\r', '\n']);
    line.split_once('\t')
        .map(|(artist, title)| (artist.trim(), title.trim()))
        .unwrap_or(("", line.trim()))
}

fn status_symbol(status: &str) -> &'static str {
    match status {
        "Playing" => "▶",
        "Paused" => "Ⅱ",
        "Stopped" => "■",
        _ => "·",
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

fn default_player() -> String {
    "auto".into()
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_max_chars() -> usize {
    64
}

fn default_separator() -> String {
    " — ".into()
}

#[cfg(test)]
mod tests {
    use super::{parse_metadata, status_symbol, truncate_chars};

    #[test]
    fn parses_metadata_fields() {
        assert_eq!(parse_metadata("Artist\tTrack\n"), ("Artist", "Track"));
        assert_eq!(parse_metadata("\tTrack\n"), ("", "Track"));
    }

    #[test]
    fn formats_status_and_utf8_truncation() {
        assert_eq!(status_symbol("Playing"), "▶");
        assert_eq!(status_symbol("Paused"), "Ⅱ");
        assert_eq!(truncate_chars("山海经音乐", 4), "山海经…");
    }
}
