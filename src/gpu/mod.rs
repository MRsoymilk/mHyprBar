mod drm_sysfs;
mod nvidia_smi;

use anyhow::{Result, bail};

pub use drm_sysfs::DrmSysfsBackend;
pub use nvidia_smi::NvidiaSmiBackend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

impl GpuVendor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nvidia => "NVIDIA",
            Self::Amd => "AMD",
            Self::Intel => "Intel",
            Self::Unknown => "GPU",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuProcess {
    pub pid: u32,
    pub kind: String,
    pub gpu_percent: Option<f32>,
    pub memory_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct GpuStats {
    pub vendor: GpuVendor,
    pub name: String,
    pub utilization_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub temperature_c: Option<f32>,
    pub power_w: Option<f32>,
}

impl GpuStats {
    pub fn memory_percent(&self) -> Option<f32> {
        let total = self.memory_total_bytes?;
        if total == 0 {
            return None;
        }
        let used = self.memory_used_bytes?.min(total);
        Some((100.0 * used as f64 / total as f64) as f32)
    }
}

pub trait GpuBackend {
    fn backend_name(&self) -> &'static str;
    fn sample(&mut self) -> Result<GpuStats>;
    fn processes(&mut self, _limit: usize) -> Result<Vec<GpuProcess>> {
        Ok(Vec::new())
    }
}

pub fn create_backend(kind: &str, device: &str) -> Result<Box<dyn GpuBackend>> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => {
            if let Ok(backend) = NvidiaSmiBackend::discover(device) {
                return Ok(Box::new(backend));
            }
            if let Ok(backend) = DrmSysfsBackend::discover(None, device) {
                return Ok(Box::new(backend));
            }
            bail!("no supported GPU backend found")
        }
        "nvidia" | "nvidia-smi" => Ok(Box::new(NvidiaSmiBackend::discover(device)?)),
        "amd" => Ok(Box::new(DrmSysfsBackend::discover(
            Some(GpuVendor::Amd),
            device,
        )?)),
        "intel" => Ok(Box::new(DrmSysfsBackend::discover(
            Some(GpuVendor::Intel),
            device,
        )?)),
        "drm" | "sysfs" => Ok(Box::new(DrmSysfsBackend::discover(None, device)?)),
        other => {
            bail!("unsupported gpu backend {other:?}; expected auto, nvidia, amd, intel, or drm")
        }
    }
}
