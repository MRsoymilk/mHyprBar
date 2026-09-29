use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{ModuleVisual, StatusModule};
use crate::config::{self, ModuleStyle};

pub const NAME: &str = "network";
pub const CONFIG_FILE: &str = "modules/network.toml";

#[derive(Debug, Deserialize)]
struct NetworkConfig {
    #[serde(default = "default_interface")]
    interface: String,
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_show_rates")]
    show_rates: bool,
    #[serde(default)]
    style: ModuleStyle,
}

#[derive(Clone)]
pub struct NetworkVisual {
    pub interface: String,
    pub download_text: String,
    pub upload_text: String,
}

pub struct NetworkModule {
    config: NetworkConfig,
    previous: Option<(Instant, u64, u64)>,
    resolved_interface: Option<String>,
    visual: NetworkVisual,
    revision: u64,
}

impl NetworkModule {
    pub fn load() -> Result<Self> {
        let config: NetworkConfig = config::load_module(NAME)?;
        ensure!(
            config.interval_ms > 0,
            "network interval_ms must be greater than zero"
        );
        ensure!(
            !config.interface.trim().is_empty(),
            "network interface must not be empty"
        );
        config.style.validate()?;
        let _ = crate::network_popup::NetworkPopupConfig::load()?;
        Ok(Self {
            config,
            previous: None,
            resolved_interface: None,
            visual: NetworkVisual {
                interface: String::new(),
                download_text: "↓ --".into(),
                upload_text: "↑ --".into(),
            },
            revision: 0,
        })
    }

    fn interface(&mut self) -> Result<String> {
        if self.config.interface != "auto" {
            return Ok(self.config.interface.clone());
        }

        if let Some(interface) = &self.resolved_interface
            && interface_is_up(interface)
        {
            return Ok(interface.clone());
        }

        let interface = default_route_interface()
            .or_else(first_up_interface)
            .context("no active network interface found")?;
        self.resolved_interface = Some(interface.clone());
        Ok(interface)
    }

    fn counters(interface: &str) -> Result<(u64, u64)> {
        let root = PathBuf::from("/sys/class/net")
            .join(interface)
            .join("statistics");
        let rx = fs::read_to_string(root.join("rx_bytes"))
            .with_context(|| format!("failed to read {interface} rx_bytes"))?
            .trim()
            .parse::<u64>()
            .context("invalid rx_bytes value")?;
        let tx = fs::read_to_string(root.join("tx_bytes"))
            .with_context(|| format!("failed to read {interface} tx_bytes"))?
            .trim()
            .parse::<u64>()
            .context("invalid tx_bytes value")?;
        Ok((rx, tx))
    }
}

impl StatusModule for NetworkModule {
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
        let interface = self.interface()?;
        let (rx, tx) = Self::counters(&interface)?;
        let now = Instant::now();

        let rates = self.previous.and_then(|(before, old_rx, old_tx)| {
            let elapsed = now.saturating_duration_since(before).as_secs_f64();
            (elapsed > 0.0).then(|| {
                (
                    rx.saturating_sub(old_rx) as f64 / elapsed,
                    tx.saturating_sub(old_tx) as f64 / elapsed,
                )
            })
        });
        self.previous = Some((now, rx, tx));

        let (download_text, upload_text) = if self.config.show_rates {
            if let Some((rx_rate, tx_rate)) = rates {
                (
                    format!("↓ {}", format_rate(rx_rate)),
                    format!("↑ {}", format_rate(tx_rate)),
                )
            } else {
                ("↓ --".into(), "↑ --".into())
            }
        } else {
            ("↓".into(), "↑".into())
        };

        let changed = self.visual.interface != interface
            || self.visual.download_text != download_text
            || self.visual.upload_text != upload_text;
        self.visual.interface = interface;
        self.visual.download_text = download_text;
        self.visual.upload_text = upload_text;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }

        Ok(String::new())
    }

    fn visual(&self) -> ModuleVisual {
        ModuleVisual::Network(self.visual.clone())
    }

    fn visual_revision(&self) -> u64 {
        self.revision
    }
}

fn default_route_interface() -> Option<String> {
    let content = fs::read_to_string("/proc/net/route").ok()?;
    parse_default_route(&content)
}

fn parse_default_route(content: &str) -> Option<String> {
    content.lines().skip(1).find_map(|line| {
        let mut fields = line.split_whitespace();
        let interface = fields.next()?;
        let destination = fields.next()?;
        (destination == "00000000" && interface != "lo").then(|| interface.to_owned())
    })
}

fn first_up_interface() -> Option<String> {
    let entries = fs::read_dir("/sys/class/net").ok()?;
    let mut names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "lo" && interface_is_up(name))
        .collect::<Vec<_>>();
    names.sort();
    names.into_iter().next()
}

fn interface_is_up(interface: &str) -> bool {
    fs::read_to_string(
        PathBuf::from("/sys/class/net")
            .join(interface)
            .join("operstate"),
    )
    .is_ok_and(|state| matches!(state.trim(), "up" | "unknown"))
}

fn format_rate(bytes_per_second: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    if bytes_per_second >= GIB {
        format!("{:.1}G/s", bytes_per_second / GIB)
    } else if bytes_per_second >= MIB {
        format!("{:.1}M/s", bytes_per_second / MIB)
    } else if bytes_per_second >= KIB {
        format!("{:.1}K/s", bytes_per_second / KIB)
    } else {
        format!("{:.0}B/s", bytes_per_second.max(0.0))
    }
}

fn default_interface() -> String {
    "auto".into()
}

fn default_interval_ms() -> u64 {
    1_000
}

fn default_show_rates() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{format_rate, parse_default_route};

    #[test]
    fn parses_default_route_interface() {
        let input = "Iface\tDestination\tGateway\tFlags\nlo\t00000000\t00000000\t0001\neth0\t00000000\t01010101\t0003\n";
        assert_eq!(parse_default_route(input).as_deref(), Some("eth0"));
    }

    #[test]
    fn formats_network_rates() {
        assert_eq!(format_rate(512.0), "512B/s");
        assert_eq!(format_rate(2048.0), "2.0K/s");
        assert_eq!(format_rate(2.5 * 1024.0 * 1024.0), "2.5M/s");
    }
}
