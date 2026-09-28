use std::{ffi::CString, mem::MaybeUninit, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "disk";
pub const CONFIG_FILE: &str = "modules/disk.toml";

#[derive(Debug, Deserialize)]
struct DiskConfig {
    #[serde(default)]
    mount: Option<String>,
    #[serde(default)]
    mounts: Vec<String>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_bar_width")]
    bar_width: i32,
    #[serde(default = "default_bar_height")]
    bar_height: i32,
    #[serde(default = "default_bar_border_width")]
    bar_border_width: i32,
    #[serde(default = "default_bar_background")]
    bar_background: String,
    #[serde(default = "default_bar_fill")]
    bar_fill: String,
    #[serde(default = "default_bar_border")]
    bar_border: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone, Debug)]
pub(crate) struct DiskStats {
    pub mount: String,
    pub total_bytes: u128,
    pub available_bytes: u128,
}

impl DiskStats {
    pub fn used_bytes(&self) -> u128 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }

    pub fn percent(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (100.0 * self.used_bytes() as f64 / self.total_bytes as f64) as f32
        }
    }
}

#[derive(Clone)]
pub struct DiskVisual {
    pub percent: f32,
    pub bar_width: i32,
    pub bar_height: i32,
    pub bar_border_width: i32,
    pub bar_background: [u8; 4],
    pub bar_fill: [u8; 4],
    pub bar_border: [u8; 4],
}

pub struct DiskModule {
    config: DiskConfig,
    mounts: Vec<String>,
    stats: DiskStats,
    bar_background: [u8; 4],
    bar_fill: [u8; 4],
    bar_border: [u8; 4],
    revision: u64,
}

impl DiskModule {
    pub fn load() -> Result<Self> {
        let config: DiskConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "disk interval_ms must be greater than zero"
        );
        ensure!(
            config.bar_width > 0,
            "disk bar_width must be greater than zero"
        );
        ensure!(
            config.bar_height > 0,
            "disk bar_height must be greater than zero"
        );
        ensure!(
            config.bar_border_width >= 0,
            "disk bar_border_width must not be negative"
        );
        config.style.validate()?;
        let mounts = normalized_mounts(config.mount.as_deref(), &config.mounts)?;
        let _ = crate::disk_popup::DiskPopupConfig::load()?;

        let bar_background = config::parse_rgba(&config.bar_background)?;
        let bar_fill = config::parse_rgba(&config.bar_fill)?;
        let bar_border = config::parse_rgba(&config.bar_border)?;
        let stats = read_mount_stats(&mounts[0])?;

        Ok(Self {
            config,
            mounts,
            stats,
            bar_background,
            bar_fill,
            bar_border,
            revision: 0,
        })
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
        self.stats = read_mount_stats(&self.mounts[0])?;
        self.revision = self.revision.wrapping_add(1);
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Disk(DiskVisual {
            percent: self.stats.percent(),
            bar_width: self.config.bar_width,
            bar_height: self.config.bar_height,
            bar_border_width: self.config.bar_border_width,
            bar_background: self.bar_background,
            bar_fill: self.bar_fill,
            bar_border: self.bar_border,
        })
    }
}

pub(crate) fn configured_mounts() -> Result<Vec<String>> {
    let config: DiskConfig = config::load_module(NAME)?;
    normalized_mounts(config.mount.as_deref(), &config.mounts)
}

pub(crate) fn read_mount_stats(mount: &str) -> Result<DiskStats> {
    let mount_c = CString::new(mount).context("disk mount must not contain NUL bytes")?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(mount_c.as_ptr(), stats.as_mut_ptr()) };
    ensure!(rc == 0, "statvfs failed for {mount}");
    let stats = unsafe { stats.assume_init() };

    let total_bytes = stats.f_blocks as u128 * stats.f_frsize as u128;
    let available_bytes = stats.f_bavail as u128 * stats.f_frsize as u128;
    ensure!(total_bytes > 0, "filesystem {mount} has zero size");

    Ok(DiskStats {
        mount: mount.to_owned(),
        total_bytes,
        available_bytes,
    })
}

fn normalized_mounts(legacy_mount: Option<&str>, mounts: &[String]) -> Result<Vec<String>> {
    let result = if mounts.is_empty() {
        vec![legacy_mount.unwrap_or("/").to_owned()]
    } else {
        mounts.to_vec()
    };
    ensure!(
        result.iter().all(|mount| !mount.is_empty()),
        "disk mounts must not contain empty paths"
    );
    Ok(result)
}

fn default_interval_ms() -> u64 {
    5_000
}

fn default_bar_width() -> i32 {
    40
}

fn default_bar_height() -> i32 {
    14
}

fn default_bar_border_width() -> i32 {
    1
}

fn default_bar_background() -> String {
    "#22222266".into()
}

fn default_bar_fill() -> String {
    "#AAAAAA".into()
}

fn default_bar_border() -> String {
    "#535D6C99".into()
}

#[cfg(test)]
mod tests {
    use super::{normalized_mounts, read_mount_stats};

    #[test]
    fn supports_legacy_mount_and_mount_list() {
        assert_eq!(
            normalized_mounts(Some("/home"), &[]).expect("legacy"),
            vec!["/home"]
        );
        assert_eq!(
            normalized_mounts(None, &["/".into(), "/home".into()]).expect("list"),
            vec!["/", "/home"]
        );
    }

    #[test]
    fn reads_root_filesystem_stats() {
        let stats = read_mount_stats("/").expect("root fs");
        assert!(stats.total_bytes > 0);
        assert!(stats.available_bytes <= stats.total_bytes);
    }
}
