//! GPU vendor telemetry adapters.
//!
//! NVIDIA: NVML is loaded at runtime from the installed driver (never
//! bundled). If it cannot be loaded, NVIDIA telemetry is reported as
//! unavailable. AMD: amdgpu sysfs is read by the Linux provider.

use be_core::inventory::{GpuDevice, Inventory, SensorKind, SensorReading};
use be_core::time::UtcMillis;

/// Probe NVML. Returns `Err(reason)` when unavailable.
#[cfg(feature = "nvml")]
pub fn nvml_probe() -> Result<u32, String> {
    use nvml_wrapper::Nvml;
    let nvml = Nvml::init().map_err(|e| format!("NVML not available: {e}"))?;
    nvml.device_count()
        .map_err(|e| format!("NVML device count failed: {e}"))
}

#[cfg(not(feature = "nvml"))]
pub fn nvml_probe() -> Result<u32, String> {
    Err("Built without NVML support".into())
}

#[cfg(feature = "nvml")]
fn with_nvml<T>(f: impl FnOnce(&nvml_wrapper::Nvml) -> T) -> Option<T> {
    use std::sync::OnceLock;
    static NVML: OnceLock<Option<nvml_wrapper::Nvml>> = OnceLock::new();
    NVML.get_or_init(|| nvml_wrapper::Nvml::init().ok())
        .as_ref()
        .map(f)
}

/// Merge NVML-provided device data into an inventory (matching on PCI address).
pub fn merge_nvml(inv: &mut Inventory) {
    #[cfg(feature = "nvml")]
    {
        let has_nvidia = inv
            .gpus
            .iter()
            .any(|g| g.vendor.as_deref() == Some("NVIDIA"));
        let merged = with_nvml(|nvml| {
            let count = nvml.device_count().unwrap_or(0);
            for i in 0..count {
                let Ok(dev) = nvml.device_by_index(i) else {
                    continue;
                };
                let bus = dev.pci_info().ok().map(|p| normalize_bus_id(&p.bus_id));
                let mem = dev.memory_info().ok();
                let entry = GpuDevice {
                    id: bus.clone().unwrap_or(format!("nvml:{i}")),
                    vendor: Some("NVIDIA".into()),
                    name: dev.name().ok(),
                    driver: nvml
                        .sys_driver_version()
                        .ok()
                        .map(|v| format!("nvidia {v}")),
                    vram_total_bytes: mem.as_ref().map(|m| m.total),
                    vram_used_bytes: mem.as_ref().map(|m| m.used),
                    pci_address: bus.clone(),
                    telemetry_source: Some("NVML".into()),
                };
                match inv
                    .gpus
                    .iter_mut()
                    .find(|g| bus.is_some() && g.pci_address == bus)
                {
                    Some(g) => *g = entry,
                    None => inv.gpus.push(entry),
                }
            }
        });
        if merged.is_none() && has_nvidia {
            inv.note_unavailable(
                "NVIDIA GPU telemetry",
                "NVML could not be loaded (NVIDIA driver not installed or not accessible)",
            );
        }
    }
    #[cfg(not(feature = "nvml"))]
    let _ = inv;
}

/// NVML sensor readings (temperature, power, fan, utilization, clocks).
pub fn nvml_sensors() -> Vec<SensorReading> {
    #[cfg(feature = "nvml")]
    {
        use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
        let now = UtcMillis::now();
        with_nvml(|nvml| {
            let mut out = Vec::new();
            let count = nvml.device_count().unwrap_or(0);
            for i in 0..count {
                let Ok(dev) = nvml.device_by_index(i) else {
                    continue;
                };
                let chip = format!("GPU {i}");
                let mut push = |key: &str, label: &str, kind: SensorKind, value: Option<f64>| {
                    out.push(SensorReading {
                        id: format!("nvml:{i}:{key}"),
                        chip: chip.clone(),
                        label: label.into(),
                        kind,
                        value,
                        unit: kind.unit().into(),
                        crit: None,
                        max: None,
                        ts: now,
                        source: "NVML".into(),
                    })
                };
                push(
                    "temp",
                    "GPU temperature",
                    SensorKind::Temperature,
                    dev.temperature(TemperatureSensor::Gpu)
                        .ok()
                        .map(|t| t as f64),
                );
                push(
                    "power",
                    "Board power",
                    SensorKind::Power,
                    dev.power_usage().ok().map(|mw| mw as f64 / 1000.0),
                );
                push(
                    "fan0",
                    "Fan 0",
                    SensorKind::Utilization,
                    dev.fan_speed(0).ok().map(|p| p as f64),
                );
                push(
                    "util",
                    "GPU utilization",
                    SensorKind::Utilization,
                    dev.utilization_rates().ok().map(|u| u.gpu as f64),
                );
                push(
                    "sm_clock",
                    "SM clock",
                    SensorKind::Frequency,
                    dev.clock_info(Clock::SM).ok().map(|c| c as f64),
                );
            }
            out
        })
        .unwrap_or_default()
    }
    #[cfg(not(feature = "nvml"))]
    Vec::new()
}

/// Point-in-time GPU status from a vendor interface. `None` fields are
/// unavailable (never zero-filled).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GpuStatus {
    pub source: String,
    pub temperature_c: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub sm_clock_mhz: Option<f64>,
    pub max_sm_clock_mhz: Option<f64>,
    pub utilization_pct: Option<f64>,
    /// Active clock-throttle reasons reported by the driver.
    pub throttle_reasons: Vec<String>,
    /// PCIe replay counter (link-level retransmissions) where supported.
    pub pcie_replay_count: Option<u64>,
}

/// NVIDIA status by PCI address (`0000:01:00.0`). `None` if NVML or the
/// device is unavailable.
pub fn nvml_status(pci_address: &str) -> Option<GpuStatus> {
    #[cfg(feature = "nvml")]
    {
        use nvml_wrapper::bitmasks::device::ThrottleReasons;
        use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
        with_nvml(|nvml| {
            let dev = nvml.device_by_pci_bus_id(pci_address).ok()?;
            let reasons = dev.current_throttle_reasons().ok();
            let mut names = Vec::new();
            if let Some(r) = reasons {
                for (flag, name) in [
                    (ThrottleReasons::SW_POWER_CAP, "software power cap"),
                    (ThrottleReasons::HW_SLOWDOWN, "hardware slowdown"),
                    (
                        ThrottleReasons::SW_THERMAL_SLOWDOWN,
                        "software thermal slowdown",
                    ),
                    (
                        ThrottleReasons::HW_THERMAL_SLOWDOWN,
                        "hardware thermal slowdown",
                    ),
                    (
                        ThrottleReasons::HW_POWER_BRAKE_SLOWDOWN,
                        "hardware power brake",
                    ),
                    (ThrottleReasons::SYNC_BOOST, "sync boost"),
                ] {
                    if r.contains(flag) {
                        names.push(name.to_string());
                    }
                }
            }
            Some(GpuStatus {
                source: "NVML".into(),
                temperature_c: dev
                    .temperature(TemperatureSensor::Gpu)
                    .ok()
                    .map(|t| t as f64),
                power_w: dev.power_usage().ok().map(|mw| mw as f64 / 1000.0),
                power_limit_w: dev.enforced_power_limit().ok().map(|mw| mw as f64 / 1000.0),
                sm_clock_mhz: dev.clock_info(Clock::SM).ok().map(|c| c as f64),
                max_sm_clock_mhz: dev.max_clock_info(Clock::SM).ok().map(|c| c as f64),
                utilization_pct: dev.utilization_rates().ok().map(|u| u.gpu as f64),
                throttle_reasons: names,
                pcie_replay_count: dev.pcie_replay_counter().ok().map(|c| c as u64),
            })
        })
        .flatten()
    }
    #[cfg(not(feature = "nvml"))]
    {
        let _ = pci_address;
        None
    }
}

/// AMD (amdgpu) status from sysfs/hwmon on Linux. `None` if unavailable.
pub fn amdgpu_status(pci_address: &str) -> Option<GpuStatus> {
    #[cfg(target_os = "linux")]
    {
        let base = std::path::PathBuf::from(format!("/sys/bus/pci/devices/{pci_address}"));
        if !base.join("gpu_busy_percent").exists() && !base.join("pp_dpm_sclk").exists() {
            return None;
        }
        let read = |p: &std::path::Path| {
            std::fs::read_to_string(p)
                .ok()
                .map(|s| s.trim().to_string())
        };
        let hw = std::fs::read_dir(base.join("hwmon"))
            .ok()
            .and_then(|mut d| d.next())
            .and_then(|e| e.ok())
            .map(|e| e.path());
        let hwv = |f: &str, scale: f64| {
            hw.as_ref()
                .and_then(|h| read(&h.join(f)))
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| v * scale)
        };
        // pp_dpm_sclk lines look like "1: 1800Mhz *" (current level marked).
        let sclk = read(&base.join("pp_dpm_sclk")).unwrap_or_default();
        let parse_mhz = |l: &str| {
            l.split_whitespace().nth(1).and_then(|v| {
                v.trim_end_matches("Mhz")
                    .trim_end_matches("MHz")
                    .parse::<f64>()
                    .ok()
            })
        };
        let cur = sclk
            .lines()
            .find(|l| l.trim_end().ends_with('*'))
            .and_then(parse_mhz);
        let max = sclk
            .lines()
            .filter_map(parse_mhz)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        Some(GpuStatus {
            source: "amdgpu sysfs".into(),
            temperature_c: hwv("temp1_input", 1e-3),
            power_w: hwv("power1_average", 1e-6).or_else(|| hwv("power1_input", 1e-6)),
            power_limit_w: hwv("power1_cap", 1e-6),
            sm_clock_mhz: cur,
            max_sm_clock_mhz: max,
            utilization_pct: read(&base.join("gpu_busy_percent")).and_then(|v| v.parse().ok()),
            throttle_reasons: Vec::new(),
            pcie_replay_count: None,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pci_address;
        None
    }
}

/// Best available vendor status for a GPU at `pci_address`.
pub fn gpu_status(pci_address: &str) -> Option<GpuStatus> {
    nvml_status(pci_address).or_else(|| amdgpu_status(pci_address))
}

/// NVML reports `00000000:01:00.0`; sysfs uses `0000:01:00.0`.
pub fn normalize_bus_id(s: &str) -> String {
    let s = s.trim().to_ascii_lowercase();
    match s.split_once(':') {
        Some((dom, rest)) if dom.len() > 4 => format!("{}:{rest}", &dom[dom.len() - 4..]),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bus_id_normalization() {
        assert_eq!(super::normalize_bus_id("00000000:01:00.0"), "0000:01:00.0");
        assert_eq!(super::normalize_bus_id("0000:41:00.0"), "0000:41:00.0");
    }
}
