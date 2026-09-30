use std::{fs, path::PathBuf, process::Command};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::config::{self, ModuleStyle};

#[derive(Clone, Debug, Deserialize)]
pub struct NetworkPopupConfig {
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
    #[serde(default = "default_popup_style")]
    pub style: ModuleStyle,
}

#[derive(Deserialize)]
struct NetworkPopupFile {
    #[serde(default)]
    popup: NetworkPopupConfig,
}

impl Default for NetworkPopupConfig {
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
            style: default_popup_style(),
        }
    }
}

impl NetworkPopupConfig {
    pub fn load() -> Result<Self> {
        let file: NetworkPopupFile = config::load_module("network")?;
        let cfg = file.popup;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.width > 0, "network popup width must be positive");
        ensure!(self.padding >= 0, "network popup padding must not be negative");
        ensure!(self.title_height > 0, "network popup title_height must be positive");
        ensure!(self.row_height > 0, "network popup row_height must be positive");
        self.style.validate()?;
        let _ = self.border_rgba()?;
        let _ = self.separator_rgba()?;
        let _ = self.active_background_rgba()?;
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
}

#[derive(Clone, Debug)]
pub struct NetworkInterfaceRow {
    pub name: String,
    pub state: String,
    pub mac: String,
    pub mtu: String,
    pub speed: String,
    pub addresses: String,
    pub is_default: bool,
}

pub struct NetworkPopupModel {
    pub config: NetworkPopupConfig,
    pub interfaces: Vec<NetworkInterfaceRow>,
}

impl NetworkPopupModel {
    pub fn new() -> Result<Self> {
        let config = NetworkPopupConfig::load()?;
        let interfaces = read_interfaces()?;
        Ok(Self { config, interfaces })
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
                    .saturating_mul(self.interfaces.len().max(1) as i32),
            )
    }
}

fn read_interfaces() -> Result<Vec<NetworkInterfaceRow>> {
    let default = default_route_interface();
    let mut names = fs::read_dir("/sys/class/net")
        .context("failed to read /sys/class/net")?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    names.sort();

    names
        .into_iter()
        .map(|name| {
            let root = PathBuf::from("/sys/class/net").join(&name);
            let state = read_trimmed(root.join("operstate")).unwrap_or_else(|| "unknown".into());
            let mac = read_trimmed(root.join("address")).unwrap_or_else(|| "--".into());
            let mtu = read_trimmed(root.join("mtu")).unwrap_or_else(|| "--".into());
            let speed = read_trimmed(root.join("speed"))
                .filter(|value| value != "-1")
                .map(|value| format!("{value} Mbps"))
                .unwrap_or_else(|| "--".into());
            let addresses = interface_addresses(&name);
            Ok(NetworkInterfaceRow {
                is_default: default.as_deref() == Some(name.as_str()),
                name,
                state,
                mac,
                mtu,
                speed,
                addresses,
            })
        })
        .collect()
}

fn read_trimmed(path: PathBuf) -> Option<String> {
    fs::read_to_string(path).ok().map(|value| value.trim().to_owned())
}

fn interface_addresses(interface: &str) -> String {
    let output = Command::new("ip")
        .args(["-o", "addr", "show", "dev", interface])
        .output();
    let Ok(output) = output else {
        return "--".into();
    };
    if !output.status.success() {
        return "--".into();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let addresses = text
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            fields
                .iter()
                .position(|field| *field == "inet" || *field == "inet6")
                .and_then(|index| fields.get(index + 1))
                .copied()
        })
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        "--".into()
    } else {
        addresses.join(", ")
    }
}

fn default_route_interface() -> Option<String> {
    let content = fs::read_to_string("/proc/net/route").ok()?;
    content.lines().skip(1).find_map(|line| {
        let mut fields = line.split_whitespace();
        let interface = fields.next()?;
        let destination = fields.next()?;
        (destination == "00000000" && interface != "lo").then(|| interface.to_owned())
    })
}

fn default_enabled() -> bool { true }
fn default_width() -> i32 { 620 }
fn default_padding() -> i32 { 12 }
fn default_title_height() -> i32 { 32 }
fn default_row_height() -> i32 { 58 }
fn default_border() -> String { "#3C414A".into() }
fn default_separator() -> String { "#353A42".into() }
fn default_active_background() -> String { "#203728".into() }
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
