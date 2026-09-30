use std::{collections::HashMap, process::Command, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct AudioPopupConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_padding")]
    pub padding: i32,
    #[serde(default = "default_title_height")]
    pub title_height: i32,
    #[serde(default = "default_row_height")]
    pub row_height: i32,
    #[serde(default = "default_mute_width")]
    pub mute_width: i32,
    #[serde(default = "default_slider_width")]
    pub slider_width: i32,
    #[serde(default = "default_slider_height")]
    pub slider_height: i32,
    #[serde(default = "default_refresh_interval_ms")]
    refresh_interval_ms: u64,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_active_background")]
    pub active_background: String,
    #[serde(default = "default_muted")]
    pub muted: String,
    #[serde(default = "default_bar_background")]
    pub bar_background: String,
    #[serde(default = "default_bar_fill")]
    pub bar_fill: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct AudioPopupFile {
    #[serde(default = "default_max_percent")]
    max_percent: u32,
    #[serde(default)]
    popup: AudioPopupConfig,
}

impl Default for AudioPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            title_height: default_title_height(),
            row_height: default_row_height(),
            mute_width: default_mute_width(),
            slider_width: default_slider_width(),
            slider_height: default_slider_height(),
            refresh_interval_ms: default_refresh_interval_ms(),
            border: default_border(),
            separator: default_separator(),
            active_background: default_active_background(),
            muted: default_muted(),
            bar_background: default_bar_background(),
            bar_fill: default_bar_fill(),
            style: default_popup_style(),
        }
    }
}

impl AudioPopupConfig {
    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "audio popup width must be positive");
        ensure!(
            self.padding >= 0,
            "audio popup padding must not be negative"
        );
        ensure!(
            self.title_height > 0,
            "audio popup title_height must be positive"
        );
        ensure!(
            self.row_height > 0,
            "audio popup row_height must be positive"
        );
        ensure!(
            self.mute_width > 0,
            "audio popup mute_width must be positive"
        );
        ensure!(
            self.slider_width > 0 && self.slider_height > 0,
            "audio popup slider dimensions must be positive"
        );
        ensure!(
            self.refresh_interval_ms > 0,
            "audio popup refresh_interval_ms must be positive"
        );
        self.style.validate()?;
        let _ = self.border_rgba()?;
        let _ = self.separator_rgba()?;
        let _ = self.active_background_rgba()?;
        let _ = self.muted_rgba()?;
        let _ = self.bar_background_rgba()?;
        let _ = self.bar_fill_rgba()?;
        Ok(())
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_interval_ms)
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn separator_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.separator)
    }

    pub fn active_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.active_background)
    }

    pub fn muted_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.muted)
    }

    pub fn bar_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_background)
    }

    pub fn bar_fill_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_fill)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioOutputBackend {
    PulseAudio,
    PipeWire,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioOutputRow {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub percent: u32,
    pub muted: bool,
    pub is_default: bool,
    pub backend: AudioOutputBackend,
}

#[derive(Clone, Copy, Debug)]
pub struct AudioRowGeometry {
    pub row_y: i32,
    pub row_h: i32,
    pub mute_x: i32,
    pub mute_y: i32,
    pub mute_w: i32,
    pub mute_h: i32,
    pub text_x: i32,
    pub text_w: i32,
    pub slider_x: i32,
    pub slider_y: i32,
    pub slider_w: i32,
    pub slider_h: i32,
    pub percent_x: i32,
    pub percent_w: i32,
}

pub struct AudioPopupModel {
    pub config: AudioPopupConfig,
    pub outputs: Vec<AudioOutputRow>,
    pub max_percent: u32,
}

impl AudioPopupModel {
    pub fn new() -> Result<Self> {
        let file: AudioPopupFile = config::load_module("audio")?;
        file.popup.validate()?;
        ensure!(file.max_percent > 0, "audio max_percent must be positive");
        let outputs = read_outputs()?;
        Ok(Self {
            config: file.popup,
            outputs,
            max_percent: file.max_percent,
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.outputs = read_outputs()?;
        Ok(())
    }

    pub fn panel_height(&self) -> i32 {
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(
                self.config
                    .row_height
                    .saturating_mul(self.outputs.len().max(1) as i32),
            )
    }

    pub fn row_geometry(&self, index: usize) -> Option<AudioRowGeometry> {
        if index >= self.outputs.len() {
            return None;
        }
        let row_y = self
            .config
            .padding
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(self.config.row_height.saturating_mul(index as i32));
        let row_h = self.config.row_height;
        let inset = 6;
        let mute_x = self.config.padding.saturating_add(inset);
        let mute_w = self.config.mute_width.min((self.config.width / 4).max(1));
        let mute_h = (row_h - 18).clamp(20, 28);
        let mute_y = row_y + (row_h - mute_h) / 2;
        let percent_w = 48;
        let percent_x = self
            .config
            .width
            .saturating_sub(self.config.padding)
            .saturating_sub(inset)
            .saturating_sub(percent_w);
        let max_slider_w = percent_x
            .saturating_sub(8)
            .saturating_sub(mute_x.saturating_add(mute_w))
            .saturating_sub(90)
            .max(48);
        let slider_w = self.config.slider_width.min(max_slider_w).max(48);
        let slider_x = percent_x.saturating_sub(8).saturating_sub(slider_w);
        let slider_h = self.config.slider_height.max(1);
        let slider_y = row_y + (row_h - slider_h) / 2;
        let text_x = mute_x.saturating_add(mute_w).saturating_add(10);
        let text_w = slider_x.saturating_sub(10).saturating_sub(text_x).max(1);

        Some(AudioRowGeometry {
            row_y,
            row_h,
            mute_x,
            mute_y,
            mute_w,
            mute_h,
            text_x,
            text_w,
            slider_x,
            slider_y,
            slider_w,
            slider_h,
            percent_x,
            percent_w,
        })
    }

    pub fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        if x < self.config.padding as f64 || x >= (self.config.width - self.config.padding) as f64 {
            return None;
        }
        let rows_y = self.config.padding + self.config.title_height + 1;
        let local_y = y - rows_y as f64;
        if local_y < 0.0 {
            return None;
        }
        let index = (local_y / self.config.row_height as f64).floor() as usize;
        (index < self.outputs.len()).then_some(index)
    }

    pub fn mute_at(&self, x: f64, y: f64) -> Option<usize> {
        let index = self.row_at(x, y)?;
        let g = self.row_geometry(index)?;
        (x >= g.mute_x as f64
            && x < (g.mute_x + g.mute_w) as f64
            && y >= g.mute_y as f64
            && y < (g.mute_y + g.mute_h) as f64)
            .then_some(index)
    }

    pub fn volume_at(&self, x: f64, y: f64) -> Option<(usize, u32)> {
        let index = self.row_at(x, y)?;
        let g = self.row_geometry(index)?;
        let hit_top = g.slider_y.saturating_sub(8);
        let hit_bottom = g.slider_y.saturating_add(g.slider_h).saturating_add(8);
        if y < hit_top as f64 || y >= hit_bottom as f64 {
            return None;
        }
        if x < g.slider_x as f64 || x > (g.slider_x + g.slider_w) as f64 {
            return None;
        }
        Some((index, self.volume_percent_for_x(index, x)?))
    }

    pub fn volume_percent_for_x(&self, index: usize, x: f64) -> Option<u32> {
        let g = self.row_geometry(index)?;
        let ratio = ((x - g.slider_x as f64) / g.slider_w.max(1) as f64).clamp(0.0, 1.0);
        Some((ratio * self.max_percent as f64).round() as u32)
    }

    pub fn toggle_mute(&mut self, index: usize) -> Result<()> {
        let (backend, id) = self
            .outputs
            .get(index)
            .map(|row| (row.backend, row.id.clone()))
            .ok_or_else(|| anyhow::anyhow!("audio output row {index} is unavailable"))?;
        match backend {
            AudioOutputBackend::PulseAudio => {
                run_command("pactl", &["set-sink-mute", &id, "toggle"])?
            }
            AudioOutputBackend::PipeWire => run_command("wpctl", &["set-mute", &id, "toggle"])?,
        }
        if let Some(row) = self.outputs.get_mut(index) {
            row.muted = !row.muted;
        }
        Ok(())
    }

    pub fn set_volume(&mut self, index: usize, percent: u32) -> Result<bool> {
        let percent = percent.min(self.max_percent);
        let (backend, id, current) = self
            .outputs
            .get(index)
            .map(|row| (row.backend, row.id.clone(), row.percent))
            .ok_or_else(|| anyhow::anyhow!("audio output row {index} is unavailable"))?;
        if current == percent {
            return Ok(false);
        }

        match backend {
            AudioOutputBackend::PulseAudio => {
                let volume = format!("{percent}%");
                run_command("pactl", &["set-sink-volume", &id, &volume])?;
            }
            AudioOutputBackend::PipeWire => {
                let volume = format!("{:.3}", percent as f64 / 100.0);
                run_command("wpctl", &["set-volume", &id, &volume])?;
            }
        }
        if let Some(row) = self.outputs.get_mut(index) {
            row.percent = percent;
        }
        Ok(true)
    }
}

#[derive(Debug, Deserialize)]
struct PactlSink {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    volume: HashMap<String, PactlVolume>,
    #[serde(default)]
    active_port: String,
    #[serde(default)]
    ports: Vec<PactlPort>,
    #[serde(default)]
    properties: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct PactlVolume {
    #[serde(default)]
    value_percent: String,
}

#[derive(Debug, Deserialize)]
struct PactlPort {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    availability: String,
}

fn read_outputs() -> Result<Vec<AudioOutputRow>> {
    read_pactl_outputs().or_else(|pulse_error| {
        read_wpctl_outputs().with_context(|| {
            format!("failed to enumerate audio outputs with pactl ({pulse_error:#}) and wpctl")
        })
    })
}

fn read_pactl_outputs() -> Result<Vec<AudioOutputRow>> {
    let default_sink = command_output("pactl", &["get-default-sink"])
        .unwrap_or_default()
        .trim()
        .to_owned();
    let json = command_output("pactl", &["-f", "json", "list", "sinks"])?;
    parse_pactl_sinks(&json, &default_sink)
}

fn parse_pactl_sinks(json: &str, default_sink: &str) -> Result<Vec<AudioOutputRow>> {
    let sinks: Vec<PactlSink> = serde_json::from_str(json).context("invalid pactl sink JSON")?;
    let mut outputs = sinks
        .into_iter()
        .map(|sink| {
            let percent = sink
                .volume
                .values()
                .filter_map(|volume| parse_percent(&volume.value_percent))
                .max()
                .unwrap_or(0);
            let port = sink
                .ports
                .iter()
                .find(|port| port.name == sink.active_port)
                .or_else(|| {
                    sink.ports
                        .iter()
                        .find(|port| port.availability.eq_ignore_ascii_case("available"))
                });
            let port_name = port
                .map(|port| port.description.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or("");
            let state = sink.state.trim().to_ascii_lowercase();
            let detail = if port_name.is_empty() {
                state
            } else if state.is_empty() {
                port_name.to_owned()
            } else {
                format!("{port_name} · {state}")
            };
            let fallback_name = sink
                .properties
                .get("device.description")
                .map(String::as_str)
                .unwrap_or("");
            let name = if !sink.description.trim().is_empty() {
                sink.description.trim().to_owned()
            } else if !fallback_name.trim().is_empty() {
                fallback_name.trim().to_owned()
            } else if !sink.name.trim().is_empty() {
                sink.name.trim().to_owned()
            } else {
                format!("Audio output {}", sink.index)
            };
            AudioOutputRow {
                id: sink.name.clone(),
                name,
                detail,
                percent,
                muted: sink.mute,
                is_default: !default_sink.is_empty() && sink.name == default_sink,
                backend: AudioOutputBackend::PulseAudio,
            }
        })
        .collect::<Vec<_>>();

    outputs.sort_by(|a, b| {
        b.is_default.cmp(&a.is_default).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    Ok(outputs)
}

fn read_wpctl_outputs() -> Result<Vec<AudioOutputRow>> {
    let status = command_output("wpctl", &["status", "-n"])?;
    let mut in_sinks = false;
    let mut outputs = Vec::new();

    for raw_line in status.lines() {
        let trimmed = raw_line.trim();
        if trimmed.ends_with("Sinks:") {
            in_sinks = true;
            continue;
        }
        if in_sinks
            && (trimmed.ends_with("Sources:")
                || trimmed.ends_with("Filters:")
                || trimmed.ends_with("Streams:")
                || trimmed.ends_with("Devices:"))
        {
            break;
        }
        if !in_sinks {
            continue;
        }

        let normalized = trimmed.trim_start_matches(['│', '├', '└', '─', ' ']).trim();
        if normalized.is_empty() {
            continue;
        }
        let (is_default, normalized) = if let Some(value) = normalized.strip_prefix('*') {
            (true, value.trim())
        } else {
            (false, normalized)
        };
        let Some((id, rest)) = normalized.split_once('.') else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }

        let name = rest
            .split_once("[vol:")
            .map(|(name, _)| name.trim())
            .unwrap_or_else(|| rest.trim())
            .to_owned();
        let state = command_output("wpctl", &["get-volume", id])
            .ok()
            .and_then(|value| parse_wpctl_volume(&value))
            .unwrap_or((0, false));
        outputs.push(AudioOutputRow {
            id: id.to_owned(),
            name: if name.is_empty() {
                format!("Audio output {id}")
            } else {
                name
            },
            detail: "PipeWire sink".into(),
            percent: state.0,
            muted: state.1,
            is_default,
            backend: AudioOutputBackend::PipeWire,
        });
    }

    if outputs.is_empty() {
        bail!("wpctl reported no audio sinks");
    }
    Ok(outputs)
}

fn parse_wpctl_volume(value: &str) -> Option<(u32, bool)> {
    let raw = value.split_whitespace().nth(1)?.parse::<f64>().ok()?;
    if !raw.is_finite() || raw < 0.0 {
        return None;
    }
    Some((
        (raw * 100.0).round().clamp(0.0, u32::MAX as f64) as u32,
        value.contains("[MUTED]"),
    ))
}

fn parse_percent(value: &str) -> Option<u32> {
    value.trim().strip_suffix('%')?.trim().parse().ok()
}

fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {program}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{program} command failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_command(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {program}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{program} command failed: {}", stderr.trim());
    }
    Ok(())
}

fn default_enabled() -> bool {
    true
}
fn default_width() -> i32 {
    560
}
fn default_padding() -> i32 {
    10
}
fn default_title_height() -> i32 {
    30
}
fn default_row_height() -> i32 {
    54
}
fn default_mute_width() -> i32 {
    48
}
fn default_slider_width() -> i32 {
    160
}
fn default_slider_height() -> i32 {
    5
}
fn default_max_percent() -> u32 {
    150
}
fn default_refresh_interval_ms() -> u64 {
    1000
}
fn default_border() -> String {
    "#3C414A".into()
}
fn default_separator() -> String {
    "#353A42".into()
}
fn default_active_background() -> String {
    "#26384A".into()
}
fn default_muted() -> String {
    "#FF5C6C".into()
}
fn default_bar_background() -> String {
    "#30343A".into()
}
fn default_bar_fill() -> String {
    "#8AB4FF".into()
}
fn default_popup_style() -> ModuleStyle {
    ModuleStyle {
        foreground: "#F2F2F2".into(),
        background: "#17191DEB".into(),
        font_family: "sans-serif".into(),
        font_size: 12.0,
        padding_x: 0,
        padding_y: 0,
        min_width: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioOutputBackend, AudioOutputRow, AudioPopupConfig, AudioPopupModel, parse_pactl_sinks,
        parse_wpctl_volume,
    };

    fn interactive_model() -> AudioPopupModel {
        AudioPopupModel {
            config: AudioPopupConfig::default(),
            outputs: vec![AudioOutputRow {
                id: "sink.test".into(),
                name: "Test output".into(),
                detail: "Test sink".into(),
                percent: 50,
                muted: false,
                is_default: true,
                backend: AudioOutputBackend::PulseAudio,
            }],
            max_percent: 150,
        }
    }

    #[test]
    fn popup_controls_hit_same_audio_row() {
        let model = interactive_model();
        let geometry = model.row_geometry(0).unwrap();
        assert_eq!(
            model.mute_at(
                (geometry.mute_x + geometry.mute_w / 2) as f64,
                (geometry.mute_y + geometry.mute_h / 2) as f64
            ),
            Some(0)
        );
        assert_eq!(
            model.volume_at(
                geometry.slider_x as f64,
                (geometry.slider_y + geometry.slider_h / 2) as f64
            ),
            Some((0, 0))
        );
        assert_eq!(
            model.volume_at(
                (geometry.slider_x + geometry.slider_w) as f64,
                (geometry.slider_y + geometry.slider_h / 2) as f64
            ),
            Some((0, 150))
        );
    }

    #[test]
    fn parses_wpctl_sink_volume() {
        assert_eq!(parse_wpctl_volume("Volume: 0.72\n"), Some((72, false)));
        assert_eq!(
            parse_wpctl_volume("Volume: 1.05 [MUTED]\n"),
            Some((105, true))
        );
    }

    #[test]
    fn parses_pactl_sink_rows() {
        let json = r#"[
          {
            "index": 1,
            "name": "alsa_output.test",
            "description": "Built-in Audio",
            "state": "RUNNING",
            "mute": false,
            "volume": {
              "front-left": {"value_percent":"48%"},
              "front-right": {"value_percent":"48%"}
            },
            "active_port": "analog-output-headphones",
            "ports": [
              {
                "name":"analog-output-headphones",
                "description":"Headphones",
                "availability":"available"
              }
            ],
            "properties": {}
          }
        ]"#;
        let rows = parse_pactl_sinks(json, "alsa_output.test").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].percent, 48);
        assert!(rows[0].is_default);
        assert_eq!(rows[0].name, "Built-in Audio");
        assert!(rows[0].detail.contains("Headphones"));
    }
}
