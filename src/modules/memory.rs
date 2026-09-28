use std::{fs, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "memory";
pub const CONFIG_FILE: &str = "modules/memory.toml";

#[derive(Debug, Deserialize)]
struct MemoryConfig {
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_bar_width")]
    bar_width: i32,
    #[serde(default = "default_bar_height")]
    bar_height: i32,
    #[serde(default = "default_row_gap")]
    row_gap: i32,
    #[serde(default = "default_text_gap")]
    text_gap: i32,
    #[serde(default = "default_bar_background")]
    bar_background: String,
    #[serde(default = "default_memory_fill")]
    memory_fill: String,
    #[serde(default = "default_swap_fill")]
    swap_fill: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MemoryStats {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

impl MemoryStats {
    pub fn used_kib(self) -> u64 {
        self.total_kib.saturating_sub(self.available_kib)
    }

    pub fn swap_used_kib(self) -> u64 {
        self.swap_total_kib.saturating_sub(self.swap_free_kib)
    }

    pub fn memory_percent(self) -> f32 {
        percent(self.used_kib(), self.total_kib)
    }

    pub fn swap_percent(self) -> f32 {
        percent(self.swap_used_kib(), self.swap_total_kib)
    }
}

#[derive(Clone)]
pub struct MemoryVisual {
    pub memory_percent: f32,
    pub swap_percent: f32,
    pub bar_width: i32,
    pub bar_height: i32,
    pub row_gap: i32,
    pub text_gap: i32,
    pub bar_background: [u8; 4],
    pub memory_fill: [u8; 4],
    pub swap_fill: [u8; 4],
}

pub struct MemoryModule {
    config: MemoryConfig,
    stats: MemoryStats,
    bar_background: [u8; 4],
    memory_fill: [u8; 4],
    swap_fill: [u8; 4],
    revision: u64,
}

impl MemoryModule {
    pub fn load() -> Result<Self> {
        let config: MemoryConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "memory interval_ms must be greater than zero"
        );
        ensure!(
            config.bar_width > 0,
            "memory bar_width must be greater than zero"
        );
        ensure!(
            config.bar_height > 0,
            "memory bar_height must be greater than zero"
        );
        ensure!(config.row_gap >= 0, "memory row_gap must not be negative");
        ensure!(config.text_gap >= 0, "memory text_gap must not be negative");
        config.style.validate()?;
        let _ = crate::memory_popup::MemoryPopupConfig::load()?;

        let bar_background = config::parse_rgba(&config.bar_background)?;
        let memory_fill = config::parse_rgba(&config.memory_fill)?;
        let swap_fill = config::parse_rgba(&config.swap_fill)?;

        Ok(Self {
            config,
            stats: MemoryStats {
                total_kib: 0,
                available_kib: 0,
                swap_total_kib: 0,
                swap_free_kib: 0,
            },
            bar_background,
            memory_fill,
            swap_fill,
            revision: 0,
        })
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
        self.stats = read_stats()?;
        self.revision = self.revision.wrapping_add(1);
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Memory(MemoryVisual {
            memory_percent: self.stats.memory_percent(),
            swap_percent: self.stats.swap_percent(),
            bar_width: self.config.bar_width,
            bar_height: self.config.bar_height,
            row_gap: self.config.row_gap,
            text_gap: self.config.text_gap,
            bar_background: self.bar_background,
            memory_fill: self.memory_fill,
            swap_fill: self.swap_fill,
        })
    }
}

pub(crate) fn read_stats() -> Result<MemoryStats> {
    let content = fs::read_to_string("/proc/meminfo").context("failed to read /proc/meminfo")?;
    let mut total = None;
    let mut available = None;
    let mut swap_total = None;
    let mut swap_free = None;

    for line in content.lines() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("MemTotal:") => total = parse_value(fields.next()),
            Some("MemAvailable:") => available = parse_value(fields.next()),
            Some("SwapTotal:") => swap_total = parse_value(fields.next()),
            Some("SwapFree:") => swap_free = parse_value(fields.next()),
            _ => {}
        }
    }

    let stats = MemoryStats {
        total_kib: total.context("MemTotal missing from /proc/meminfo")?,
        available_kib: available.context("MemAvailable missing from /proc/meminfo")?,
        swap_total_kib: swap_total.unwrap_or(0),
        swap_free_kib: swap_free.unwrap_or(0),
    };
    ensure!(stats.total_kib > 0, "MemTotal must be greater than zero");
    Ok(stats)
}

fn parse_value(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.parse::<u64>().ok())
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (100.0 * used as f64 / total as f64) as f32
    }
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_bar_width() -> i32 {
    42
}

fn default_bar_height() -> i32 {
    4
}

fn default_row_gap() -> i32 {
    2
}

fn default_text_gap() -> i32 {
    3
}

fn default_bar_background() -> String {
    "#303030".into()
}

fn default_memory_fill() -> String {
    "#4EA1FF".into()
}

fn default_swap_fill() -> String {
    "#A56BFF".into()
}

#[cfg(test)]
mod tests {
    use super::read_stats;

    #[test]
    fn reads_memory_and_swap_stats() {
        let stats = read_stats().expect("memory stats");
        assert!(stats.total_kib > 0);
        assert!(stats.available_kib <= stats.total_kib);
        assert!(stats.swap_free_kib <= stats.swap_total_kib || stats.swap_total_kib == 0);
    }
}
