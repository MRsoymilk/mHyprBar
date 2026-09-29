use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::{
    config::{self, ModuleStyle},
    modules::clock::{LocalDateTime, days_in_month, local_datetime, month_name, weekday_for_date},
};

#[derive(Clone, Debug, Deserialize)]
pub struct ClockPopupConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_padding")]
    pub padding: i32,
    #[serde(default = "default_header_height")]
    pub header_height: i32,
    #[serde(default = "default_weekday_height")]
    pub weekday_height: i32,
    #[serde(default = "default_cell_height")]
    pub cell_height: i32,
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_today_background")]
    pub today_background: String,
    #[serde(default = "default_today_foreground")]
    pub today_foreground: String,
    #[serde(default = "default_adjacent_foreground")]
    pub adjacent_foreground: String,
    #[serde(default = "default_weekday_foreground")]
    pub weekday_foreground: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct ClockPopupFile {
    #[serde(default)]
    popup: ClockPopupConfig,
}

impl Default for ClockPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            header_height: default_header_height(),
            weekday_height: default_weekday_height(),
            cell_height: default_cell_height(),
            refresh_ms: default_refresh_ms(),
            border: default_border(),
            separator: default_separator(),
            today_background: default_today_background(),
            today_foreground: default_today_foreground(),
            adjacent_foreground: default_adjacent_foreground(),
            weekday_foreground: default_weekday_foreground(),
            style: default_popup_style(),
        }
    }
}

impl ClockPopupConfig {
    pub fn load() -> Result<Self> {
        let file: ClockPopupFile = config::load_module("clock")?;
        let cfg = file.popup;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.width > 0,
            "clock popup width must be greater than zero"
        );
        ensure!(
            self.padding >= 0,
            "clock popup padding must not be negative"
        );
        ensure!(
            self.header_height > 0,
            "clock popup header_height must be greater than zero"
        );
        ensure!(
            self.weekday_height > 0,
            "clock popup weekday_height must be greater than zero"
        );
        ensure!(
            self.cell_height > 0,
            "clock popup cell_height must be greater than zero"
        );
        ensure!(
            self.refresh_ms > 0,
            "clock popup refresh_ms must be greater than zero"
        );
        self.style.validate()?;
        let _ = config::parse_rgba(&self.border)?;
        let _ = config::parse_rgba(&self.separator)?;
        let _ = config::parse_rgba(&self.today_background)?;
        let _ = config::parse_rgba(&self.today_foreground)?;
        let _ = config::parse_rgba(&self.adjacent_foreground)?;
        let _ = config::parse_rgba(&self.weekday_foreground)?;
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

    pub fn today_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.today_background)
    }
}

#[derive(Clone, Debug)]
pub struct CalendarCell {
    pub day: u32,
    pub in_month: bool,
    pub today: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockPopupAction {
    Previous,
    Today,
    Next,
}

pub struct ClockPopupModel {
    pub config: ClockPopupConfig,
    pub now: LocalDateTime,
    pub view_year: i32,
    pub view_month: u32,
    pub cells: Vec<CalendarCell>,
}

impl ClockPopupModel {
    pub fn new() -> Result<Self> {
        let config = ClockPopupConfig::load()?;
        let now = local_datetime()?;
        let view_year = now.year;
        let view_month = now.month;
        let cells = build_calendar(view_year, view_month, &now)?;
        Ok(Self {
            config,
            now,
            view_year,
            view_month,
            cells,
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.now = local_datetime()?;
        self.rebuild_calendar()
    }

    pub fn navigate_month(&mut self, delta: i32) -> Result<()> {
        if delta == 0 {
            return Ok(());
        }
        let absolute = self
            .view_year
            .saturating_mul(12)
            .saturating_add(self.view_month as i32 - 1)
            .saturating_add(delta);
        self.view_year = absolute.div_euclid(12);
        self.view_month = (absolute.rem_euclid(12) + 1) as u32;
        self.rebuild_calendar()
    }

    pub fn reset_to_current_month(&mut self) -> Result<()> {
        self.view_year = self.now.year;
        self.view_month = self.now.month;
        self.rebuild_calendar()
    }

    pub fn apply_action(&mut self, action: ClockPopupAction) -> Result<()> {
        match action {
            ClockPopupAction::Previous => self.navigate_month(-1),
            ClockPopupAction::Today => self.reset_to_current_month(),
            ClockPopupAction::Next => self.navigate_month(1),
        }
    }

    pub fn header_action_at(&self, x: f64, y: f64) -> Option<ClockPopupAction> {
        let top = self.config.padding as f64;
        let bottom = top + 26.0;
        if y < top || y >= bottom {
            return None;
        }

        let left = self.config.padding as f64;
        let right = (self.config.width - self.config.padding) as f64;
        let button_width = 30.0;
        if x >= left && x < left + button_width {
            Some(ClockPopupAction::Previous)
        } else if x >= right - button_width && x < right {
            Some(ClockPopupAction::Next)
        } else if x >= left + button_width && x < right - button_width {
            Some(ClockPopupAction::Today)
        } else {
            None
        }
    }

    fn rebuild_calendar(&mut self) -> Result<()> {
        self.cells = build_calendar(self.view_year, self.view_month, &self.now)?;
        Ok(())
    }

    pub fn panel_height(&self) -> i32 {
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(self.config.header_height)
            .saturating_add(1)
            .saturating_add(self.config.weekday_height)
            .saturating_add(self.config.cell_height.saturating_mul(6))
    }

    pub fn month_title(&self) -> String {
        format!("{} {}", month_name(self.view_month), self.view_year)
    }

    pub fn date_summary(&self) -> String {
        format!(
            "{}, {} {}",
            weekday_name(self.now.weekday),
            self.now.day,
            month_name(self.now.month)
        )
    }

    pub fn time_text(&self) -> String {
        format!("{:02}:{:02}", self.now.hour, self.now.minute)
    }
}

fn build_calendar(year: i32, month: u32, now: &LocalDateTime) -> Result<Vec<CalendarCell>> {
    let first_sunday_based = weekday_for_date(year, month, 1)?;
    let first_monday_based = (first_sunday_based + 6) % 7;
    let current_days = days_in_month(year, month);

    let (prev_year, prev_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    let prev_days = days_in_month(prev_year, prev_month);

    let mut cells = Vec::with_capacity(42);
    for index in 0..42_u32 {
        if index < first_monday_based {
            let day = prev_days - first_monday_based + index + 1;
            cells.push(CalendarCell {
                day,
                in_month: false,
                today: false,
            });
            continue;
        }

        let current_index = index - first_monday_based + 1;
        if current_index <= current_days {
            cells.push(CalendarCell {
                day: current_index,
                in_month: true,
                today: year == now.year && month == now.month && current_index == now.day,
            });
        } else {
            cells.push(CalendarCell {
                day: current_index - current_days,
                in_month: false,
                today: false,
            });
        }
    }
    Ok(cells)
}

fn weekday_name(weekday: u32) -> &'static str {
    match weekday {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        6 => "Saturday",
        _ => "",
    }
}

fn default_enabled() -> bool {
    true
}

fn default_width() -> i32 {
    322
}

fn default_padding() -> i32 {
    14
}

fn default_header_height() -> i32 {
    48
}

fn default_weekday_height() -> i32 {
    24
}

fn default_cell_height() -> i32 {
    32
}

fn default_refresh_ms() -> u64 {
    60_000
}

fn default_border() -> String {
    "#3C414A".into()
}

fn default_separator() -> String {
    "#353A42".into()
}

fn default_today_background() -> String {
    "#4D7DFF".into()
}

fn default_today_foreground() -> String {
    "#FFFFFF".into()
}

fn default_adjacent_foreground() -> String {
    "#5F6670".into()
}

fn default_weekday_foreground() -> String {
    "#9198A4".into()
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
    use super::{ClockPopupModel, build_calendar};
    use crate::modules::clock::LocalDateTime;

    fn sample_now() -> LocalDateTime {
        LocalDateTime {
            year: 2026,
            month: 9,
            day: 28,
            weekday: 1,
            hour: 18,
            minute: 3,
        }
    }

    #[test]
    fn calendar_has_42_cells_and_marks_today() {
        let now = sample_now();
        let cells = build_calendar(2026, 9, &now).expect("calendar");
        assert_eq!(cells.len(), 42);
        assert!(cells.iter().any(|cell| cell.today && cell.day == 28));

        let october = build_calendar(2026, 10, &now).expect("october");
        assert!(!october.iter().any(|cell| cell.today));
    }

    #[test]
    fn month_navigation_wraps_year_boundaries() {
        let mut model = ClockPopupModel {
            config: Default::default(),
            now: sample_now(),
            view_year: 2026,
            view_month: 12,
            cells: Vec::new(),
        };
        model.navigate_month(1).expect("next month");
        assert_eq!((model.view_year, model.view_month), (2027, 1));
        model.navigate_month(-2).expect("previous months");
        assert_eq!((model.view_year, model.view_month), (2026, 11));
        model.reset_to_current_month().expect("current month");
        assert_eq!((model.view_year, model.view_month), (2026, 9));
    }
}
