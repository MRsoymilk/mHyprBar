use std::{fs, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "memory";
pub const CONFIG_FILE: &str = "modules/memory.toml";

#[derive(Debug, Deserialize)]
struct MemoryConfig {
    #[serde(default = "default_label")]
    label: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct MemoryModule {
    config: MemoryConfig,
}

impl MemoryModule {
    pub fn load() -> Result<Self> {
        let config: MemoryConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "memory interval_ms must be greater than zero"
        );
        ensure!(!config.label.is_empty(), "memory label must not be empty");
        config.style.validate()?;
        Ok(Self { config })
    }

    fn read_kib() -> Result<(u64, u64)> {
        let content =
            fs::read_to_string("/proc/meminfo").context("failed to read /proc/meminfo")?;
        let mut total = None;
        let mut available = None;

        for line in content.lines() {
            let mut fields = line.split_whitespace();
            match fields.next() {
                Some("MemTotal:") => {
                    total = fields.next().and_then(|value| value.parse::<u64>().ok());
                }
                Some("MemAvailable:") => {
                    available = fields.next().and_then(|value| value.parse::<u64>().ok());
                }
                _ => {}
            }
        }

        Ok((
            total.context("MemTotal missing from /proc/meminfo")?,
            available.context("MemAvailable missing from /proc/meminfo")?,
        ))
    }
}

impl StatusModule for MemoryModule {
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
        let (total, available) = Self::read_kib()?;
        ensure!(total > 0, "MemTotal must be greater than zero");
        let used = total.saturating_sub(available);
        let percent = 100.0 * used as f64 / total as f64;
        Ok(format!("{} {:.0}%", self.config.label, percent))
    }
}

fn default_label() -> String {
    "MEM".into()
}

fn default_interval_ms() -> u64 {
    2_000
}
