use std::{
    collections::HashSet,
    env,
    ffi::CString,
    fs,
    io::Write,
    mem::MaybeUninit,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "disk";
pub const CONFIG_FILE: &str = "modules/disk.toml";

const ICON_DISK_SVG: &[u8] = include_bytes!("../../res/disk/disk.svg");

#[derive(Debug, Deserialize)]
struct DiskConfig {
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    mount: Option<String>,
    #[serde(default)]
    mounts: Vec<String>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_icon_scale")]
    icon_scale: f32,
    #[serde(default = "default_icon_gap")]
    icon_gap: i32,
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
struct DiskIcon {
    pixels: Arc<[u8]>,
    width: i32,
    height: i32,
}

#[derive(Clone)]
pub struct DiskVisual {
    pub percent: f32,
    pub icon_scale: f32,
    pub icon_gap: i32,
    pub icon_pixels: Arc<[u8]>,
    pub icon_width: i32,
    pub icon_height: i32,
    pub bar_width: i32,
    pub bar_height: i32,
    pub bar_border_width: i32,
    pub bar_background: [u8; 4],
    pub bar_fill: [u8; 4],
    pub bar_border: [u8; 4],
}

pub struct DiskModule {
    config: DiskConfig,
    stats: DiskStats,
    icon: DiskIcon,
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
            config.icon_scale.is_finite() && (0.1..=1.0).contains(&config.icon_scale),
            "disk icon_scale must be in 0.1..=1.0"
        );
        ensure!(config.icon_gap >= 0, "disk icon_gap must not be negative");
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
        let _ = configured_paths_from(&config)?;
        let _ = crate::disk_popup::DiskPopupConfig::load()?;

        let icon = rasterize_svg("disk.svg", ICON_DISK_SVG)?;
        let bar_background = config::parse_rgba(&config.bar_background)?;
        let bar_fill = config::parse_rgba(&config.bar_fill)?;
        let bar_border = config::parse_rgba(&config.bar_border)?;
        let stats = read_total_disk_stats()?;

        Ok(Self {
            config,
            stats,
            icon,
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
        self.stats = read_total_disk_stats()?;
        self.revision = self.revision.wrapping_add(1);
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Disk(DiskVisual {
            percent: self.stats.percent(),
            icon_scale: self.config.icon_scale,
            icon_gap: self.config.icon_gap,
            icon_pixels: Arc::clone(&self.icon.pixels),
            icon_width: self.icon.width,
            icon_height: self.icon.height,
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

fn read_total_disk_stats() -> Result<DiskStats> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .context("failed to read /proc/self/mountinfo")?;
    let mut seen_devices = HashSet::new();
    let mut total_bytes = 0_u128;
    let mut available_bytes = 0_u128;

    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            continue;
        };
        if fields.len() <= separator + 2 || fields.len() < 5 {
            continue;
        }

        let device = fields[2];
        let mount_point = decode_mountinfo_field(fields[4]);
        let fs_type = fields[separator + 1];
        let source = fields[separator + 2];

        if !is_local_disk_filesystem(fs_type, source) || !seen_devices.insert(device.to_owned()) {
            continue;
        }

        let Ok(stats) = read_mount_stats(&mount_point) else {
            continue;
        };
        total_bytes = total_bytes.saturating_add(stats.total_bytes);
        available_bytes = available_bytes.saturating_add(stats.available_bytes);
    }

    if total_bytes == 0 {
        let mut fallback = read_mount_stats("/")?;
        fallback.mount = "Total".into();
        return Ok(fallback);
    }

    Ok(DiskStats {
        mount: "Total".into(),
        total_bytes,
        available_bytes,
    })
}

fn is_local_disk_filesystem(fs_type: &str, source: &str) -> bool {
    source.starts_with("/dev/")
        || matches!(
            fs_type,
            "ext2"
                | "ext3"
                | "ext4"
                | "xfs"
                | "btrfs"
                | "f2fs"
                | "vfat"
                | "exfat"
                | "ntfs"
                | "ntfs3"
                | "reiserfs"
                | "jfs"
                | "bcachefs"
                | "zfs"
        )
}

fn decode_mountinfo_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
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

fn rasterize_svg(name: &str, svg_bytes: &[u8]) -> Result<DiskIcon> {
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

    ensure!(
        status.success() && output.status.success(),
        "disk SVG rasterization failed for {name}"
    );

    let expected = RASTER_SIZE as usize * RASTER_SIZE as usize * 4;
    ensure!(
        output.stdout.len() == expected,
        "disk SVG rasterizer returned {} bytes for {RASTER_SIZE}x{RASTER_SIZE}, expected {expected}: {name}",
        output.stdout.len()
    );

    crop_transparent_margin(name, &output.stdout, RASTER_SIZE, RASTER_SIZE)
}

fn crop_transparent_margin(name: &str, pixels: &[u8], width: i32, height: i32) -> Result<DiskIcon> {
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
        "disk SVG has no visible pixels: {name}"
    );

    let cropped_width = max_x - min_x + 1;
    let cropped_height = max_y - min_y + 1;
    let mut cropped = Vec::with_capacity(cropped_width as usize * cropped_height as usize * 4);

    for y in min_y..=max_y {
        let start = ((y * width + min_x) * 4) as usize;
        let end = start + cropped_width as usize * 4;
        cropped.extend_from_slice(&pixels[start..end]);
    }

    Ok(DiskIcon {
        pixels: Arc::from(cropped),
        width: cropped_width,
        height: cropped_height,
    })
}

fn default_interval_ms() -> u64 {
    5_000
}

fn default_icon_scale() -> f32 {
    0.8
}

fn default_icon_gap() -> i32 {
    4
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
    use super::{decode_mountinfo_field, read_mount_stats, read_total_disk_stats, resolve_path};

    #[test]
    fn resolves_home_path() {
        let home = std::env::var("HOME").expect("HOME");
        assert_eq!(resolve_path("$HOME").expect("home"), home);
        assert_eq!(resolve_path("~").expect("tilde"), home);
    }

    #[test]
    fn decodes_mountinfo_paths() {
        assert_eq!(
            decode_mountinfo_field("/media/My\\040Disk"),
            "/media/My Disk"
        );
    }

    #[test]
    fn reads_root_filesystem_stats() {
        let stats = read_mount_stats("/").expect("root fs");
        assert!(stats.total_bytes > 0);
        assert!(stats.available_bytes <= stats.total_bytes);
    }

    #[test]
    fn aggregates_local_disk_filesystems() {
        let stats = read_total_disk_stats().expect("total disks");
        assert_eq!(stats.mount, "Total");
        assert!(stats.total_bytes > 0);
        assert!(stats.available_bytes <= stats.total_bytes);
    }
}
