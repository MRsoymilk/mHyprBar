use std::time::Duration;

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct DiskPopupConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_row_height")]
    pub row_height: i32,
    #[serde(default = "default_padding")]
    pub padding: i32,
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_bar_width")]
    pub bar_width: i32,
    #[serde(default = "default_bar_background")]
    pub bar_background: String,
    #[serde(default = "default_bar_fill")]
    pub bar_fill: String,
    #[serde(default = "default_bar_border")]
    pub bar_border: String,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_hover")]
    pub hover_background: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct DiskPopupFile {
    #[serde(default)]
    popup: DiskPopupConfig,
}

impl Default for DiskPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            row_height: default_row_height(),
            padding: default_padding(),
            refresh_ms: default_refresh_ms(),
            bar_width: default_bar_width(),
            bar_background: default_bar_background(),
            bar_fill: default_bar_fill(),
            bar_border: default_bar_border(),
            border: default_border(),
            hover_background: default_hover(),
            style: default_popup_style(),
        }
    }
}

impl DiskPopupConfig {
    pub fn load() -> Result<Self> {
        let file: DiskPopupFile = config::load_module("disk")?;
        let cfg = file.popup;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "disk popup width must be greater than zero");
        ensure!(
            self.row_height > 0,
            "disk popup row_height must be greater than zero"
        );
        ensure!(self.padding >= 0, "disk popup padding must not be negative");
        ensure!(
            self.refresh_ms > 0,
            "disk popup refresh_ms must be greater than zero"
        );
        ensure!(
            self.bar_width > 0,
            "disk popup bar_width must be greater than zero"
        );
        self.style.validate()?;
        let _ = config::parse_rgba(&self.bar_background)?;
        let _ = config::parse_rgba(&self.bar_fill)?;
        let _ = config::parse_rgba(&self.bar_border)?;
        let _ = config::parse_rgba(&self.border)?;
        let _ = config::parse_rgba(&self.hover_background)?;
        Ok(())
    }

    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_ms)
    }

    pub fn bar_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_background)
    }

    pub fn bar_fill_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_fill)
    }

    pub fn bar_border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.bar_border)
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn hover_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.hover_background)
    }
}

pub struct DiskPopupModel {
    pub config: DiskPopupConfig,
    pub rows: Vec<crate::modules::disk::DiskStats>,
    pub hovered_row: Option<usize>,
}

impl DiskPopupModel {
    pub fn new() -> Result<Self> {
        let config = DiskPopupConfig::load()?;
        let rows = read_rows()?;
        Ok(Self {
            config,
            rows,
            hovered_row: None,
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.rows = read_rows()?;
        Ok(())
    }

    pub fn panel_height(&self) -> i32 {
        let header_rows = 1_i32;
        let rows = header_rows + self.rows.len() as i32;
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(rows.saturating_mul(self.config.row_height))
    }

    pub fn row_area_start(&self) -> i32 {
        self.config.padding.saturating_add(self.config.row_height)
    }

    pub fn row_at(&self, y: f64) -> Option<usize> {
        let start = self.row_area_start();
        if y < start as f64 {
            return None;
        }
        let idx = ((y - start as f64) / self.config.row_height as f64) as usize;
        (idx < self.rows.len()).then_some(idx)
    }
}

pub fn format_bytes(bytes: u128) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let value = bytes as f64;
    if value >= TIB {
        format!("{:.1}T", value / TIB)
    } else if value >= GIB {
        format!("{:.1}G", value / GIB)
    } else if value >= MIB {
        format!("{:.0}M", value / MIB)
    } else if value >= KIB {
        format!("{:.0}K", value / KIB)
    } else {
        format!("{bytes}B")
    }
}

fn read_rows() -> Result<Vec<crate::modules::disk::DiskStats>> {
    crate::modules::disk::configured_mounts()?
        .iter()
        .map(|mount| crate::modules::disk::read_mount_stats(mount))
        .collect()
}

fn default_enabled() -> bool {
    true
}

fn default_width() -> i32 {
    420
}

fn default_row_height() -> i32 {
    28
}

fn default_padding() -> i32 {
    10
}

fn default_refresh_ms() -> u64 {
    5_000
}

fn default_bar_width() -> i32 {
    120
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

fn default_border() -> String {
    "#626262".into()
}

fn default_hover() -> String {
    "#3A3A3AF0".into()
}

fn default_popup_style() -> ModuleStyle {
    ModuleStyle {
        foreground: "#F2F2F2".into(),
        background: "#202020F2".into(),
        font_family: "sans-serif".into(),
        font_size: 12.0,
        padding_x: 0,
        padding_y: 0,
        min_width: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskPopupModel, format_bytes};

    #[test]
    fn formats_storage_sizes() {
        assert_eq!(format_bytes(1024_u128 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn popup_reads_configured_mounts() {
        let model = DiskPopupModel::new().expect("disk popup");
        assert!(!model.rows.is_empty());
        assert!(model.panel_height() > 0);
    }
}
