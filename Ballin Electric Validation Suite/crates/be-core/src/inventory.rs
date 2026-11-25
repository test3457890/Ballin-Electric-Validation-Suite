//! Hardware inventory data model. Every field that may be unknown is an
//! `Option`; providers record *why* something is unavailable.

use crate::time::UtcMillis;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub collected_utc: Option<UtcMillis>,
    pub platform: String,
    pub provider: String,
    pub system: SystemIdentity,
    pub os: OsInfo,
    pub cpu: Option<CpuInfo>,
    pub memory: Option<MemoryInfo>,
    pub storage: Vec<StorageDevice>,
    pub pci: Vec<PciDevice>,
    pub gpus: Vec<GpuDevice>,
    pub network: Vec<NetworkInterface>,
    pub sensors: Vec<SensorReading>,
    pub firmware: FirmwareInfo,
    pub usb: Vec<UsbDevice>,
    pub unavailable: Vec<Unavailable>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemIdentity {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub board_vendor: Option<String>,
    pub board_name: Option<String>,
    pub chassis_type: Option<String>,
    pub hostname: Option<String>,
    pub virtualized: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    pub kernel: Option<String>,
    pub arch: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CpuInfo {
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub packages: Option<u32>,
    pub physical_cores: Option<u32>,
    pub logical_cpus: u32,
    pub base_mhz: Option<f64>,
    pub max_mhz: Option<f64>,
    pub current_mhz: Vec<Option<f64>>,
    pub features: Vec<String>,
    pub cache: Vec<String>,
    pub microcode: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub ecc_controllers: Vec<EccController>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EccController {
    pub name: String,
    pub corrected: Option<u64>,
    pub uncorrected: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageDevice {
    pub name: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub size_bytes: Option<u64>,
    pub rotational: Option<bool>,
    pub removable: Option<bool>,
    pub transport: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PciDevice {
    pub address: String,
    pub vendor_id: Option<u16>,
    pub device_id: Option<u16>,
    pub class_code: Option<u32>,
    pub class_name: String,
    pub driver: Option<String>,
    pub current_link_speed: Option<String>,
    pub current_link_width: Option<u32>,
    pub max_link_speed: Option<String>,
    pub max_link_width: Option<u32>,
    pub aer_correctable: Option<u64>,
    pub aer_nonfatal: Option<u64>,
    pub aer_fatal: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuDevice {
    /// Stable identifier: PCI address where available.
    pub id: String,
    pub vendor: Option<String>,
    pub name: Option<String>,
    pub driver: Option<String>,
    pub vram_total_bytes: Option<u64>,
    pub vram_used_bytes: Option<u64>,
    pub pci_address: Option<String>,
    pub telemetry_source: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub mac: Option<String>,
    pub driver: Option<String>,
    pub oper_state: Option<String>,
    pub speed_mbps: Option<i64>,
    pub rx_errors: Option<u64>,
    pub tx_errors: Option<u64>,
    pub is_virtual: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsbDevice {
    pub path: String,
    pub vendor_id: Option<String>,
    pub product_id: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub speed: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirmwareInfo {
    pub bios_vendor: Option<String>,
    pub bios_version: Option<String>,
    pub bios_date: Option<String>,
    pub uefi: Option<bool>,
    pub secure_boot: Option<bool>,
    pub tpm_present: Option<bool>,
    pub tpm_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    Temperature,
    Fan,
    Voltage,
    Power,
    Current,
    Frequency,
    Utilization,
    Other,
}

impl SensorKind {
    pub fn unit(self) -> &'static str {
        match self {
            SensorKind::Temperature => "°C",
            SensorKind::Fan => "RPM",
            SensorKind::Voltage => "V",
            SensorKind::Power => "W",
            SensorKind::Current => "A",
            SensorKind::Frequency => "MHz",
            SensorKind::Utilization => "%",
            SensorKind::Other => "",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SensorKind::Temperature => "Temperature",
            SensorKind::Fan => "Fan",
            SensorKind::Voltage => "Voltage",
            SensorKind::Power => "Power",
            SensorKind::Current => "Current",
            SensorKind::Frequency => "Clock",
            SensorKind::Utilization => "Utilization",
            SensorKind::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorReading {
    /// Stable id, e.g. `hwmon:coretemp:temp1`.
    pub id: String,
    pub chip: String,
    pub label: String,
    pub kind: SensorKind,
    /// `None` = read failed / unknown. Never zero-filled.
    pub value: Option<f64>,
    pub unit: String,
    pub crit: Option<f64>,
    pub max: Option<f64>,
    pub ts: UtcMillis,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unavailable {
    pub item: String,
    pub reason: String,
}

impl Inventory {
    pub fn note_unavailable(&mut self, item: impl Into<String>, reason: impl Into<String>) {
        self.unavailable.push(Unavailable {
            item: item.into(),
            reason: reason.into(),
        });
    }

    pub fn temperature_sensors(&self) -> impl Iterator<Item = &SensorReading> {
        self.sensors
            .iter()
            .filter(|s| s.kind == SensorKind::Temperature)
    }
}

pub fn format_bytes(b: u64) -> String {
    const U: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(16 * 1024 * 1024 * 1024), "16.0 GiB");
    }
}
