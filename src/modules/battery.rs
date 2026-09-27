use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::StatusModule;
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "battery";
pub const CONFIG_FILE: &str = "modules/battery.toml";

#[derive(Debug, Deserialize)]
struct BatteryConfig {
    #[serde(default = "default_device")]
    device: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_low_percent")]
    low_percent: f32,
    #[serde(default)]
    empty_text: String,
    #[serde(default)]
    style: ModuleStyle,
}

pub struct BatteryModule {
    config: BatteryConfig,
    resolved_device: Option<String>,
}

impl BatteryModule {
    pub fn load() -> Result<Self> {
        let config: BatteryConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "battery interval_ms must be greater than zero"
        );
        ensure!(
            !config.device.trim().is_empty(),
            "battery device must not be empty"
        );
        ensure!(
            (0.0..=100.0).contains(&config.low_percent),
            "battery low_percent must be between 0 and 100"
        );
        config.style.validate()?;
        Ok(Self {
            config,
            resolved_device: None,
        })
    }

    fn root(&mut self) -> Result<Option<PathBuf>> {
        let power_supply = Path::new("/sys/class/power_supply");

        if self.config.device != "auto" {
            return Ok(Some(power_supply.join(&self.config.device)));
        }

        if let Some(device) = &self.resolved_device {
            let root = power_supply.join(device);
            if root.exists() && is_battery_root(&root) {
                return Ok(Some(root));
            }
            self.resolved_device = None;
        }

        let Some(device) = discover_battery(power_supply)? else {
            return Ok(None);
        };
        let root = power_supply.join(&device);
        self.resolved_device = Some(device);
        Ok(Some(root))
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
            return Ok(self.config.empty_text.clone());
        };

        let device = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("battery");
        let capacity = fs::read_to_string(root.join("capacity"))
            .with_context(|| format!("failed to read battery {device}"))?;
        let capacity = capacity
            .trim()
            .parse::<u32>()
            .context("invalid battery capacity")?;
        let status = fs::read_to_string(root.join("status"))
            .unwrap_or_default()
            .trim()
            .to_owned();

        let suffix = match status.as_str() {
            "Charging" => "+",
            "Discharging" => "-",
            "Full" => "=",
            _ => "",
        };
        Ok(format!("BAT {capacity}%{suffix}"))
    }
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
    10_000
}

fn default_low_percent() -> f32 {
    20.0
}

#[cfg(test)]
mod tests {
    use super::is_battery_type;

    #[test]
    fn identifies_battery_type() {
        assert!(is_battery_type("Battery\n"));
        assert!(is_battery_type("battery"));
        assert!(!is_battery_type("Mains\n"));
        assert!(!is_battery_type("USB\n"));
    }
}
