use std::{fs, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
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
    #[serde(default = "default_graph_enabled")]
    graph_enabled: bool,
    #[serde(default = "default_graph_width")]
    graph_width: i32,
    #[serde(default = "default_graph_height")]
    graph_height: i32,
    #[serde(default = "default_step_width")]
    step_width: i32,
    #[serde(default = "default_step_spacing")]
    step_spacing: i32,
    #[serde(default = "default_text_gap")]
    text_gap: i32,
    #[serde(default = "default_show_percent")]
    show_percent: bool,
    #[serde(default = "default_graph_background")]
    graph_background: String,
    #[serde(default = "default_graph_low")]
    graph_low: String,
    #[serde(default = "default_graph_mid")]
    graph_mid: String,
    #[serde(default = "default_graph_high")]
    graph_high: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone)]
pub struct CpuVisual {
    pub history: Vec<f32>,
    pub graph_enabled: bool,
    pub graph_width: i32,
    pub graph_height: i32,
    pub step_width: i32,
    pub step_spacing: i32,
    pub text_gap: i32,
    pub warn_percent: f32,
    pub graph_background: [u8; 4],
    pub graph_low: [u8; 4],
    pub graph_mid: [u8; 4],
    pub graph_high: [u8; 4],
}

pub struct CpuModule {
    config: CpuConfig,
    previous: Option<(u64, u64)>,
    history: Vec<f32>,
    graph_background: [u8; 4],
    graph_low: [u8; 4],
    graph_mid: [u8; 4],
    graph_high: [u8; 4],
    revision: u64,
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
        ensure!(
            config.graph_width > 0,
            "cpu graph_width must be greater than zero"
        );
        ensure!(
            config.graph_height >= 0,
            "cpu graph_height must not be negative"
        );
        ensure!(
            config.step_width > 0,
            "cpu step_width must be greater than zero"
        );
        ensure!(
            config.step_spacing >= 0,
            "cpu step_spacing must not be negative"
        );
        ensure!(config.text_gap >= 0, "cpu text_gap must not be negative");
        ensure!(
            config.graph_enabled || config.show_percent || !config.label.is_empty(),
            "cpu must show either graph or text"
        );
        config.style.validate()?;
        let _ = crate::cpu_popup::CpuPopupConfig::load()?;

        let graph_background = config::parse_rgba(&config.graph_background)?;
        let graph_low = config::parse_rgba(&config.graph_low)?;
        let graph_mid = config::parse_rgba(&config.graph_mid)?;
        let graph_high = config::parse_rgba(&config.graph_high)?;

        Ok(Self {
            config,
            previous: None,
            history: Vec::new(),
            graph_background,
            graph_low,
            graph_mid,
            graph_high,
            revision: 0,
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

    fn history_capacity(&self) -> usize {
        let step = self
            .config
            .step_width
            .saturating_add(self.config.step_spacing)
            .max(1);
        ((self.config.graph_width + self.config.step_spacing) / step)
            .max(1)
            .try_into()
            .unwrap_or(1)
    }

    fn push_history(&mut self, value: f32) {
        let capacity = self.history_capacity();
        if self.history.len() >= capacity {
            let remove = self.history.len() + 1 - capacity;
            self.history.drain(0..remove);
        }
        self.history.push(value.clamp(0.0, 100.0));
        self.revision = self.revision.wrapping_add(1);
    }

    fn format_text(&self, percent: Option<f32>) -> String {
        if !self.config.show_percent {
            return self.config.label.clone();
        }

        match (self.config.label.is_empty(), percent) {
            (true, Some(value)) => format!("{value:.0}%"),
            (true, None) => "--%".into(),
            (false, Some(value)) => format!("{} {value:.0}%", self.config.label),
            (false, None) => format!("{} --%", self.config.label),
        }
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
                (100.0 * (total_delta.saturating_sub(idle_delta)) as f64 / total_delta as f64)
                    as f32
            })
        });
        self.previous = Some(current);

        if let Some(value) = percent {
            self.push_history(value);
        }

        Ok(self.format_text(percent))
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Cpu(CpuVisual {
            history: self.history.clone(),
            graph_enabled: self.config.graph_enabled,
            graph_width: self.config.graph_width,
            graph_height: self.config.graph_height,
            step_width: self.config.step_width,
            step_spacing: self.config.step_spacing,
            text_gap: self.config.text_gap,
            warn_percent: self.config.warn_percent,
            graph_background: self.graph_background,
            graph_low: self.graph_low,
            graph_mid: self.graph_mid,
            graph_high: self.graph_high,
        })
    }
}

fn default_label() -> String {
    String::new()
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_warn_percent() -> f32 {
    85.0
}

fn default_graph_enabled() -> bool {
    true
}

fn default_graph_width() -> i32 {
    50
}

fn default_graph_height() -> i32 {
    0
}

fn default_step_width() -> i32 {
    2
}

fn default_step_spacing() -> i32 {
    1
}

fn default_text_gap() -> i32 {
    6
}

fn default_show_percent() -> bool {
    false
}

fn default_graph_background() -> String {
    "#00000000".into()
}

fn default_graph_low() -> String {
    "#F2F2F2".into()
}

fn default_graph_mid() -> String {
    "#FFFF00".into()
}

fn default_graph_high() -> String {
    "#FF0000".into()
}

#[cfg(test)]
mod tests {
    use super::CpuModule;

    #[test]
    fn history_is_bounded() {
        let mut module = CpuModule::load().expect("load cpu config");
        for value in 0..100 {
            module.push_history(value as f32);
        }
        assert!(module.history.len() <= module.history_capacity());
        assert_eq!(module.revision, 100);
    }
}
