use std::{ffi::CString, mem::MaybeUninit, os::raw::c_char, ptr, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "clock";
pub const CONFIG_FILE: &str = "modules/clock.toml";

#[derive(Debug, Deserialize)]
struct ClockConfig {
    #[serde(default = "default_format")]
    format: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct ClockModule {
    config: ClockConfig,
}

impl ClockModule {
    pub fn load() -> Result<Self> {
        let config: ClockConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "clock interval_ms must be greater than zero"
        );
        ensure!(!config.format.is_empty(), "clock format must not be empty");
        config.style.validate()?;
        Ok(Self { config })
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
        let format = CString::new(self.config.format.as_str())
            .context("clock format must not contain NUL bytes")?;
        let timestamp = unsafe { libc::time(ptr::null_mut()) };
        ensure!(timestamp != -1, "failed to read system time");

        let mut tm = MaybeUninit::<libc::tm>::uninit();
        let result = unsafe { libc::localtime_r(&timestamp, tm.as_mut_ptr()) };
        ensure!(
            !result.is_null(),
            "failed to convert system time to local time"
        );
        let tm = unsafe { tm.assume_init() };

        let mut buffer = [0_u8; 256];
        let written = unsafe {
            libc::strftime(
                buffer.as_mut_ptr().cast::<c_char>(),
                buffer.len(),
                format.as_ptr(),
                &tm,
            )
        };
        ensure!(written > 0, "formatted clock text is empty or too long");

        Ok(String::from_utf8_lossy(&buffer[..written]).into_owned())
    }
}

fn default_format() -> String {
    "%Y-%m-%d %H:%M".into()
}

fn default_interval_ms() -> u64 {
    1_000
}
