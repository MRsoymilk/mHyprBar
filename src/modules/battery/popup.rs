use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct BatteryPopupConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_padding")]
    pub padding: i32,
    #[serde(default = "default_title_height")]
    pub title_height: i32,
    #[serde(default = "default_progress_height")]
    pub progress_height: i32,
    #[serde(default = "default_status_height")]
    pub status_height: i32,
    #[serde(default = "default_row_height")]
    pub row_height: i32,
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_bar_background")]
    pub bar_background: String,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct BatteryPopupFile {
    #[serde(default = "default_high_percent")]
    high_percent: f32,
    #[serde(default = "default_medium_percent")]
    medium_percent: f32,
    #[serde(default = "default_low_percent")]
    low_percent: f32,
    #[serde(default = "default_high_color")]
    high_color: String,
    #[serde(default = "default_medium_color")]
    medium_color: String,
    #[serde(default = "default_low_color")]
    low_color: String,
    #[serde(default = "default_critical_color")]
    critical_color: String,
    #[serde(default = "default_charging_color")]
    charging_color: String,
    #[serde(default)]
    popup: BatteryPopupConfig,
}

impl Default for BatteryPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            title_height: default_title_height(),
            progress_height: default_progress_height(),
            status_height: default_status_height(),
            row_height: default_row_height(),
            refresh_ms: default_refresh_ms(),
            bar_background: default_bar_background(),
            border: default_border(),
            separator: default_separator(),
            style: default_popup_style(),
        }
    }
}

impl BatteryPopupConfig {
    pub fn load() -> Result<Self> {
        let file: BatteryPopupFile = config::load_module("battery")?;
        let cfg = file.popup;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.width > 0,
            "battery popup width must be greater than zero"
        );
        ensure!(
            self.padding >= 0,
            "battery popup padding must not be negative"
        );
        ensure!(
            self.title_height > 0,
            "battery popup title_height must be greater than zero"
        );
        ensure!(
            self.progress_height > 0,
            "battery popup progress_height must be greater than zero"
        );
        ensure!(
            self.status_height > 0,
            "battery popup status_height must be greater than zero"
        );
        ensure!(
            self.row_height > 0,
            "battery popup row_height must be greater than zero"
        );
        ensure!(
            self.refresh_ms > 0,
            "battery popup refresh_ms must be greater than zero"
        );
        self.style.validate()?;
        let _ = config::parse_rgba(&self.bar_background)?;
        let _ = config::parse_rgba(&self.border)?;
        let _ = config::parse_rgba(&self.separator)?;
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
}

pub struct BatteryPopupModel {
    pub config: BatteryPopupConfig,
    pub stats: crate::modules::battery::BatteryStats,
    pub high_percent: f32,
    pub medium_percent: f32,
    pub low_percent: f32,
    pub high_color: [u8; 4],
    pub medium_color: [u8; 4],
    pub low_color: [u8; 4],
    pub critical_color: [u8; 4],
    pub charging_color: [u8; 4],
}

impl BatteryPopupModel {
    pub fn new() -> Result<Option<Self>> {
        let file: BatteryPopupFile = config::load_module("battery")?;
        file.popup.validate()?;

        let Some(stats) = crate::modules::battery::read_configured_stats()? else {
            return Ok(None);
        };

        Ok(Some(Self {
            config: file.popup,
            stats,
            high_percent: file.high_percent,
            medium_percent: file.medium_percent,
            low_percent: file.low_percent,
            high_color: config::parse_rgba(&file.high_color)?,
            medium_color: config::parse_rgba(&file.medium_color)?,
            low_color: config::parse_rgba(&file.low_color)?,
            critical_color: config::parse_rgba(&file.critical_color)?,
            charging_color: config::parse_rgba(&file.charging_color)?,
        }))
    }

    pub fn refresh(&mut self) -> Result<()> {
        if let Some(stats) = crate::modules::battery::read_configured_stats()? {
            self.stats = stats;
        }
        Ok(())
    }

    pub fn fill_color(&self) -> [u8; 4] {
        if self.stats.charging() {
            return self.charging_color;
        }
        if self.stats.capacity >= self.high_percent {
            self.high_color
        } else if self.stats.capacity >= self.medium_percent {
            self.medium_color
        } else if self.stats.capacity >= self.low_percent {
            self.low_color
        } else {
            self.critical_color
        }
    }

    pub fn panel_height(&self) -> i32 {
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(self.config.title_height)
            .saturating_add(self.config.progress_height)
            .saturating_add(8)
            .saturating_add(self.config.status_height)
            .saturating_add(1)
            .saturating_add(self.config.row_height.saturating_mul(5))
    }

    pub fn capacity_text(&self) -> String {
        format!("{:.0}%", self.stats.capacity)
    }

    pub fn time_remaining_text(&self) -> String {
        format_duration(self.stats.time_remaining_seconds())
    }

    pub fn status_detail_text(&self) -> String {
        let Some(seconds) = self.stats.time_remaining_seconds() else {
            return String::new();
        };
        let duration = format_duration(Some(seconds));
        if self.stats.charging() {
            format!("{duration} until full")
        } else if self.stats.status.eq_ignore_ascii_case("Discharging") {
            format!("{duration} remaining")
        } else {
            String::new()
        }
    }

    pub fn health_text(&self) -> String {
        self.stats
            .health_percent()
            .map(|value| format!("{value:.0}%"))
            .unwrap_or_else(|| "--".into())
    }

    pub fn design_capacity_text(&self) -> String {
        self.stats
            .energy_full_design_wh
            .map(|value| format!("{value:.1} Wh"))
            .unwrap_or_else(|| "--".into())
    }

    pub fn current_rate_text(&self) -> String {
        self.stats
            .power_now_w
            .map(|value| {
                let sign = if self.stats.charging() { "+" } else { "-" };
                format!("{sign}{value:.1} W")
            })
            .unwrap_or_else(|| "--".into())
    }
}

pub fn format_duration(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "--".into();
    };
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours} h {minutes:02} min")
    } else {
        format!("{minutes} min")
    }
}

fn default_enabled() -> bool {
    true
}

fn default_width() -> i32 {
    380
}

fn default_padding() -> i32 {
    14
}

fn default_title_height() -> i32 {
    32
}

fn default_progress_height() -> i32 {
    14
}

fn default_status_height() -> i32 {
    54
}

fn default_row_height() -> i32 {
    30
}

fn default_refresh_ms() -> u64 {
    5_000
}

fn default_bar_background() -> String {
    "#25282D".into()
}

fn default_border() -> String {
    "#3C414A".into()
}

fn default_separator() -> String {
    "#353A42".into()
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

fn default_high_percent() -> f32 {
    60.0
}

fn default_medium_percent() -> f32 {
    30.0
}

fn default_low_percent() -> f32 {
    10.0
}

fn default_high_color() -> String {
    "#56E36B".into()
}

fn default_medium_color() -> String {
    "#FFD84A".into()
}

fn default_low_color() -> String {
    "#FF8A25".into()
}

fn default_critical_color() -> String {
    "#FF365A".into()
}

fn default_charging_color() -> String {
    "#56E36B".into()
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn formats_remaining_time() {
        assert_eq!(format_duration(Some(5_040)), "1 h 24 min");
        assert_eq!(format_duration(Some(1_200)), "20 min");
        assert_eq!(format_duration(None), "--");
    }
}
