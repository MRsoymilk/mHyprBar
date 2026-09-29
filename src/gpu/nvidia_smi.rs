use std::process::Command;

use anyhow::{Context, Result, ensure};

use super::{GpuBackend, GpuProcess, GpuStats, GpuVendor};

pub struct NvidiaSmiBackend {
    index: String,
}

impl NvidiaSmiBackend {
    pub fn discover(device: &str) -> Result<Self> {
        let output = Command::new("nvidia-smi")
            .arg("--query-gpu=index")
            .arg("--format=csv,noheader,nounits")
            .output()
            .context("failed to launch nvidia-smi")?;
        ensure!(
            output.status.success(),
            "nvidia-smi GPU discovery failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );

        let indices = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        ensure!(!indices.is_empty(), "nvidia-smi reported no GPUs");

        let requested = device.trim();
        let index = if requested.is_empty() || requested.eq_ignore_ascii_case("auto") {
            indices[0].clone()
        } else {
            ensure!(
                indices.iter().any(|index| index == requested),
                "NVIDIA GPU index {requested:?} was not found"
            );
            requested.to_owned()
        };

        Ok(Self { index })
    }
}

impl GpuBackend for NvidiaSmiBackend {
    fn backend_name(&self) -> &'static str {
        "nvidia-smi"
    }

    fn sample(&mut self) -> Result<GpuStats> {
        let output = Command::new("nvidia-smi")
            .arg(format!("--id={}", self.index))
            .arg("--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw")
            .arg("--format=csv,noheader,nounits")
            .output()
            .context("failed to launch nvidia-smi")?;
        ensure!(
            output.status.success(),
            "nvidia-smi sampling failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .context("nvidia-smi returned no GPU data")?;
        parse_sample(line)
    }

    fn processes(&mut self, limit: usize) -> Result<Vec<GpuProcess>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let output = Command::new("nvidia-smi")
            .arg("pmon")
            .arg("-c")
            .arg("1")
            .arg("-s")
            .arg("um")
            .output()
            .context("failed to launch nvidia-smi pmon")?;
        ensure!(
            output.status.success(),
            "nvidia-smi pmon failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut rows = stdout
            .lines()
            .filter_map(|line| parse_pmon_process(line, &self.index))
            .collect::<Vec<_>>();

        rows.sort_by(|a, b| {
            b.gpu_percent
                .unwrap_or(-1.0)
                .total_cmp(&a.gpu_percent.unwrap_or(-1.0))
                .then_with(|| {
                    b.memory_percent
                        .unwrap_or(-1.0)
                        .total_cmp(&a.memory_percent.unwrap_or(-1.0))
                })
                .then_with(|| b.memory_bytes.cmp(&a.memory_bytes))
                .then_with(|| a.pid.cmp(&b.pid))
        });
        rows.truncate(limit);
        Ok(rows)
    }
}

fn parse_pmon_process(line: &str, gpu_index: &str) -> Option<GpuProcess> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 12 || fields[0] != gpu_index {
        return None;
    }

    let pid = fields[1].parse::<u32>().ok()?;
    let kind = fields[2].to_owned();
    let gpu_percent = parse_optional_f32(fields[3]);
    let memory_percent = parse_optional_f32(fields[4]);
    let memory_bytes = parse_optional_f64(fields[9]).map(mib_to_bytes);
    let name = fields[11..].join(" ");

    Some(GpuProcess {
        pid,
        kind,
        gpu_percent,
        memory_percent,
        memory_bytes,
        name,
    })
}

fn parse_sample(line: &str) -> Result<GpuStats> {
    let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
    ensure!(
        fields.len() >= 6,
        "unexpected nvidia-smi field count: {}",
        fields.len()
    );

    let memory_used_mib = parse_optional_f64(fields[2]);
    let memory_total_mib = parse_optional_f64(fields[3]);

    Ok(GpuStats {
        vendor: GpuVendor::Nvidia,
        name: fields[0].to_owned(),
        utilization_percent: parse_optional_f32(fields[1]),
        memory_used_bytes: memory_used_mib.map(mib_to_bytes),
        memory_total_bytes: memory_total_mib.map(mib_to_bytes),
        temperature_c: parse_optional_f32(fields[4]),
        power_w: parse_optional_f32(fields[5]),
    })
}

fn parse_optional_f32(value: &str) -> Option<f32> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("n/a") || value == "[N/A]" {
        None
    } else {
        value.parse::<f32>().ok()
    }
}

fn parse_optional_f64(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("n/a") || value == "[N/A]" {
        None
    } else {
        value.parse::<f64>().ok()
    }
}

fn mib_to_bytes(value: f64) -> u64 {
    (value.max(0.0) * 1024.0 * 1024.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::{parse_pmon_process, parse_sample};

    #[test]
    fn parses_pmon_graphics_process() {
        let process =
            parse_pmon_process("0 2200 G 11 6 - - - - 1 0 Hyprland", "0").expect("pmon process");
        assert_eq!(process.pid, 2200);
        assert_eq!(process.kind, "G");
        assert_eq!(process.gpu_percent, Some(11.0));
        assert_eq!(process.memory_percent, Some(6.0));
        assert_eq!(process.memory_bytes, Some(1024 * 1024));
        assert_eq!(process.name, "Hyprland");
    }

    #[test]
    fn parses_nvidia_smi_csv() {
        let stats =
            parse_sample("NVIDIA GeForce GTX 1060, 12, 43, 6144, 51, 5.37").expect("sample");
        assert_eq!(stats.utilization_percent, Some(12.0));
        assert_eq!(stats.temperature_c, Some(51.0));
        assert_eq!(stats.memory_total_bytes, Some(6144 * 1024 * 1024));
        assert_eq!(stats.memory_used_bytes, Some(43 * 1024 * 1024));
    }
}
