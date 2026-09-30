use std::{collections::BTreeMap, fs, process::Command, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct CpuPopupConfig {
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
struct CpuPopupFile {
    #[serde(default = "default_warn_percent")]
    warn_percent: f32,
    #[serde(default = "default_graph_low")]
    graph_low: String,
    #[serde(default = "default_graph_mid")]
    graph_mid: String,
    #[serde(default = "default_graph_high")]
    graph_high: String,
    #[serde(default)]
    popup: CpuPopupConfig,
}

impl Default for CpuPopupConfig {
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
            border: default_border(),
            separator: default_separator(),
            hover_background: default_hover(),
            style: default_popup_style(),
        }
    }
}

impl CpuPopupConfig {
    pub fn load() -> Result<Self> {
        Ok(load_popup_file()?.popup)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "cpu popup width must be greater than zero");
        ensure!(
            self.row_height > 0,
            "cpu popup row_height must be greater than zero"
        );
        ensure!(self.padding >= 0, "cpu popup padding must not be negative");
        ensure!(
            self.refresh_ms > 0,
            "cpu popup refresh_ms must be greater than zero"
        );
        ensure!(
            self.max_processes > 0,
            "cpu popup max_processes must be greater than zero"
        );
        ensure!(
            self.bar_width > 0,
            "cpu popup bar_width must be greater than zero"
        );
        self.style.validate()?;
        let _ = config::parse_rgba(&self.bar_background)?;
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
pub struct CpuCoreRow {
    pub name: String,
    pub usage: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct CpuProcessRow {
    pub pid: u32,
    pub cpu: f32,
    pub name: String,
    pub executable: String,
    pub working_dir: String,
    pub command_line: String,
    pub parent_pid: Option<u32>,
    pub state: Option<String>,
    pub threads: Option<u32>,
}

pub struct CpuPopupModel {
    pub config: CpuPopupConfig,
    pub warn_percent: f32,
    pub graph_low: [u8; 4],
    pub graph_mid: [u8; 4],
    pub graph_high: [u8; 4],
    pub cores: Vec<CpuCoreRow>,
    pub processes: Vec<CpuProcessRow>,
    pub hovered_process: Option<usize>,
    previous: BTreeMap<u32, (u64, u64)>,
}

impl CpuPopupModel {
    pub fn new() -> Result<Self> {
        let file = load_popup_file()?;
        let graph_low = config::parse_rgba(&file.graph_low)?;
        let graph_mid = config::parse_rgba(&file.graph_mid)?;
        let graph_high = config::parse_rgba(&file.graph_high)?;
        let warn_percent = file.warn_percent;
        let config = file.popup;
        let previous = read_core_counters()?;
        let processes = read_processes(config.max_processes)?;
        let cores = previous
            .keys()
            .map(|index| CpuCoreRow {
                name: format!("CPU{index}"),
                usage: None,
            })
            .collect();
        Ok(Self {
            config,
            warn_percent,
            graph_low,
            graph_mid,
            graph_high,
            cores,
            processes,
            hovered_process: None,
            previous,
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let current = read_core_counters()?;
        self.cores = current
            .iter()
            .map(|(index, &(total, idle))| {
                let usage = self.previous.get(index).and_then(|&(old_total, old_idle)| {
                    let total_delta = total.saturating_sub(old_total);
                    let idle_delta = idle.saturating_sub(old_idle);
                    (total_delta > 0).then(|| {
                        (100.0 * (total_delta.saturating_sub(idle_delta)) as f64
                            / total_delta as f64) as f32
                    })
                });
                CpuCoreRow {
                    name: format!("CPU{index}"),
                    usage,
                }
            })
            .collect();
        self.previous = current;
        self.processes = read_processes(self.config.max_processes)?;
        Ok(())
    }

    pub fn panel_height(&self) -> i32 {
        let header_rows = 2_i32;
        let separator = 1_i32;
        let rows = self.cores.len() as i32 + self.processes.len() as i32 + header_rows;
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(rows.saturating_mul(self.config.row_height))
            .saturating_add(separator)
    }

    pub fn process_area_start(&self) -> i32 {
        self.config
            .padding
            .saturating_add((self.cores.len() as i32 + 2).saturating_mul(self.config.row_height))
            .saturating_add(1)
    }

    pub fn process_at(&self, y: f64) -> Option<usize> {
        let start = self.process_area_start();
        if y < start as f64 {
            return None;
        }
        let idx = ((y - start as f64) / self.config.row_height as f64) as usize;
        (idx < self.processes.len()).then_some(idx)
    }

    pub fn process_tooltip_text(&self, index: usize) -> Option<String> {
        let process = self.processes.get(index)?;
        let ppid = process
            .parent_pid
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());
        let state = process.state.as_deref().unwrap_or("-");
        let threads = process
            .threads
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());
        Some(format!(
            "{}  ·  PID {}  ·  CPU {:.1}%\nExecutable: {}\nWorking dir: {}\nArguments: {}\nPPID: {}  ·  State: {}  ·  Threads: {}",
            process.name,
            process.pid,
            process.cpu,
            process.executable,
            process.working_dir,
            process.command_line,
            ppid,
            state,
            threads,
        ))
    }
}

fn load_popup_file() -> Result<CpuPopupFile> {
    let file: CpuPopupFile = config::load_module("cpu")?;
    file.popup.validate()?;
    ensure!(
        (0.0..=100.0).contains(&file.warn_percent),
        "cpu warn_percent must be between 0 and 100"
    );
    let _ = config::parse_rgba(&file.graph_low)?;
    let _ = config::parse_rgba(&file.graph_mid)?;
    let _ = config::parse_rgba(&file.graph_high)?;
    Ok(file)
}

fn read_core_counters() -> Result<BTreeMap<u32, (u64, u64)>> {
    let content = fs::read_to_string("/proc/stat").context("failed to read /proc/stat")?;
    let mut result = BTreeMap::new();

    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        if !name.starts_with("cpu") || name == "cpu" {
            continue;
        }
        let suffix = &name[3..];
        let Ok(index) = suffix.parse::<u32>() else {
            continue;
        };

        let values: Vec<u64> = fields
            .take(10)
            .map(|value| value.parse::<u64>())
            .collect::<std::result::Result<_, _>>()
            .context("invalid per-core /proc/stat counter")?;
        if values.len() < 4 {
            continue;
        }
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        let total = values.iter().copied().sum();
        result.insert(index, (total, idle));
    }

    ensure!(!result.is_empty(), "no per-core CPU counters found");
    Ok(result)
}

fn read_processes(limit: usize) -> Result<Vec<CpuProcessRow>> {
    let output = Command::new("ps")
        .args(["-eo", "pid=,pcpu=,comm=", "--sort=-pcpu"])
        .output()
        .context("failed to launch ps for CPU popup")?;
    ensure!(output.status.success(), "ps failed for CPU popup");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in stdout.lines() {
        if rows.len() >= limit {
            break;
        }
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(cpu) = fields.next().and_then(|value| value.parse::<f32>().ok()) else {
            continue;
        };
        let name = fields.collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            continue;
        }
        let details = read_process_details(pid, &name);
        rows.push(CpuProcessRow {
            pid,
            cpu,
            name,
            executable: details.executable,
            working_dir: details.working_dir,
            command_line: details.command_line,
            parent_pid: details.parent_pid,
            state: details.state,
            threads: details.threads,
        });
    }
    Ok(rows)
}

struct CpuProcessDetails {
    executable: String,
    working_dir: String,
    command_line: String,
    parent_pid: Option<u32>,
    state: Option<String>,
    threads: Option<u32>,
}

fn read_process_details(pid: u32, fallback_name: &str) -> CpuProcessDetails {
    let executable = fs::read_link(format!("/proc/{pid}/exe"))
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "-".into());
    let working_dir = fs::read_link(format!("/proc/{pid}/cwd"))
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "-".into());

    let command_line = fs::read(format!("/proc/{pid}/cmdline"))
        .ok()
        .map(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback_name.to_owned());
    let command_line = truncate_chars(&command_line, 800);

    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok();
    let parent_pid = status
        .as_deref()
        .and_then(|text| status_value(text, "PPid:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u32>().ok());
    let state = status
        .as_deref()
        .and_then(|text| status_value(text, "State:"))
        .map(str::to_owned);
    let threads = status
        .as_deref()
        .and_then(|text| status_value(text, "Threads:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u32>().ok());

    CpuProcessDetails {
        executable,
        working_dir,
        command_line,
        parent_pid,
        state,
        threads,
    }
}

fn status_value<'a>(status: &'a str, key: &str) -> Option<&'a str> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key).map(str::trim))
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut truncated: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

fn default_enabled() -> bool {
    true
}

fn default_width() -> i32 {
    360
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
    150
}

fn default_bar_background() -> String {
    "#303030".into()
}

fn default_warn_percent() -> f32 {
    85.0
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
    use super::{CpuPopupModel, read_core_counters};

    #[test]
    fn reads_per_core_counters() {
        assert!(!read_core_counters().expect("cores").is_empty());
    }

    #[test]
    fn popup_height_is_positive() {
        let model = CpuPopupModel::new().expect("popup");
        assert!(model.panel_height() > 0);
    }
}
