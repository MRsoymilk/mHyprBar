use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "battery";
pub const CONFIG_FILE: &str = "modules/battery.toml";

#[derive(Debug, Deserialize)]
struct BatteryConfig {
    #[serde(default = "default_device")]
    device: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_icon_width")]
    icon_width: i32,
    #[serde(default = "default_icon_height")]
    icon_height: i32,
    #[serde(default = "default_icon_tip_width")]
    icon_tip_width: i32,
    #[serde(default = "default_icon_border_width")]
    icon_border_width: i32,
    #[serde(default = "default_text_gap")]
    text_gap: i32,
    #[serde(default = "default_high_percent")]
    high_percent: f32,
    #[serde(default = "default_medium_percent")]
    medium_percent: f32,
    #[serde(default = "default_low_percent")]
    low_percent: f32,
    #[serde(default = "default_high_color")]
    high_color: String,
    #[serde(default = "default_medium_color")]
    medium_color: String,
    #[serde(default = "default_low_color")]
    low_color: String,
    #[serde(default = "default_critical_color")]
    critical_color: String,
    #[serde(default = "default_charging_color")]
    charging_color: String,
    #[serde(default = "default_icon_background")]
    icon_background: String,
    #[serde(default)]
    empty_text: String,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone, Debug)]
pub(crate) struct BatteryStats {
    pub capacity: f32,
    pub status: String,
    pub energy_now_wh: Option<f64>,
    pub energy_full_wh: Option<f64>,
    pub energy_full_design_wh: Option<f64>,
    pub power_now_w: Option<f64>,
}

impl BatteryStats {
    pub fn charging(&self) -> bool {
        self.status.eq_ignore_ascii_case("Charging")
    }

    pub fn health_percent(&self) -> Option<f64> {
        let full = self.energy_full_wh?;
        let design = self.energy_full_design_wh?;
        (design > 0.0).then_some((100.0 * full / design).clamp(0.0, 200.0))
    }

    pub fn time_remaining_seconds(&self) -> Option<u64> {
        let rate = self.power_now_w?;
        if rate <= 0.01 {
            return None;
        }

        let hours = if self.charging() {
            let full = self.energy_full_wh?;
            let now = self.energy_now_wh?;
            (full - now).max(0.0) / rate
        } else if self.status.eq_ignore_ascii_case("Discharging") {
            self.energy_now_wh? / rate
        } else {
            return None;
        };

        if !hours.is_finite() || !(0.0..=24.0 * 30.0).contains(&hours) {
            return None;
        }
        Some((hours * 3600.0).round() as u64)
    }
}

#[derive(Clone)]
pub struct BatteryVisual {
    pub capacity: f32,
    pub charging: bool,
    pub icon_width: i32,
    pub icon_height: i32,
    pub icon_tip_width: i32,
    pub icon_border_width: i32,
    pub text_gap: i32,
    pub high_percent: f32,
    pub medium_percent: f32,
    pub low_percent: f32,
    pub high_color: [u8; 4],
    pub medium_color: [u8; 4],
    pub low_color: [u8; 4],
    pub critical_color: [u8; 4],
    pub charging_color: [u8; 4],
    pub icon_background: [u8; 4],
}

pub struct BatteryModule {
    config: BatteryConfig,
    resolved_device: Option<String>,
    stats: Option<BatteryStats>,
    high_color: [u8; 4],
    medium_color: [u8; 4],
    low_color: [u8; 4],
    critical_color: [u8; 4],
    charging_color: [u8; 4],
    icon_background: [u8; 4],
    revision: u64,
}

impl BatteryModule {
    pub fn load() -> Result<Self> {
        let config: BatteryConfig = config::load_module(NAME)?;
        validate_config(&config)?;
        let _ = crate::battery_popup::BatteryPopupConfig::load()?;

        let high_color = config::parse_rgba(&config.high_color)?;
        let medium_color = config::parse_rgba(&config.medium_color)?;
        let low_color = config::parse_rgba(&config.low_color)?;
        let critical_color = config::parse_rgba(&config.critical_color)?;
        let charging_color = config::parse_rgba(&config.charging_color)?;
        let icon_background = config::parse_rgba(&config.icon_background)?;

        Ok(Self {
            config,
            resolved_device: None,
            stats: None,
            high_color,
            medium_color,
            low_color,
            critical_color,
            charging_color,
            icon_background,
            revision: 0,
        })
    }

    fn root(&mut self) -> Result<Option<PathBuf>> {
        resolve_battery_root(&self.config.device, &mut self.resolved_device)
    }
}

impl StatusModule for BatteryModule {
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
        let Some(root) = self.root()? else {
            self.stats = None;
            self.revision = self.revision.wrapping_add(1);
            return Ok(self.config.empty_text.clone());
        };

        self.stats = Some(read_battery_stats(&root)?);
        self.revision = self.revision.wrapping_add(1);
        Ok(String::new())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }

    fn visual(&self) -> ModuleVisual {
        let Some(stats) = self.stats.as_ref() else {
            return ModuleVisual::Text;
        };
        ModuleVisual::Battery(BatteryVisual {
            capacity: stats.capacity,
            charging: stats.charging(),
            icon_width: self.config.icon_width,
            icon_height: self.config.icon_height,
            icon_tip_width: self.config.icon_tip_width,
            icon_border_width: self.config.icon_border_width,
            text_gap: self.config.text_gap,
            high_percent: self.config.high_percent,
            medium_percent: self.config.medium_percent,
            low_percent: self.config.low_percent,
            high_color: self.high_color,
            medium_color: self.medium_color,
            low_color: self.low_color,
            critical_color: self.critical_color,
            charging_color: self.charging_color,
            icon_background: self.icon_background,
        })
    }
}

pub(crate) fn read_configured_stats() -> Result<Option<BatteryStats>> {
    let config: BatteryConfig = config::load_module(NAME)?;
    validate_config(&config)?;
    let mut resolved = None;
    let Some(root) = resolve_battery_root(&config.device, &mut resolved)? else {
        return Ok(None);
    };
    Ok(Some(read_battery_stats(&root)?))
}

fn validate_config(config: &BatteryConfig) -> Result<()> {
    ensure!(
        config.interval_ms > 0,
        "battery interval_ms must be greater than zero"
    );
    ensure!(
        !config.device.trim().is_empty(),
        "battery device must not be empty"
    );
    ensure!(
        config.icon_width > 4,
        "battery icon_width must be greater than 4"
    );
    ensure!(
        config.icon_height > 4,
        "battery icon_height must be greater than 4"
    );
    ensure!(
        config.icon_tip_width > 0,
        "battery icon_tip_width must be greater than zero"
    );
    ensure!(
        config.icon_border_width > 0,
        "battery icon_border_width must be greater than zero"
    );
    ensure!(
        config.text_gap >= 0,
        "battery text_gap must not be negative"
    );
    ensure!(
        (0.0..=100.0).contains(&config.low_percent)
            && (0.0..=100.0).contains(&config.medium_percent)
            && (0.0..=100.0).contains(&config.high_percent)
            && config.low_percent <= config.medium_percent
            && config.medium_percent <= config.high_percent,
        "battery thresholds must satisfy 0 <= low <= medium <= high <= 100"
    );
    config.style.validate()?;
    Ok(())
}

fn resolve_battery_root(
    device: &str,
    resolved_device: &mut Option<String>,
) -> Result<Option<PathBuf>> {
    let power_supply = Path::new("/sys/class/power_supply");

    if device != "auto" {
        return Ok(Some(power_supply.join(device)));
    }

    if let Some(current) = resolved_device.as_ref() {
        let root = power_supply.join(current);
        if root.exists() && is_battery_root(&root) {
            return Ok(Some(root));
        }
        *resolved_device = None;
    }

    let Some(discovered) = discover_battery(power_supply)? else {
        return Ok(None);
    };
    let root = power_supply.join(&discovered);
    *resolved_device = Some(discovered);
    Ok(Some(root))
}

fn read_battery_stats(root: &Path) -> Result<BatteryStats> {
    let device = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("battery")
        .to_owned();

    let capacity = read_number(root, "capacity")
        .with_context(|| format!("failed to read battery {device} capacity"))?
        .clamp(0.0, 100.0) as f32;
    let status = read_text(root, "status").unwrap_or_else(|| "Unknown".into());

    let voltage_now_uv =
        read_number(root, "voltage_now").or_else(|| read_number(root, "voltage_min_design"));
    let energy_now_wh = energy_wh(root, "energy_now", "charge_now", voltage_now_uv);
    let energy_full_wh = energy_wh(root, "energy_full", "charge_full", voltage_now_uv);
    let energy_full_design_wh = energy_wh(
        root,
        "energy_full_design",
        "charge_full_design",
        voltage_now_uv,
    );
    let power_now_w = read_number(root, "power_now")
        .map(|value| value / 1_000_000.0)
        .or_else(|| {
            let current_ua = read_number(root, "current_now")?;
            let voltage_uv = voltage_now_uv?;
            Some(current_ua * voltage_uv / 1_000_000_000_000.0)
        });

    Ok(BatteryStats {
        capacity,
        status,
        energy_now_wh,
        energy_full_wh,
        energy_full_design_wh,
        power_now_w,
    })
}

fn energy_wh(
    root: &Path,
    energy_field: &str,
    charge_field: &str,
    voltage_uv: Option<f64>,
) -> Option<f64> {
    if let Some(value) = read_number(root, energy_field) {
        return Some(value / 1_000_000.0);
    }
    let charge_uah = read_number(root, charge_field)?;
    let voltage_uv = voltage_uv?;
    Some(charge_uah * voltage_uv / 1_000_000_000_000.0)
}

fn read_number(root: &Path, field: &str) -> Option<f64> {
    fs::read_to_string(root.join(field))
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
}

fn read_text(root: &Path, field: &str) -> Option<String> {
    Some(fs::read_to_string(root.join(field)).ok()?.trim().to_owned())
}

fn discover_battery(power_supply: &Path) -> Result<Option<String>> {
    let entries = match fs::read_dir(power_supply) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("failed to enumerate power supplies"),
    };

    let mut devices = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            is_battery_root(&entry.path()).then_some(name)
        })
        .collect::<Vec<_>>();
    devices.sort();

    Ok(devices.into_iter().next())
}

fn is_battery_root(root: &Path) -> bool {
    fs::read_to_string(root.join("type")).is_ok_and(|kind| is_battery_type(&kind))
}

fn is_battery_type(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("battery")
}

fn default_device() -> String {
    "auto".into()
}

fn default_interval_ms() -> u64 {
    5_000
}

fn default_icon_width() -> i32 {
    26
}

fn default_icon_height() -> i32 {
    14
}

fn default_icon_tip_width() -> i32 {
    3
}

fn default_icon_border_width() -> i32 {
    2
}

fn default_text_gap() -> i32 {
    6
}

fn default_high_percent() -> f32 {
    60.0
}

fn default_medium_percent() -> f32 {
    30.0
}

fn default_low_percent() -> f32 {
    10.0
}

fn default_high_color() -> String {
    "#56E36B".into()
}

fn default_medium_color() -> String {
    "#FFD84A".into()
}

fn default_low_color() -> String {
    "#FF8A25".into()
}

fn default_critical_color() -> String {
    "#FF365A".into()
}

fn default_charging_color() -> String {
    "#56E36B".into()
}

fn default_icon_background() -> String {
    "#303030".into()
}

#[cfg(test)]
mod tests {
    use super::{BatteryStats, is_battery_type};

    #[test]
    fn identifies_battery_type() {
        assert!(is_battery_type("Battery\n"));
        assert!(is_battery_type("battery"));
        assert!(!is_battery_type("Mains\n"));
        assert!(!is_battery_type("USB\n"));
    }

    #[test]
    fn computes_health_and_time_remaining() {
        let stats = BatteryStats {
            capacity: 50.0,
            status: "Discharging".into(),
            energy_now_wh: Some(25.0),
            energy_full_wh: Some(50.0),
            energy_full_design_wh: Some(60.0),
            power_now_w: Some(10.0),
        };
        assert!((stats.health_percent().expect("health") - 83.333).abs() < 0.01);
        assert_eq!(stats.time_remaining_seconds(), Some(9_000));
    }
}
