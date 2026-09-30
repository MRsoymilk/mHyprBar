use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use super::{GpuBackend, GpuStats, GpuVendor};

pub struct DrmSysfsBackend {
    card_name: String,
    device_path: PathBuf,
    vendor: GpuVendor,
}

impl DrmSysfsBackend {
    pub fn discover(vendor_filter: Option<GpuVendor>, device: &str) -> Result<Self> {
        let mut cards = fs::read_dir("/sys/class/drm")
            .context("failed to read /sys/class/drm")?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                is_card_name(&name).then_some((name, entry.path()))
            })
            .collect::<Vec<_>>();
        cards.sort_by_key(|(name, _)| card_number(name).unwrap_or(u32::MAX));

        let requested = normalize_device(device);
        for (card_name, card_path) in cards {
            if let Some(requested) = requested.as_deref()
                && card_name != requested
            {
                continue;
            }

            let device_path = card_path.join("device");
            if !device_path.exists() {
                continue;
            }
            let vendor = read_vendor(&device_path).unwrap_or(GpuVendor::Unknown);
            if vendor_filter.is_some_and(|expected| expected != vendor) {
                continue;
            }

            return Ok(Self {
                card_name,
                device_path,
                vendor,
            });
        }

        if let Some(vendor) = vendor_filter {
            bail!("no {} DRM GPU matched device {device:?}", vendor.label());
        }
        bail!("no DRM GPU matched device {device:?}")
    }
}

impl GpuBackend for DrmSysfsBackend {
    fn backend_name(&self) -> &'static str {
        "drm-sysfs"
    }

    fn sample(&mut self) -> Result<GpuStats> {
        let utilization_percent = read_number::<f32>(&self.device_path.join("gpu_busy_percent"))
            .map(|value| value.clamp(0.0, 100.0));

        let memory_used_bytes = read_number::<u64>(&self.device_path.join("mem_info_vram_used"));
        let memory_total_bytes = read_number::<u64>(&self.device_path.join("mem_info_vram_total"));

        let hwmon = find_hwmon(&self.device_path);
        let temperature_c = hwmon
            .as_ref()
            .and_then(|path| read_number::<f32>(&path.join("temp1_input")))
            .map(|millidegrees| millidegrees / 1000.0);
        let power_w = hwmon
            .as_ref()
            .and_then(|path| read_number::<f32>(&path.join("power1_average")))
            .map(|microwatts| microwatts / 1_000_000.0);

        Ok(GpuStats {
            vendor: self.vendor,
            name: format!("{} {}", self.vendor.label(), self.card_name),
            utilization_percent,
            memory_used_bytes,
            memory_total_bytes,
            temperature_c,
            power_w,
        })
    }
}

fn is_card_name(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
}

fn card_number(name: &str) -> Option<u32> {
    name.strip_prefix("card")?.parse().ok()
}

fn normalize_device(device: &str) -> Option<String> {
    let device = device.trim();
    if device.is_empty() || device.eq_ignore_ascii_case("auto") {
        None
    } else if device.chars().all(|ch| ch.is_ascii_digit()) {
        Some(format!("card{device}"))
    } else {
        Some(device.to_owned())
    }
}

fn read_vendor(device_path: &Path) -> Result<GpuVendor> {
    let raw =
        fs::read_to_string(device_path.join("vendor")).context("failed to read DRM vendor id")?;
    let value = raw.trim().trim_start_matches("0x");
    let id = u16::from_str_radix(value, 16).context("invalid DRM vendor id")?;
    Ok(match id {
        0x10de => GpuVendor::Nvidia,
        0x1002 => GpuVendor::Amd,
        0x8086 => GpuVendor::Intel,
        _ => GpuVendor::Unknown,
    })
}

fn read_number<T>(path: &Path) -> Option<T>
where
    T: std::str::FromStr,
{
    fs::read_to_string(path).ok()?.trim().parse::<T>().ok()
}

fn find_hwmon(device_path: &Path) -> Option<PathBuf> {
    let root = device_path.join("hwmon");
    let mut entries = fs::read_dir(root).ok()?.filter_map(Result::ok);
    entries.next().map(|entry| entry.path())
}

#[cfg(test)]
mod tests {
    use super::{is_card_name, normalize_device};

    #[test]
    fn recognizes_only_primary_drm_card_nodes() {
        assert!(is_card_name("card0"));
        assert!(is_card_name("card12"));
        assert!(!is_card_name("card0-DP-1"));
        assert!(!is_card_name("renderD128"));
    }

    #[test]
    fn normalizes_drm_device_selection() {
        assert_eq!(normalize_device("auto"), None);
        assert_eq!(normalize_device("0").as_deref(), Some("card0"));
        assert_eq!(normalize_device("card1").as_deref(), Some("card1"));
    }
}
