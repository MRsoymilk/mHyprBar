use std::{env, ffi::CString, mem::MaybeUninit, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "disk";
pub const CONFIG_FILE: &str = "modules/disk.toml";

#[derive(Debug, Deserialize)]
struct DiskConfig {
    #[serde(default)]
    primary: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
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
    primary_path: String,
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
        let primary_path = resolve_primary_path(&config)?;
        let _ = configured_paths_from(&config)?;
        let _ = crate::disk_popup::DiskPopupConfig::load()?;

        let bar_background = config::parse_rgba(&config.bar_background)?;
        let bar_fill = config::parse_rgba(&config.bar_fill)?;
        let bar_border = config::parse_rgba(&config.bar_border)?;
        let stats = read_mount_stats(&primary_path)?;

        Ok(Self {
            config,
            primary_path,
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
        self.stats = read_mount_stats(&self.primary_path)?;
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
    configured_paths_from(&config)
}

pub(crate) fn read_mount_stats(path: &str) -> Result<DiskStats> {
    let resolved = resolve_path(path)?;
    let mount_c =
        CString::new(resolved.as_str()).context("disk path must not contain NUL bytes")?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(mount_c.as_ptr(), stats.as_mut_ptr()) };
    ensure!(rc == 0, "statvfs failed for {resolved}");
    let stats = unsafe { stats.assume_init() };

    let total_bytes = stats.f_blocks as u128 * stats.f_frsize as u128;
    let available_bytes = stats.f_bavail as u128 * stats.f_frsize as u128;
    ensure!(total_bytes > 0, "filesystem {resolved} has zero size");

    Ok(DiskStats {
        mount: if path == "$HOME" || path == "~" {
            resolved
        } else {
            path.to_owned()
        },
        total_bytes,
        available_bytes,
    })
}

fn resolve_primary_path(config: &DiskConfig) -> Result<String> {
    if let Some(primary) = config.primary.as_deref() {
        return resolve_path(primary);
    }
    if let Some(first) = config.paths.first() {
        return resolve_path(first);
    }
    if let Some(first) = config.mounts.first() {
        return resolve_path(first);
    }
    resolve_path(config.mount.as_deref().unwrap_or("$HOME"))
}

fn configured_paths_from(config: &DiskConfig) -> Result<Vec<String>> {
    let result = if !config.paths.is_empty() {
        config.paths.clone()
    } else if !config.mounts.is_empty() {
        config.mounts.clone()
    } else if let Some(mount) = config.mount.as_ref() {
        vec![mount.clone()]
    } else {
        vec!["/".into()]
    };
    ensure!(
        result.iter().all(|path| !path.is_empty()),
        "disk paths must not contain empty values"
    );
    Ok(result)
}

fn resolve_path(path: &str) -> Result<String> {
    if path == "$HOME" || path == "~" {
        return env::var("HOME").context("HOME is not set for disk module");
    }
    ensure!(!path.is_empty(), "disk path must not be empty");
    Ok(path.to_owned())
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
    use super::{read_mount_stats, resolve_path};

    #[test]
    fn resolves_home_path() {
        let home = std::env::var("HOME").expect("HOME");
        assert_eq!(resolve_path("$HOME").expect("home"), home);
        assert_eq!(resolve_path("~").expect("tilde"), home);
    }

    #[test]
    fn reads_root_filesystem_stats() {
        let stats = read_mount_stats("/").expect("root fs");
        assert!(stats.total_bytes > 0);
        assert!(stats.available_bytes <= stats.total_bytes);
    }
}
