use anyhow::Result;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

#[cfg(mhypr_module = "tray")]
use crate::modules::tray::popup::{TrayPopupHit, TrayPopupModel, TrayPopupRect};
use crate::{
    config::{BarConfig, ModuleStyle, WorkspacesConfig},
    hyprland::{MonitorState, Snapshot},
    modules::{ModuleManager, ModuleView, ModuleVisual, tray::runtime::TrayState},
};

mod bar;
mod layout;
mod popup_common;
mod popup_controls;
mod popup_resources;
mod popup_system;
mod primitives;

#[allow(unused_imports)]
pub use layout::{ModuleHit, module_at_x, workspace_at_x};
pub(crate) use primitives::cpu_usage_color;

pub struct Renderer {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Rect {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) w: i32,
    pub(super) h: i32,
}
