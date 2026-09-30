use std::{fs, process::Command, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct MemoryPopupConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_row_height")]
    pub row_height: i32,
    #[serde(default = "default_padding")]
    pub padding: i32,
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_max_processes")]
    pub max_processes: usize,
    #[serde(default = "default_bar_width")]
    pub bar_width: i32,
    #[serde(default = "default_bar_background")]
    pub bar_background: String,
    #[serde(default = "default_memory_fill")]
    pub memory_fill: String,
    #[serde(default = "default_swap_fill")]
    pub swap_fill: String,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_hover")]
    pub hover_background: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct MemoryPopupFile {
    #[serde(default)]
    popup: MemoryPopupConfig,
}

impl Default for MemoryPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            row_height: default_row_height(),
            padding: default_padding(),
            refresh_ms: default_refresh_ms(),
            max_processes: default_max_processes(),
            bar_width: default_bar_width(),
            bar_background: default_bar_background(),
            memory_fill: default_memory_fill(),
            swap_fill: default_swap_fill(),
            border: default_border(),
            separator: default_separator(),
            hover_background: default_hover(),
            style: default_popup_style(),
        }
    }
}

impl MemoryPopupConfig {
    pub fn load() -> Result<Self> {
        let file: MemoryPopupFile = config::load_module("memory")?;
        let cfg = file.popup;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.width > 0,
            "memory popup width must be greater than zero"
        );
        ensure!(
            self.row_height > 0,
            "memory popup row_height must be greater than zero"
        );
        ensure!(
            self.padding >= 0,
            "memory popup padding must not be negative"
        );
        ensure!(
            self.refresh_ms > 0,
            "memory popup refresh_ms must be greater than zero"
        );
        ensure!(
            self.max_processes > 0,
            "memory popup max_processes must be greater than zero"
        );
        ensure!(
            self.bar_width > 0,
            "memory popup bar_width must be greater than zero"
        );
        self.style.validate()?;
        let _ = config::parse_rgba(&self.bar_background)?;
        let _ = config::parse_rgba(&self.memory_fill)?;
        let _ = config::parse_rgba(&self.swap_fill)?;
        let _ = config::parse_rgba(&self.border)?;
        let _ = config::parse_rgba(&self.separator)?;
        let _ = config::parse_rgba(&self.hover_background)?;
        Ok(())
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_ms)
    }

    pub fn bar_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_background)
    }

    pub fn memory_fill_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.memory_fill)
    }

    pub fn swap_fill_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.swap_fill)
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn separator_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.separator)
    }

    pub fn hover_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.hover_background)
    }
}

#[derive(Clone, Debug)]
pub struct MemoryProcessRow {
    pub pid: u32,
    pub rss_kib: u64,
    pub swap_kib: u64,
    pub percent: f32,
    pub swap_percent: f32,
    pub name: String,
}

pub struct MemoryPopupModel {
    pub config: MemoryPopupConfig,
    pub stats: crate::modules::memory::MemoryStats,
    pub processes: Vec<MemoryProcessRow>,
    pub hovered_process: Option<usize>,
}

impl MemoryPopupModel {
    pub fn new() -> Result<Self> {
        let config = MemoryPopupConfig::load()?;
        let stats = crate::modules::memory::read_stats()?;
        let processes =
            read_processes(config.max_processes, stats.total_kib, stats.swap_used_kib())?;
        Ok(Self {
            config,
            stats,
            processes,
            hovered_process: None,
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.stats = crate::modules::memory::read_stats()?;
        self.processes = read_processes(
            self.config.max_processes,
            self.stats.total_kib,
            self.stats.swap_used_kib(),
        )?;
        Ok(())
    }

    pub fn panel_height(&self) -> i32 {
        let summary_rows = 2_i32;
        let header_rows = 1_i32;
        let separator = 1_i32;
        let rows = summary_rows + header_rows + self.processes.len() as i32;
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(rows.saturating_mul(self.config.row_height))
            .saturating_add(separator)
    }

    pub fn process_area_start(&self) -> i32 {
        self.config
            .padding
            .saturating_add(2_i32.saturating_mul(self.config.row_height))
            .saturating_add(1)
            .saturating_add(self.config.row_height)
    }

    pub fn process_at(&self, y: f64) -> Option<usize> {
        let start = self.process_area_start();
        if y < start as f64 {
            return None;
        }
        let idx = ((y - start as f64) / self.config.row_height as f64) as usize;
        (idx < self.processes.len()).then_some(idx)
    }
}

pub fn format_kib(kib: u64) -> String {
    if kib >= 1024 * 1024 {
        format!("{:.1}G", kib as f64 / (1024.0 * 1024.0))
    } else if kib >= 1024 {
        format!("{:.0}M", kib as f64 / 1024.0)
    } else {
        format!("{kib}K")
    }
}

fn read_processes(
    limit: usize,
    total_kib: u64,
    swap_used_kib: u64,
) -> Result<Vec<MemoryProcessRow>> {
    let output = Command::new("ps")
        .args(["-eo", "pid=,rss=,comm="])
        .output()
        .context("failed to launch ps for memory popup")?;
    ensure!(output.status.success(), "ps failed for memory popup");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in stdout.lines() {
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(rss_kib) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        let name = fields.collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            continue;
        }

        let swap_kib = read_process_swap_kib(pid).unwrap_or(0);
        let percent = percent_of(rss_kib, total_kib);
        let swap_percent = percent_of(swap_kib, swap_used_kib);
        rows.push(MemoryProcessRow {
            pid,
            rss_kib,
            swap_kib,
            percent,
            swap_percent,
            name,
        });
    }

    rows.sort_by(|a, b| {
        b.rss_kib
            .saturating_add(b.swap_kib)
            .cmp(&a.rss_kib.saturating_add(a.swap_kib))
            .then_with(|| b.swap_kib.cmp(&a.swap_kib))
            .then_with(|| b.rss_kib.cmp(&a.rss_kib))
    });
    rows.truncate(limit);
    Ok(rows)
}

fn read_process_swap_kib(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == "VmSwap:")
            .then(|| fields.next()?.parse::<u64>().ok())
            .flatten()
    })
}

fn percent_of(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (100.0 * used as f64 / total as f64) as f32
    }
}

fn default_enabled() -> bool {
    true
}

fn default_width() -> i32 {
    430
}

fn default_row_height() -> i32 {
    24
}

fn default_padding() -> i32 {
    10
}

fn default_refresh_ms() -> u64 {
    1_000
}

fn default_max_processes() -> usize {
    10
}

fn default_bar_width() -> i32 {
    92
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

fn default_border() -> String {
    "#626262".into()
}

fn default_separator() -> String {
    "#626262".into()
}

fn default_hover() -> String {
    "#3A3A3AF0".into()
}

fn default_popup_style() -> ModuleStyle {
    ModuleStyle {
        foreground: "#F2F2F2".into(),
        background: "#202020F2".into(),
        font_family: "sans-serif".into(),
        font_size: 12.0,
        padding_x: 0,
        padding_y: 0,
        min_width: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryPopupModel;

    #[test]
    fn popup_contains_at_most_configured_processes() {
        let model = MemoryPopupModel::new().expect("memory popup");
        assert!(model.processes.len() <= model.config.max_processes);
        assert!(model.panel_height() > 0);
    }
}
