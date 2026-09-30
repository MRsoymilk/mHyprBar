use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::{
    config::{self, ModuleStyle},
    hyprland,
    window_icon::{WindowIcon, resolve_window_icon},
};

#[derive(Clone, Debug)]
pub enum WindowListScope {
    Workspace { id: i32 },
    Monitor { id: i32, name: String },
}

#[derive(Clone, Debug, Deserialize)]
pub struct ActiveWindowPopupConfig {
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
    #[serde(default = "default_icon_size")]
    pub icon_size: i32,
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

#[derive(Deserialize)]
struct ActiveWindowPopupFile {
    #[serde(default)]
    popup: ActiveWindowPopupConfig,
}

impl Default for ActiveWindowPopupConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            width: default_width(),
            padding: default_padding(),
            title_height: default_title_height(),
            row_height: default_row_height(),
            icon_size: default_icon_size(),
            border: default_border(),
            separator: default_separator(),
            active_background: default_active_background(),
            hover_background: default_hover_background(),
            style: default_popup_style(),
        }
    }
}

impl ActiveWindowPopupConfig {
    fn load() -> Result<Self> {
        let file: ActiveWindowPopupFile = config::load_module("active_window")?;
        let config = file.popup;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "active_window popup width must be positive");
        ensure!(
            self.padding >= 0,
            "active_window popup padding must not be negative"
        );
        ensure!(
            self.title_height > 0,
            "active_window popup title_height must be positive"
        );
        ensure!(
            self.row_height > 0,
            "active_window popup row_height must be positive"
        );
        ensure!(
            self.icon_size > 0 && self.icon_size <= self.row_height,
            "active_window popup icon_size must be in 1..=row_height"
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
pub struct WindowRow {
    pub address: String,
    pub class: String,
    pub title: String,
    pub workspace_id: i32,
    pub active: bool,
    pub icon: Option<WindowIcon>,
}

pub struct ActiveWindowPopupModel {
    pub config: ActiveWindowPopupConfig,
    pub title: String,
    pub rows: Vec<WindowRow>,
    pub hovered_row: Option<usize>,
}

impl ActiveWindowPopupModel {
    pub fn new(scope: WindowListScope, active_address: &str) -> Result<Self> {
        let config = ActiveWindowPopupConfig::load()?;
        let mut clients = hyprland::window_clients()?;
        clients.retain(|client| match &scope {
            WindowListScope::Workspace { id } => client.workspace_id == *id,
            WindowListScope::Monitor { id, .. } => client.monitor_id == *id,
        });

        let mut rows = clients
            .into_iter()
            .map(|client| {
                let icon =
                    resolve_window_icon(&client.class, &client.initial_class, config.icon_size);
                WindowRow {
                    active: !active_address.is_empty()
                        && client.address.eq_ignore_ascii_case(active_address),
                    address: client.address,
                    class: if client.class.trim().is_empty() {
                        client.initial_class
                    } else {
                        client.class
                    },
                    title: client.title,
                    workspace_id: client.workspace_id,
                    icon,
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            b.active
                .cmp(&a.active)
                .then_with(|| a.workspace_id.cmp(&b.workspace_id))
                .then_with(|| {
                    a.class
                        .to_ascii_lowercase()
                        .cmp(&b.class.to_ascii_lowercase())
                })
                .then_with(|| {
                    a.title
                        .to_ascii_lowercase()
                        .cmp(&b.title.to_ascii_lowercase())
                })
        });

        let title = match scope {
            WindowListScope::Workspace { .. } => format!("Current tag · {} windows", rows.len()),
            WindowListScope::Monitor { name, .. } => {
                format!("Monitor {name} · {} windows", rows.len())
            }
        };

        Ok(Self {
            config,
            title,
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
        if x < 0.0 || x >= self.config.width as f64 || y < 0.0 || y >= self.panel_height() as f64 {
            return None;
        }
        let rows_y = self
            .config
            .padding
            .saturating_add(self.config.title_height)
            .saturating_add(1);
        if y < rows_y as f64 {
            return None;
        }
        let index = ((y - rows_y as f64) / self.config.row_height as f64).floor() as usize;
        (index < self.rows.len()).then_some(index)
    }
}

fn default_enabled() -> bool {
    true
}
fn default_width() -> i32 {
    520
}
fn default_padding() -> i32 {
    10
}
fn default_title_height() -> i32 {
    30
}
fn default_row_height() -> i32 {
    46
}
fn default_icon_size() -> i32 {
    28
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
