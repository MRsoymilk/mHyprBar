use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::{
    config::{self, ModuleStyle},
    hyprland,
    modules::layout::layout_display_name,
};

#[derive(Clone, Debug, Deserialize)]
pub struct LayoutPopupConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_padding")]
    pub padding: i32,
    #[serde(default = "default_title_height")]
    pub title_height: i32,
    #[serde(default = "default_row_height")]
    pub row_height: i32,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_active_background")]
    pub active_background: String,
    #[serde(default = "default_hover_background")]
    pub hover_background: String,
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Debug, Deserialize)]
struct LayoutPopupFile {
    #[serde(default = "default_layouts")]
    layouts: Vec<String>,
    #[serde(default)]
    popup: LayoutPopupConfig,
}

impl Default for LayoutPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            title_height: default_title_height(),
            row_height: default_row_height(),
            border: default_border(),
            separator: default_separator(),
            active_background: default_active_background(),
            hover_background: default_hover_background(),
            style: default_popup_style(),
        }
    }
}

impl LayoutPopupConfig {
    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "layout popup width must be positive");
        ensure!(
            self.padding >= 0,
            "layout popup padding must not be negative"
        );
        ensure!(
            self.title_height > 0,
            "layout popup title_height must be positive"
        );
        ensure!(
            self.row_height > 0,
            "layout popup row_height must be positive"
        );
        self.style.validate()?;
        let _ = self.border_rgba()?;
        let _ = self.separator_rgba()?;
        let _ = self.active_background_rgba()?;
        let _ = self.hover_background_rgba()?;
        Ok(())
    }

    pub fn border_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.border)
    }

    pub fn separator_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.separator)
    }

    pub fn active_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.active_background)
    }

    pub fn hover_background_rgba(&self) -> Result<[u8; 4]> {
        config::parse_rgba(&self.hover_background)
    }
}

#[derive(Clone, Debug)]
pub struct LayoutRow {
    pub name: String,
    pub label: String,
    pub active: bool,
}

pub struct LayoutPopupModel {
    pub config: LayoutPopupConfig,
    pub rows: Vec<LayoutRow>,
    pub hovered_row: Option<usize>,
}

impl LayoutPopupModel {
    pub fn new() -> Result<Self> {
        let file: LayoutPopupFile = config::load_module("layout")?;
        file.popup.validate()?;
        ensure!(
            !file.layouts.is_empty() && file.layouts.iter().all(|layout| !layout.trim().is_empty()),
            "layout layouts must contain at least one non-empty layout"
        );
        let current = hyprland::active_layout()?;
        let rows = file
            .layouts
            .iter()
            .cloned()
            .map(|name| LayoutRow {
                active: name.eq_ignore_ascii_case(&current),
                label: layout_display_name(&name),
                name,
            })
            .collect();

        Ok(Self {
            config: file.popup,
            rows,
            hovered_row: None,
        })
    }

    pub fn panel_height(&self) -> i32 {
        self.config
            .padding
            .saturating_mul(2)
            .saturating_add(self.config.title_height)
            .saturating_add(1)
            .saturating_add(
                self.config
                    .row_height
                    .saturating_mul(self.rows.len().max(1) as i32),
            )
    }

    pub fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        if x < 0.0 || x >= self.config.width as f64 {
            return None;
        }
        let rows_y = self.config.padding + self.config.title_height + 1;
        let local_y = y - rows_y as f64;
        if local_y < 0.0 {
            return None;
        }
        let index = (local_y / self.config.row_height as f64).floor() as usize;
        (index < self.rows.len()).then_some(index)
    }

    pub fn apply(&self, index: usize) -> Result<()> {
        let row = self
            .rows
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("layout popup row {index} is unavailable"))?;
        hyprland::set_active_layout(&row.name)
    }
}

fn default_layouts() -> Vec<String> {
    vec!["dwindle".into(), "floating".into()]
}

fn default_enabled() -> bool {
    true
}

fn default_width() -> i32 {
    240
}

fn default_padding() -> i32 {
    8
}

fn default_title_height() -> i32 {
    28
}

fn default_row_height() -> i32 {
    40
}

fn default_border() -> String {
    "#3C414A".into()
}

fn default_separator() -> String {
    "#353A42".into()
}

fn default_active_background() -> String {
    "#26384A".into()
}

fn default_hover_background() -> String {
    "#2A2D33".into()
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
