use std::{ffi::CString, mem::MaybeUninit, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "disk";
pub const CONFIG_FILE: &str = "modules/disk.toml";

#[derive(Debug, Deserialize)]
struct DiskConfig {
    #[serde(default = "default_mount")]
    mount: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct DiskModule {
    config: DiskConfig,
}

impl DiskModule {
    pub fn load() -> Result<Self> {
        let config: DiskConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "disk interval_ms must be greater than zero"
        );
        ensure!(!config.mount.is_empty(), "disk mount must not be empty");
        config.style.validate()?;
        Ok(Self { config })
    }
}

impl StatusModule for DiskModule {
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
        let mount = CString::new(self.config.mount.as_str())
            .context("disk mount must not contain NUL bytes")?;
        let mut stats = MaybeUninit::<libc::statvfs>::uninit();
        let rc = unsafe { libc::statvfs(mount.as_ptr(), stats.as_mut_ptr()) };
        ensure!(rc == 0, "statvfs failed for {}", self.config.mount);
        let stats = unsafe { stats.assume_init() };

        let total = stats.f_blocks as u128 * stats.f_frsize as u128;
        let available = stats.f_bavail as u128 * stats.f_frsize as u128;
        ensure!(total > 0, "filesystem {} has zero size", self.config.mount);
        let used = total.saturating_sub(available);
        let percent = 100.0 * used as f64 / total as f64;

        Ok(format!("{} {:.0}%", self.config.mount, percent))
    }
}

fn default_mount() -> String {
    "/".into()
}

fn default_interval_ms() -> u64 {
    5_000
}
