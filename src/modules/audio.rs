use std::{process::Command, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "audio";
pub const CONFIG_FILE: &str = "modules/audio.toml";

#[derive(Debug, Deserialize)]
struct AudioConfig {
    #[serde(default = "default_target")]
    target: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_label")]
    label: String,
    #[serde(default = "default_muted_text")]
    muted_text: String,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct AudioModule {
    config: AudioConfig,
}

impl AudioModule {
    pub fn load() -> Result<Self> {
        let config: AudioConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "audio interval_ms must be greater than zero"
        );
        ensure!(
            !config.target.trim().is_empty(),
            "audio target must not be empty"
        );
        config.style.validate()?;
        Ok(Self { config })
    }
}

impl StatusModule for AudioModule {
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
        let output = Command::new("wpctl")
            .arg("get-volume")
            .arg(&self.config.target)
            .output()
            .context("failed to execute wpctl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("wpctl get-volume failed: {}", stderr.trim());
        }

        let value = String::from_utf8_lossy(&output.stdout);
        let state = parse_wpctl_volume(&value)?;
        if state.muted {
            if self.config.label.is_empty() {
                Ok(self.config.muted_text.clone())
            } else {
                Ok(format!("{} {}", self.config.label, self.config.muted_text))
            }
        } else if self.config.label.is_empty() {
            Ok(format!("{}%", state.percent))
        } else {
            Ok(format!("{} {}%", self.config.label, state.percent))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VolumeState {
    percent: u32,
    muted: bool,
}

fn parse_wpctl_volume(value: &str) -> Result<VolumeState> {
    let mut fields = value.split_whitespace();
    ensure!(
        fields.next() == Some("Volume:"),
        "unexpected wpctl volume output"
    );
    let raw = fields
        .next()
        .context("wpctl volume output is missing a value")?
        .parse::<f64>()
        .context("invalid wpctl volume value")?;
    ensure!(raw.is_finite() && raw >= 0.0, "invalid wpctl volume value");

    Ok(VolumeState {
        percent: (raw * 100.0).round().clamp(0.0, u32::MAX as f64) as u32,
        muted: value.contains("[MUTED]"),
    })
}

fn default_target() -> String {
    "@DEFAULT_AUDIO_SINK@".into()
}

fn default_interval_ms() -> u64 {
    500
}

fn default_label() -> String {
    "VOL".into()
}

fn default_muted_text() -> String {
    "MUTE".into()
}

#[cfg(test)]
mod tests {
    use super::{VolumeState, parse_wpctl_volume};

    #[test]
    fn parses_wpctl_volume() {
        assert_eq!(
            parse_wpctl_volume("Volume: 0.52\n").unwrap(),
            VolumeState {
                percent: 52,
                muted: false
            }
        );
        assert_eq!(
            parse_wpctl_volume("Volume: 1.25 [MUTED]\n").unwrap(),
            VolumeState {
                percent: 125,
                muted: true
            }
        );
    }
}
