use std::{fs, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "cpu";
pub const CONFIG_FILE: &str = "modules/cpu.toml";

#[derive(Debug, Deserialize)]
struct CpuConfig {
    #[serde(default = "default_label")]
    label: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_warn_percent")]
    warn_percent: f32,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct CpuModule {
    config: CpuConfig,
    previous: Option<(u64, u64)>,
}

impl CpuModule {
    pub fn load() -> Result<Self> {
        let config: CpuConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "cpu interval_ms must be greater than zero"
        );
        ensure!(
            (0.0..=100.0).contains(&config.warn_percent),
            "cpu warn_percent must be between 0 and 100"
        );
        ensure!(!config.label.is_empty(), "cpu label must not be empty");
        config.style.validate()?;
        Ok(Self {
            config,
            previous: None,
        })
    }

    fn counters() -> Result<(u64, u64)> {
        let content = fs::read_to_string("/proc/stat").context("failed to read /proc/stat")?;
        let line = content.lines().next().context("/proc/stat is empty")?;
        let mut fields = line.split_whitespace();
        ensure!(fields.next() == Some("cpu"), "invalid /proc/stat cpu line");

        let values: Vec<u64> = fields
            .take(10)
            .map(|value| value.parse::<u64>())
            .collect::<std::result::Result<_, _>>()
            .context("invalid /proc/stat cpu counter")?;
        ensure!(values.len() >= 4, "incomplete /proc/stat cpu counters");

        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        let total = values.iter().copied().sum();
        Ok((total, idle))
    }
}

impl StatusModule for CpuModule {
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
        let current = Self::counters()?;
        let percent = self.previous.and_then(|previous| {
            let total_delta = current.0.saturating_sub(previous.0);
            let idle_delta = current.1.saturating_sub(previous.1);
            (total_delta > 0).then(|| {
                100.0 * (total_delta.saturating_sub(idle_delta)) as f64 / total_delta as f64
            })
        });
        self.previous = Some(current);

        Ok(match percent {
            Some(value) => format!("{} {:.0}%", self.config.label, value),
            None => format!("{} --%", self.config.label),
        })
    }
}

fn default_label() -> String {
    "CPU".into()
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_warn_percent() -> f32 {
    85.0
}
