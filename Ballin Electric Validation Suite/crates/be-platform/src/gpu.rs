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
        return with_nvml(|nvml| {
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
        .unwrap_or_default();
    }
    #[cfg(not(feature = "nvml"))]
    Vec::new()
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
