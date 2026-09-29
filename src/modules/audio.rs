use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "audio";
pub const CONFIG_FILE: &str = "modules/audio.toml";

const ICON_MUTED_SVG: &[u8] = include_bytes!("../../res/audio/volume-muted.svg");
const ICON_ZERO_SVG: &[u8] = include_bytes!("../../res/audio/volume-zero.svg");
const ICON_LOW_SVG: &[u8] = include_bytes!("../../res/audio/volume-low.svg");
const ICON_HIGH_SVG: &[u8] = include_bytes!("../../res/audio/volume-high.svg");

#[derive(Debug, Deserialize)]
struct AudioConfig {
    #[serde(default = "default_backend")]
    backend: String,
    #[serde(default = "default_target")]
    target: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_step_percent")]
    step_percent: u32,
    #[serde(default = "default_max_percent")]
    max_percent: u32,
    #[serde(default = "default_icon_size")]
    icon_size: i32,
    #[serde(default = "default_bar_width")]
    bar_width: i32,
    #[serde(default = "default_bar_height")]
    bar_height: i32,
    #[serde(default = "default_text_gap")]
    text_gap: i32,
    #[serde(default = "default_bar_background")]
    bar_background: String,
    #[serde(default = "default_fill")]
    fill: String,
    #[serde(default = "default_muted_fill")]
    muted_fill: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VolumeState {
    percent: u32,
    muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioBackend {
    Auto,
    PipeWire,
    PulseAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioIconKind {
    Muted,
    Zero,
    Low,
    High,
}

#[derive(Clone)]
struct AudioIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone)]
struct AudioIcons {
    muted: AudioIcon,
    zero: AudioIcon,
    low: AudioIcon,
    high: AudioIcon,
}

impl AudioIcons {
    fn load() -> Result<Self> {
        Ok(Self {
            muted: rasterize_svg("volume-muted.svg", ICON_MUTED_SVG)?,
            zero: rasterize_svg("volume-zero.svg", ICON_ZERO_SVG)?,
            low: rasterize_svg("volume-low.svg", ICON_LOW_SVG)?,
            high: rasterize_svg("volume-high.svg", ICON_HIGH_SVG)?,
        })
    }

    fn icon(&self, kind: AudioIconKind) -> AudioIcon {
        match kind {
            AudioIconKind::Muted => self.muted.clone(),
            AudioIconKind::Zero => self.zero.clone(),
            AudioIconKind::Low => self.low.clone(),
            AudioIconKind::High => self.high.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AudioVisual {
    pub percent: u32,
    pub muted: bool,
    pub icon_size: i32,
    pub icon_pixels: Arc<[u8]>,
    pub icon_width: i32,
    pub icon_height: i32,
    pub bar_width: i32,
    pub bar_height: i32,
    pub text_gap: i32,
    pub bar_background: [u8; 4],
    pub fill: [u8; 4],
    pub muted_fill: [u8; 4],
}

pub struct AudioModule {
    config: AudioConfig,
    backend: AudioBackend,
    state: VolumeState,
    icons: AudioIcons,
    bar_background: [u8; 4],
    fill: [u8; 4],
    muted_fill: [u8; 4],
    revision: u64,
}

impl AudioModule {
    pub fn load() -> Result<Self> {
        let config: AudioConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let backend = parse_backend(&config.backend)?;
        let state = VolumeState {
            percent: 0,
            muted: false,
        };
        let icons = AudioIcons::load()?;
        let bar_background = config::parse_rgba(&config.bar_background)?;
        let fill = config::parse_rgba(&config.fill)?;
        let muted_fill = config::parse_rgba(&config.muted_fill)?;

        Ok(Self {
            config,
            backend,
            state,
            icons,
            bar_background,
            fill,
            muted_fill,
            revision: 0,
        })
    }

    fn resolve_state(&mut self) -> Result<VolumeState> {
        if self.backend == AudioBackend::Auto {
            if let Ok(state) = read_pipewire_state(pipewire_target(&self.config.target)) {
                self.backend = AudioBackend::PipeWire;
                return Ok(state);
            }
            let state = read_pulse_state(pulse_target(&self.config.target))?;
            self.backend = AudioBackend::PulseAudio;
            return Ok(state);
        }

        match self.backend {
            AudioBackend::PipeWire => read_pipewire_state(pipewire_target(&self.config.target)),
            AudioBackend::PulseAudio => read_pulse_state(pulse_target(&self.config.target)),
            AudioBackend::Auto => unreachable!(),
        }
    }

    fn toggle_mute(&mut self) -> Result<bool> {
        if self.backend == AudioBackend::Auto {
            self.state = self.resolve_state()?;
        }

        match self.backend {
            AudioBackend::PipeWire => run_command(
                "wpctl",
                &["set-mute", pipewire_target(&self.config.target), "toggle"],
            )?,
            AudioBackend::PulseAudio => run_command(
                "pactl",
                &["set-sink-mute", pulse_target(&self.config.target), "toggle"],
            )?,
            AudioBackend::Auto => unreachable!(),
        }

        self.state = self.resolve_state()?;
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
    }

    fn adjust(&mut self, direction: i32) -> Result<bool> {
        if direction == 0 {
            return Ok(false);
        }
        if self.backend == AudioBackend::Auto {
            self.state = self.resolve_state()?;
        }

        let delta = self.config.step_percent as i32 * direction.signum();
        let next =
            (self.state.percent as i32 + delta).clamp(0, self.config.max_percent as i32) as u32;
        let volume = format!("{next}%");

        match self.backend {
            AudioBackend::PipeWire => run_command(
                "wpctl",
                &[
                    "set-volume",
                    pipewire_target(&self.config.target),
                    volume.as_str(),
                ],
            )?,
            AudioBackend::PulseAudio => run_command(
                "pactl",
                &[
                    "set-sink-volume",
                    pulse_target(&self.config.target),
                    volume.as_str(),
                ],
            )?,
            AudioBackend::Auto => unreachable!(),
        }

        self.state = self.resolve_state()?;
        self.revision = self.revision.wrapping_add(1);
        Ok(true)
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
        let next = self.resolve_state()?;
        if next != self.state {
            self.state = next;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(String::new())
    }

    fn visual(&self) -> ModuleVisual {
        let kind = icon_kind(self.state.percent, self.state.muted);
        let icon = self.icons.icon(kind);
        ModuleVisual::Audio(AudioVisual {
            percent: self.state.percent,
            muted: self.state.muted,
            icon_size: self.config.icon_size,
            icon_pixels: icon.pixels,
            icon_width: icon.width,
            icon_height: icon.height,
            bar_width: self.config.bar_width,
            bar_height: self.config.bar_height,
            text_gap: self.config.text_gap,
            bar_background: self.bar_background,
            fill: self.fill,
            muted_fill: self.muted_fill,
        })
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn activate(&mut self) -> Result<bool> {
        self.toggle_mute()
    }

    fn scroll(&mut self, direction: i32) -> Result<bool> {
        self.adjust(direction)
    }
}

fn icon_kind(percent: u32, muted: bool) -> AudioIconKind {
    if muted {
        AudioIconKind::Muted
    } else if percent == 0 {
        AudioIconKind::Zero
    } else if percent < 50 {
        AudioIconKind::Low
    } else {
        AudioIconKind::High
    }
}

fn rasterize_svg(name: &str, svg_bytes: &[u8]) -> Result<AudioIcon> {
    const RASTER_SIZE: i32 = 96;

    let mut svg = Command::new("rsvg-convert")
        .arg("--width")
        .arg(RASTER_SIZE.to_string())
        .arg("--height")
        .arg(RASTER_SIZE.to_string())
        .arg("--keep-aspect-ratio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch rsvg-convert for {name}"))?;

    let stdout = svg
        .stdout
        .take()
        .context("failed to capture rsvg-convert output")?;
    let mut svg_stdin = svg
        .stdin
        .take()
        .context("failed to open rsvg-convert stdin")?;

    let magick = Command::new("magick")
        .arg("png:-")
        .arg("rgba:-")
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to launch magick for {name}"))?;

    svg_stdin
        .write_all(svg_bytes)
        .with_context(|| format!("failed to feed SVG data for {name}"))?;
    drop(svg_stdin);

    let status = svg
        .wait()
        .with_context(|| format!("failed to wait for rsvg-convert for {name}"))?;
    let output = magick
        .wait_with_output()
        .with_context(|| format!("failed to wait for magick for {name}"))?;

    if !status.success() || !output.status.success() {
        bail!("audio SVG rasterization failed for {name}");
    }

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "audio SVG rasterizer returned {} bytes for {RASTER_SIZE}x{RASTER_SIZE}, expected {expected}: {name}",
        output.stdout.len()
    );

    crop_transparent_margin(name, &output.stdout, RASTER_SIZE, RASTER_SIZE)
}

fn crop_transparent_margin(
    name: &str,
    pixels: &[u8],
    width: i32,
    height: i32,
) -> Result<AudioIcon> {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = -1;
    let mut max_y = -1;

    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            if pixels[offset + 3] == 0 {
                continue;
            }
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    ensure!(
        max_x >= min_x && max_y >= min_y,
        "audio SVG has no visible pixels: {name}"
    );

    let cropped_width = max_x - min_x + 1;
    let cropped_height = max_y - min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);

    for y in min_y..=max_y {
        let start = ((y * width + min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&pixels[start..end]);
    }

    Ok(AudioIcon {
        pixels: Arc::from(cropped),
        width: cropped_width,
        height: cropped_height,
    })
}

fn validate_config(config: &AudioConfig) -> Result<()> {
    let _ = parse_backend(&config.backend)?;
    ensure!(
        config.interval_ms > 0,
        "audio interval_ms must be greater than zero"
    );
    ensure!(
        !config.target.trim().is_empty(),
        "audio target must not be empty"
    );
    ensure!(
        config.step_percent > 0 && config.step_percent <= 100,
        "audio step_percent must be in 1..=100"
    );
    ensure!(
        config.max_percent >= 100 && config.max_percent <= 200,
        "audio max_percent must be in 100..=200"
    );
    ensure!(
        (config.icon_size == 0 || config.icon_size > 4)
            && config.bar_width > 0
            && config.bar_height > 0,
        "audio icon_size must be 0 (auto) or greater than 4, and bar dimensions must be positive"
    );
    ensure!(config.text_gap >= 0, "audio text_gap must not be negative");
    config.style.validate()?;
    Ok(())
}

fn parse_backend(value: &str) -> Result<AudioBackend> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(AudioBackend::Auto),
        "pipewire" | "wpctl" => Ok(AudioBackend::PipeWire),
        "pulseaudio" | "pulse" | "pactl" => Ok(AudioBackend::PulseAudio),
        _ => bail!("audio backend must be auto, pipewire, or pulseaudio"),
    }
}

fn pipewire_target(target: &str) -> &str {
    if target == "auto" {
        "@DEFAULT_AUDIO_SINK@"
    } else {
        target
    }
}

fn pulse_target(target: &str) -> &str {
    if target == "auto" {
        "@DEFAULT_SINK@"
    } else {
        target
    }
}

fn read_pipewire_state(target: &str) -> Result<VolumeState> {
    let output = command_output("wpctl", &["get-volume", target])?;
    parse_wpctl_volume(&output)
}

fn read_pulse_state(target: &str) -> Result<VolumeState> {
    let volume = command_output("pactl", &["get-sink-volume", target])?;
    let mute = command_output("pactl", &["get-sink-mute", target])?;
    parse_pactl_state(&volume, &mute)
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
    let _ = command_output(program, args)?;
    Ok(())
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

fn parse_pactl_state(volume: &str, mute: &str) -> Result<VolumeState> {
    let percent = volume
        .split_whitespace()
        .find_map(|field| field.strip_suffix('%')?.parse::<u32>().ok())
        .context("pactl volume output is missing a percentage")?;
    let muted = match mute.split_once(':').map(|(_, value)| value.trim()) {
        Some("yes") => true,
        Some("no") => false,
        _ => bail!("unexpected pactl mute output"),
    };
    Ok(VolumeState { percent, muted })
}

fn default_backend() -> String {
    "auto".into()
}

fn default_target() -> String {
    "auto".into()
}

fn default_interval_ms() -> u64 {
    500
}

fn default_step_percent() -> u32 {
    5
}

fn default_max_percent() -> u32 {
    150
}

fn default_icon_size() -> i32 {
    0
}

fn default_bar_width() -> i32 {
    32
}

fn default_bar_height() -> i32 {
    4
}

fn default_text_gap() -> i32 {
    4
}

fn default_bar_background() -> String {
    "#303030".into()
}

fn default_fill() -> String {
    "#8AB4FF".into()
}

fn default_muted_fill() -> String {
    "#FF5C6C".into()
}

#[cfg(test)]
mod tests {
    use super::{AudioIconKind, VolumeState, icon_kind, parse_pactl_state, parse_wpctl_volume};

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

    #[test]
    fn chooses_svg_icon_by_volume_state() {
        assert_eq!(icon_kind(100, true), AudioIconKind::Muted);
        assert_eq!(icon_kind(0, false), AudioIconKind::Zero);
        assert_eq!(icon_kind(1, false), AudioIconKind::Low);
        assert_eq!(icon_kind(49, false), AudioIconKind::Low);
        assert_eq!(icon_kind(50, false), AudioIconKind::High);
        assert_eq!(icon_kind(150, false), AudioIconKind::High);
    }

    #[test]
    fn parses_pactl_volume_and_mute() {
        assert_eq!(
            parse_pactl_state(
                "Volume: front-left: 65536 / 100% / 0.00 dB, front-right: 65536 / 100% / 0.00 dB\n",
                "Mute: no\n",
            )
            .unwrap(),
            VolumeState {
                percent: 100,
                muted: false
            }
        );
    }
}
