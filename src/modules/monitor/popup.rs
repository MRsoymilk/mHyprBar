use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::MissingMonitorWindow;
use crate::{
    config::{self, ModuleStyle},
    hyprland::{self, MonitorInfo},
};

#[derive(Clone, Debug, Deserialize)]
pub struct MonitorPopupConfig {
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
    #[serde(default = "default_control_height")]
    pub control_height: i32,
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_selected_background")]
    pub selected_background: String,
    #[serde(default = "default_hover_background")]
    pub hover_background: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct MonitorPopupFile {
    #[serde(default)]
    popup: MonitorPopupConfig,
}

impl Default for MonitorPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            title_height: default_title_height(),
            row_height: default_row_height(),
            control_height: default_control_height(),
            refresh_ms: default_refresh_ms(),
            border: default_border(),
            separator: default_separator(),
            selected_background: default_selected_background(),
            hover_background: default_hover_background(),
            style: default_popup_style(),
        }
    }
}

impl MonitorPopupConfig {
    pub fn load() -> Result<Self> {
        let file: MonitorPopupFile = config::load_module("monitor")?;
        let cfg = file.popup;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "monitor popup width must be positive");
        ensure!(
            self.padding >= 0,
            "monitor popup padding must not be negative"
        );
        ensure!(
            self.title_height > 0,
            "monitor popup title_height must be positive"
        );
        ensure!(
            self.row_height > 0,
            "monitor popup row_height must be positive"
        );
        ensure!(
            self.control_height > 0,
            "monitor popup control_height must be positive"
        );
        ensure!(
            self.refresh_ms > 0,
            "monitor popup refresh_ms must be positive"
        );
        self.style.validate()?;
        let _ = self.border_rgba()?;
        let _ = self.separator_rgba()?;
        let _ = self.selected_background_rgba()?;
        let _ = self.hover_background_rgba()?;
        Ok(())
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_ms)
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn separator_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.separator)
    }

    pub fn selected_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.selected_background)
    }

    pub fn hover_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.hover_background)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorPopupAction {
    Select(usize),
    RecallWindow(usize),
    Focus,
    ScaleDown,
    ScaleUp,
    AutoLeft,
    AutoUp,
    AutoDown,
    AutoRight,
    ModePrevious,
    ModeNext,
}

pub fn context_action(index: usize) -> Option<MonitorPopupAction> {
    [
        MonitorPopupAction::Focus,
        MonitorPopupAction::ScaleDown,
        MonitorPopupAction::ScaleUp,
        MonitorPopupAction::AutoLeft,
        MonitorPopupAction::AutoUp,
        MonitorPopupAction::AutoDown,
        MonitorPopupAction::AutoRight,
        MonitorPopupAction::ModePrevious,
        MonitorPopupAction::ModeNext,
    ]
    .get(index)
    .copied()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MonitorPopupMode {
    Displays,
    MissingWindows { target_monitor: String },
}

pub struct MonitorPopupModel {
    pub config: MonitorPopupConfig,
    pub monitors: Vec<MonitorInfo>,
    pub missing_windows: Vec<MissingMonitorWindow>,
    pub selected: usize,
    pub hovered_row: Option<usize>,
    pub context_row: Option<usize>,
    mode: MonitorPopupMode,
}

impl MonitorPopupModel {
    pub fn new() -> Result<Self> {
        let config = MonitorPopupConfig::load()?;
        let monitors = hyprland::monitor_infos()?;
        let selected = monitors
            .iter()
            .position(|monitor| monitor.focused)
            .unwrap_or(0)
            .min(monitors.len().saturating_sub(1));
        Ok(Self {
            config,
            monitors,
            missing_windows: Vec::new(),
            selected,
            hovered_row: None,
            context_row: None,
            mode: MonitorPopupMode::Displays,
        })
    }

    pub fn new_missing(
        target_monitor: String,
        mut missing_windows: Vec<MissingMonitorWindow>,
    ) -> Result<Self> {
        let config = MonitorPopupConfig::load()?;
        let monitors = hyprland::monitor_infos()?;
        let selected = monitors
            .iter()
            .position(|monitor| monitor.name == target_monitor)
            .unwrap_or(0)
            .min(monitors.len().saturating_sub(1));
        missing_windows.sort_by(|left, right| {
            left.origin_monitor
                .cmp(&right.origin_monitor)
                .then_with(|| left.workspace_id.cmp(&right.workspace_id))
                .then_with(|| left.class.cmp(&right.class))
                .then_with(|| left.title.cmp(&right.title))
        });
        Ok(Self {
            config,
            monitors,
            missing_windows,
            selected,
            hovered_row: None,
            context_row: None,
            mode: MonitorPopupMode::MissingWindows { target_monitor },
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let selected_name = self
            .monitors
            .get(self.selected)
            .map(|monitor| monitor.name.clone());
        self.monitors = hyprland::monitor_infos()?;
        self.selected = selected_name
            .as_deref()
            .and_then(|name| {
                self.monitors
                    .iter()
                    .position(|monitor| monitor.name == name)
            })
            .or_else(|| self.monitors.iter().position(|monitor| monitor.focused))
            .unwrap_or(0)
            .min(self.monitors.len().saturating_sub(1));
        if self
            .context_row
            .is_some_and(|index| index >= self.monitors.len())
        {
            self.context_row = None;
        }
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
                    .saturating_mul(self.row_count().max(1) as i32),
            )
    }

    pub fn is_missing_windows(&self) -> bool {
        matches!(self.mode, MonitorPopupMode::MissingWindows { .. })
    }

    pub fn title_text(&self) -> String {
        match &self.mode {
            MonitorPopupMode::Displays => format!("Displays · {}", self.monitors.len()),
            MonitorPopupMode::MissingWindows { target_monitor } => format!(
                "Disconnected displays · {} windows → {target_monitor}",
                self.missing_windows.len()
            ),
        }
    }

    fn row_count(&self) -> usize {
        if self.is_missing_windows() {
            self.missing_windows.len()
        } else {
            self.monitors.len()
        }
    }

    pub fn selected_monitor(&self) -> Option<&MonitorInfo> {
        self.monitors.get(self.selected)
    }

    pub fn row_at(&self, local_y: f64) -> Option<usize> {
        let start = self.config.padding + self.config.title_height + 1;
        if local_y < start as f64 {
            return None;
        }
        let index = ((local_y - start as f64) / self.config.row_height as f64) as usize;
        (index < self.row_count()).then_some(index)
    }

    pub fn open_context_at(&mut self, local_y: f64) -> bool {
        if self.is_missing_windows() {
            self.context_row = None;
            return false;
        }
        let Some(index) = self.row_at(local_y) else {
            self.context_row = None;
            return false;
        };
        self.selected = index;
        self.context_row = Some(index);
        true
    }

    pub fn action_at(&self, local_x: f64, local_y: f64) -> Option<MonitorPopupAction> {
        if let Some(index) = self.row_at(local_y) {
            return Some(if self.is_missing_windows() {
                MonitorPopupAction::RecallWindow(index)
            } else {
                MonitorPopupAction::Select(index)
            });
        }
        self.context_action_at(local_x, local_y)
    }

    pub fn context_action_at(
        &self,
        local_x: f64,
        local_y: f64,
    ) -> Option<MonitorPopupAction> {
        if self.is_missing_windows() {
            return None;
        }
        self.context_row?;
        let controls_y = self
            .config
            .padding
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(
                self.config
                    .row_height
                    .saturating_mul(self.monitors.len().max(1) as i32),
            )
            .saturating_add(1);
        if local_x < self.config.padding as f64
            || local_x >= (self.config.width - self.config.padding) as f64
            || local_y < controls_y as f64
        {
            return None;
        }
        let index =
            ((local_y - controls_y as f64) / self.config.control_height as f64) as usize;
        [
            MonitorPopupAction::Focus,
            MonitorPopupAction::ScaleDown,
            MonitorPopupAction::ScaleUp,
            MonitorPopupAction::AutoLeft,
            MonitorPopupAction::AutoUp,
            MonitorPopupAction::AutoDown,
            MonitorPopupAction::AutoRight,
            MonitorPopupAction::ModePrevious,
            MonitorPopupAction::ModeNext,
        ]
        .get(index)
        .copied()
    }

    pub fn apply_action(&mut self, action: MonitorPopupAction) -> Result<Option<String>> {
        match action {
            MonitorPopupAction::Select(index) => {
                if index < self.monitors.len() {
                    self.selected = index;
                }
                return Ok(None);
            }
            MonitorPopupAction::RecallWindow(index) => {
                let MonitorPopupMode::MissingWindows { target_monitor } = &self.mode else {
                    return Ok(None);
                };
                let Some(window) = self.missing_windows.get(index).cloned() else {
                    return Ok(None);
                };
                let monitor = self
                    .monitors
                    .iter()
                    .find(|monitor| monitor.name == *target_monitor)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("target monitor {target_monitor} is unavailable"))?;
                ensure!(
                    monitor.active_workspace > 0,
                    "target monitor {} has no active workspace",
                    monitor.name
                );
                hyprland::move_window_to_workspace(&window.address, monitor.active_workspace)?;
                hyprland::focus_monitor(&monitor.name)?;
                hyprland::switch_workspace(&monitor.name, monitor.active_workspace)?;
                hyprland::focus_window(&window.address)?;
                return Ok(Some(window.address));
            }
            _ => {}
        }

        let Some(monitor) = self.selected_monitor().cloned() else {
            return Ok(None);
        };

        match action {
            MonitorPopupAction::Focus => hyprland::focus_monitor(&monitor.name)?,
            MonitorPopupAction::ScaleDown => {
                let scale = ((monitor.scale - 0.25) * 4.0).round() / 4.0;
                hyprland::configure_monitor(
                    &monitor.name,
                    &monitor.mode_string(),
                    &monitor.position_string(),
                    scale.clamp(0.5, 4.0),
                )?;
            }
            MonitorPopupAction::ScaleUp => {
                let scale = ((monitor.scale + 0.25) * 4.0).round() / 4.0;
                hyprland::configure_monitor(
                    &monitor.name,
                    &monitor.mode_string(),
                    &monitor.position_string(),
                    scale.clamp(0.5, 4.0),
                )?;
            }
            MonitorPopupAction::AutoLeft => {
                hyprland::configure_monitor(
                    &monitor.name,
                    &monitor.mode_string(),
                    "auto-left",
                    monitor.scale,
                )?;
            }
            MonitorPopupAction::AutoUp => {
                hyprland::configure_monitor(
                    &monitor.name,
                    &monitor.mode_string(),
                    "auto-up",
                    monitor.scale,
                )?;
            }
            MonitorPopupAction::AutoDown => {
                hyprland::configure_monitor(
                    &monitor.name,
                    &monitor.mode_string(),
                    "auto-down",
                    monitor.scale,
                )?;
            }
            MonitorPopupAction::AutoRight => {
                hyprland::configure_monitor(
                    &monitor.name,
                    &monitor.mode_string(),
                    "auto-right",
                    monitor.scale,
                )?;
            }
            MonitorPopupAction::ModePrevious => {
                if let Some(mode) = adjacent_mode(&monitor, -1) {
                    hyprland::configure_monitor(
                        &monitor.name,
                        &mode,
                        &monitor.position_string(),
                        monitor.scale,
                    )?;
                }
            }
            MonitorPopupAction::ModeNext => {
                if let Some(mode) = adjacent_mode(&monitor, 1) {
                    hyprland::configure_monitor(
                        &monitor.name,
                        &mode,
                        &monitor.position_string(),
                        monitor.scale,
                    )?;
                }
            }
            MonitorPopupAction::Select(_) | MonitorPopupAction::RecallWindow(_) => {}
        }
        self.refresh()?;
        Ok(None)
    }

    pub fn primary_text(monitor: &MonitorInfo) -> String {
        let focused = if monitor.focused { " · focused" } else { "" };
        format!("{}{}  {}", monitor.name, focused, monitor.model)
    }

    pub fn detail_text(monitor: &MonitorInfo) -> String {
        format!(
            "{}×{} @ {:.0} Hz   scale {:.2}   pos {},{}   ws {}",
            monitor.width,
            monitor.height,
            monitor.refresh_rate,
            monitor.scale,
            monitor.x,
            monitor.y,
            monitor.active_workspace
        )
    }

    pub fn missing_primary_text(window: &MissingMonitorWindow) -> String {
        let class = if window.class.trim().is_empty() {
            "window"
        } else {
            window.class.trim()
        };
        format!("{} · {class}", window.origin_monitor)
    }

    pub fn missing_detail_text(window: &MissingMonitorWindow) -> String {
        let title = window.title.trim();
        if title.is_empty() {
            format!("workspace {}", window.workspace_id)
        } else {
            format!("ws {} · {title}", window.workspace_id)
        }
    }
}

fn adjacent_mode(monitor: &MonitorInfo, delta: i32) -> Option<String> {
    let mut modes = monitor
        .available_modes
        .iter()
        .map(|mode| normalize_mode(mode))
        .filter(|mode| !mode.is_empty())
        .collect::<Vec<_>>();
    modes.dedup();
    if modes.is_empty() {
        return None;
    }

    let current = normalize_mode(&monitor.mode_string());
    let index = modes
        .iter()
        .position(|mode| same_mode(mode, &current))
        .unwrap_or(0);
    let next = if delta < 0 {
        if index == 0 {
            modes.len() - 1
        } else {
            index - 1
        }
    } else {
        (index + 1) % modes.len()
    };
    Some(modes[next].clone())
}

fn normalize_mode(mode: &str) -> String {
    mode.trim().trim_end_matches("Hz").trim().to_owned()
}

fn same_mode(left: &str, right: &str) -> bool {
    let parse = |value: &str| {
        let (size, refresh) = value.split_once('@')?;
        let (width, height) = size.split_once('x')?;
        Some((
            width.parse::<i32>().ok()?,
            height.parse::<i32>().ok()?,
            refresh.parse::<f64>().ok()?,
        ))
    };

    match (parse(left), parse(right)) {
        (Some((lw, lh, lr)), Some((rw, rh, rr))) => lw == rw && lh == rh && (lr - rr).abs() < 0.1,
        _ => left == right,
    }
}

fn default_enabled() -> bool {
    true
}
fn default_width() -> i32 {
    560
}
fn default_padding() -> i32 {
    12
}
fn default_title_height() -> i32 {
    32
}
fn default_row_height() -> i32 {
    48
}
fn default_control_height() -> i32 {
    26
}
fn default_refresh_ms() -> u64 {
    1000
}
fn default_border() -> String {
    "#3C414A".into()
}
fn default_separator() -> String {
    "#353A42".into()
}
fn default_selected_background() -> String {
    "#203728".into()
}
fn default_hover_background() -> String {
    "#3A4048".into()
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
        MonitorPopupAction, MonitorPopupConfig, MonitorPopupMode, MonitorPopupModel, adjacent_mode,
        normalize_mode, same_mode,
    };
    use crate::{hyprland::MonitorInfo, modules::monitor::MissingMonitorWindow};

    #[test]
    fn mode_matching_ignores_hz_suffix_and_small_rounding() {
        assert_eq!(normalize_mode("1920x1080@60.00Hz"), "1920x1080@60.00");
        assert!(same_mode("1920x1080@60.00", "1920x1080@60.000"));
    }

    #[test]
    fn cycles_available_modes() {
        let monitor = MonitorInfo {
            id: 0,
            name: "DP-1".into(),
            description: String::new(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            x: 0,
            y: 0,
            scale: 1.0,
            focused: true,
            dpms_status: true,
            active_workspace: 1,
            available_modes: vec![
                "1920x1080@60.00Hz".into(),
                "1920x1080@99.93Hz".into(),
                "1280x720@60.00Hz".into(),
            ],
        };
        assert_eq!(
            adjacent_mode(&monitor, 1).as_deref(),
            Some("1920x1080@99.93")
        );
        assert_eq!(
            adjacent_mode(&monitor, -1).as_deref(),
            Some("1280x720@60.00")
        );
    }

    #[test]
    fn missing_window_row_maps_to_recall_action() {
        let mut model = MonitorPopupModel {
            config: MonitorPopupConfig::default(),
            monitors: Vec::new(),
            missing_windows: vec![MissingMonitorWindow {
                origin_monitor: "HDMI-A-1".into(),
                address: "0x1234".into(),
                class: "firefox".into(),
                title: "Docs".into(),
                workspace_id: 7,
            }],
            selected: 0,
            hovered_row: None,
            context_row: None,
            mode: MonitorPopupMode::MissingWindows {
                target_monitor: "eDP-1".into(),
            },
        };
        let y = (model.config.padding + model.config.title_height + 2) as f64;
        assert_eq!(
            model.action_at(model.config.padding as f64 + 1.0, y),
            Some(MonitorPopupAction::RecallWindow(0))
        );
        assert!(!model.open_context_at(y));
    }

    #[test]
    fn action_layout_maps_controls() {
        let model = MonitorPopupModel {
            config: MonitorPopupConfig::default(),
            monitors: Vec::new(),
            missing_windows: Vec::new(),
            selected: 0,
            hovered_row: None,
            context_row: Some(0),
            mode: MonitorPopupMode::Displays,
        };
        let controls_y =
            model.config.padding + model.config.title_height + 1 + model.config.row_height + 1;
        let x = model.config.padding as f64 + 10.0;
        let row_h = model.config.control_height as f64;
        assert_eq!(
            model.context_action_at(x, controls_y as f64 + 5.0),
            Some(MonitorPopupAction::Focus)
        );
        assert_eq!(
            model.context_action_at(x, controls_y as f64 + row_h * 6.5),
            Some(MonitorPopupAction::AutoRight)
        );
        assert_eq!(
            model.context_action_at(x, controls_y as f64 + row_h * 8.5),
            Some(MonitorPopupAction::ModeNext)
        );
    }
}
