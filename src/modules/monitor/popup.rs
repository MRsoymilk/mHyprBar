use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{
    MissingMonitorWindow,
    settings::{DisplaySettings, Layout},
};
use crate::{
    config::{self, ModuleStyle},
    hyprland,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MonitorPopupConfig {
    pub enabled: bool,
    pub width: i32,
    pub padding: i32,
    pub title_height: i32,
    pub row_height: i32,
    pub control_height: i32,
    pub refresh_ms: u64,
    pub border: String,
    pub separator: String,
    pub selected_background: String,
    pub hover_background: String,
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
            enabled: true,
            width: 560,
            padding: 12,
            title_height: 32,
            row_height: 48,
            control_height: 26,
            refresh_ms: 1000,
            border: "#3C414A".into(),
            separator: "#353A42".into(),
            selected_background: "#203728".into(),
            hover_background: "#3A4048".into(),
            style: ModuleStyle {
                foreground: "#F2F2F2".into(),
                background: "#17191DEB".into(),
                font_family: "sans-serif".into(),
                font_size: 12.0,
                padding_x: 0,
                padding_y: 0,
                min_width: 0,
            },
        }
    }
}

impl MonitorPopupConfig {
    pub fn load() -> Result<Self> {
        let file: MonitorPopupFile = config::load_module("monitor")?;
        let cfg = file.popup;
        ensure!(cfg.width > 0, "monitor popup width must be positive");
        ensure!(
            cfg.padding >= 0,
            "monitor popup padding must not be negative"
        );
        ensure!(
            cfg.title_height > 0,
            "monitor popup title_height must be positive"
        );
        ensure!(
            cfg.row_height > 0,
            "monitor popup row_height must be positive"
        );
        ensure!(
            cfg.control_height > 0,
            "monitor popup control_height must be positive"
        );
        ensure!(
            cfg.refresh_ms > 0,
            "monitor popup refresh_ms must be positive"
        );
        cfg.style.validate()?;
        let _ = cfg.border_rgba()?;
        let _ = cfg.separator_rgba()?;
        let _ = cfg.selected_background_rgba()?;
        let _ = cfg.hover_background_rgba()?;
        Ok(cfg)
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

pub struct MonitorPopupModel {
    pub config: MonitorPopupConfig,
    pub settings: Option<DisplaySettings>,
    pub missing_windows: Vec<MissingMonitorWindow>,
    pub hovered_row: Option<usize>,
    target_monitor: Option<String>,
}

impl MonitorPopupModel {
    pub fn new() -> Result<Self> {
        Ok(Self {
            config: MonitorPopupConfig::load()?,
            settings: Some(DisplaySettings::new(hyprland::monitor_infos()?)),
            missing_windows: Vec::new(),
            hovered_row: None,
            target_monitor: None,
        })
    }

    pub fn new_missing(
        target_monitor: String,
        mut missing_windows: Vec<MissingMonitorWindow>,
    ) -> Result<Self> {
        missing_windows.sort_by(|left, right| {
            left.origin_monitor
                .cmp(&right.origin_monitor)
                .then_with(|| left.workspace_id.cmp(&right.workspace_id))
                .then_with(|| left.class.cmp(&right.class))
                .then_with(|| left.title.cmp(&right.title))
        });
        Ok(Self {
            config: MonitorPopupConfig::load()?,
            settings: None,
            missing_windows,
            hovered_row: None,
            target_monitor: Some(target_monitor),
        })
    }

    pub fn display_layout(&self) -> Layout {
        Layout::new(
            self.config.width,
            self.config.padding,
            self.config.control_height,
        )
    }

    pub fn panel_height(&self) -> i32 {
        if self.settings.is_some() {
            return self.display_layout().height;
        }
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(
                self.config
                    .row_height
                    .saturating_mul(self.missing_windows.len().max(1) as i32),
            )
    }

    pub fn title_text(&self) -> String {
        format!(
            "Disconnected displays · {} windows → {}",
            self.missing_windows.len(),
            self.target_monitor.as_deref().unwrap_or_default()
        )
    }

    pub fn row_at(&self, local_y: f64) -> Option<usize> {
        let start = self.config.padding + self.config.title_height + 1;
        if local_y < f64::from(start) {
            return None;
        }
        let index = ((local_y - f64::from(start)) / f64::from(self.config.row_height)) as usize;
        (index < self.missing_windows.len()).then_some(index)
    }

    pub fn recall_window(&self, row: usize) -> Result<Option<String>> {
        let (Some(target_monitor), Some(window)) =
            (&self.target_monitor, self.missing_windows.get(row))
        else {
            return Ok(None);
        };
        let monitor = hyprland::monitor_infos()?
            .into_iter()
            .find(|monitor| monitor.name == *target_monitor)
            .with_context(|| format!("target monitor {target_monitor} is unavailable"))?;
        ensure!(
            monitor.active_workspace > 0,
            "target monitor {} has no active workspace",
            monitor.name
        );
        hyprland::move_window_to_workspace(&window.address, monitor.active_workspace)?;
        hyprland::focus_monitor(&monitor.name)?;
        hyprland::switch_workspace(&monitor.name, monitor.active_workspace)?;
        hyprland::focus_window(&window.address)?;
        Ok(Some(window.address.clone()))
    }

    pub fn missing_primary_text(window: &MissingMonitorWindow) -> String {
        let class = window.class.trim();
        format!(
            "{} · {}",
            window.origin_monitor,
            if class.is_empty() { "window" } else { class }
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_window_row_maps_to_recall_action() {
        let model = MonitorPopupModel {
            config: MonitorPopupConfig::default(),
            settings: None,
            missing_windows: vec![MissingMonitorWindow {
                origin_monitor: "HDMI-A-1".into(),
                address: "0x1234".into(),
                class: "firefox".into(),
                title: "Docs".into(),
                workspace_id: 7,
            }],
            hovered_row: None,
            target_monitor: Some("eDP-1".into()),
        };
        let y = f64::from(model.config.padding + model.config.title_height + 2);
        assert_eq!(model.row_at(y), Some(0));
        assert_eq!(model.row_at(y + f64::from(model.config.row_height)), None);
    }
}
