use std::{ffi::CString, mem::MaybeUninit, os::raw::c_char, ptr, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "clock";
pub const CONFIG_FILE: &str = "modules/clock.toml";

#[derive(Debug, Deserialize)]
struct ClockConfig {
    #[serde(default)]
    format: Option<String>,
    #[serde(default = "default_time_format")]
    time_format: String,
    #[serde(default = "default_date_format")]
    date_format: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_time_font_size")]
    time_font_size: f32,
    #[serde(default = "default_date_font_size")]
    date_font_size: f32,
    #[serde(default = "default_row_gap")]
    row_gap: i32,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone, Debug)]
pub(crate) struct LocalDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
}

#[derive(Clone)]
pub struct ClockVisual {
    pub time_text: String,
    pub date_text: String,
    pub time_font_size: f32,
    pub date_font_size: f32,
    pub row_gap: i32,
}

pub struct ClockModule {
    config: ClockConfig,
    visual: ClockVisual,
    revision: u64,
}

impl ClockModule {
    pub fn load() -> Result<Self> {
        let config: ClockConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let _ = crate::clock_popup::ClockPopupConfig::load()?;

        let visual = ClockVisual {
            time_text: String::new(),
            date_text: String::new(),
            time_font_size: config.time_font_size,
            date_font_size: config.date_font_size,
            row_gap: config.row_gap,
        };
        Ok(Self {
            config,
            visual,
            revision: 0,
        })
    }
}

impl StatusModule for ClockModule {
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
        let tm = local_tm()?;
        if let Some(legacy) = self.config.format.as_deref()
            && self.config.time_format == default_time_format()
            && self.config.date_format == default_date_format()
        {
            self.visual.time_text = format_tm(&tm, legacy)?;
            self.visual.date_text.clear();
        } else {
            self.visual.time_text = format_tm(&tm, &self.config.time_format)?;
            self.visual.date_text = format_tm(&tm, &self.config.date_format)?;
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Clock(self.visual.clone())
    }
}

pub(crate) fn local_datetime() -> Result<LocalDateTime> {
    let tm = local_tm()?;
    Ok(LocalDateTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        weekday: tm.tm_wday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
    })
}

pub(crate) fn weekday_for_date(year: i32, month: u32, day: u32) -> Result<u32> {
    ensure!(
        (1..=12).contains(&month),
        "clock month must be between 1 and 12"
    );
    let mut tm = local_tm()?;
    tm.tm_year = year - 1900;
    tm.tm_mon = month as i32 - 1;
    tm.tm_mday = day as i32;
    tm.tm_hour = 12;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    let timestamp = unsafe { libc::mktime(&mut tm) };
    ensure!(timestamp != -1, "failed to normalize calendar date");
    Ok(tm.tm_wday as u32)
}

pub(crate) fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

pub(crate) fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    }
}

fn validate_config(config: &ClockConfig) -> Result<()> {
    ensure!(
        config.interval_ms > 0,
        "clock interval_ms must be greater than zero"
    );
    ensure!(
        !config.time_format.is_empty(),
        "clock time_format must not be empty"
    );
    ensure!(
        !config.date_format.is_empty(),
        "clock date_format must not be empty"
    );
    ensure!(
        config.time_font_size > 0.0 && config.date_font_size > 0.0,
        "clock font sizes must be greater than zero"
    );
    ensure!(config.row_gap >= 0, "clock row_gap must not be negative");
    config.style.validate()?;
    Ok(())
}

fn local_tm() -> Result<libc::tm> {
    let timestamp = unsafe { libc::time(ptr::null_mut()) };
    ensure!(timestamp != -1, "failed to read system time");
    let mut tm = MaybeUninit::<libc::tm>::uninit();
    let result = unsafe { libc::localtime_r(&timestamp, tm.as_mut_ptr()) };
    ensure!(
        !result.is_null(),
        "failed to convert system time to local time"
    );
    Ok(unsafe { tm.assume_init() })
}

fn format_tm(tm: &libc::tm, format: &str) -> Result<String> {
    let format = CString::new(format).context("clock format must not contain NUL bytes")?;
    let mut buffer = [0_u8; 256];
    let written = unsafe {
        libc::strftime(
            buffer.as_mut_ptr().cast::<c_char>(),
            buffer.len(),
            format.as_ptr(),
            tm,
        )
    };
    ensure!(written > 0, "formatted clock text is empty or too long");
    Ok(String::from_utf8_lossy(&buffer[..written]).into_owned())
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn default_time_format() -> String {
    "%H:%M".into()
}

fn default_date_format() -> String {
    "%a %d %b".into()
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_time_font_size() -> f32 {
    13.0
}

fn default_date_font_size() -> f32 {
    8.5
}

fn default_row_gap() -> i32 {
    0
}

#[cfg(test)]
mod tests {
    use super::{days_in_month, month_name};

    #[test]
    fn calendar_month_lengths_are_correct() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2026, 9), 30);
        assert_eq!(month_name(9), "September");
    }
}
