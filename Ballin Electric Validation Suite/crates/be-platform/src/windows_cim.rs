//! Windows provider using documented WMI/CIM classes.
//!
//! The query is executed with the in-box Windows PowerShell
//! `Get-CimInstance` cmdlet and returned as structured JSON (no screen
//! scraping). Parsing is platform independent so it can be fixture-tested
//! on any host; execution is only compiled on Windows.
//!
//! STATUS: implemented and fixture-tested; NOT yet validated on physical
//! Windows hardware. A native `windows`/`wmi` crate backend may replace the
//! PowerShell bridge after license and API review.

use be_core::inventory::*;
use be_core::time::UtcMillis;
use serde_json::Value;

/// PowerShell script producing one JSON document with all queried classes.
pub const CIM_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$o = [ordered]@{}
$o.cs    = Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,Name,HypervisorPresent
$o.bios  = Get-CimInstance Win32_BIOS | Select-Object Manufacturer,SMBIOSBIOSVersion,ReleaseDate,SerialNumber
$o.board = Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product
$o.os    = Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber,TotalVisibleMemorySize,FreePhysicalMemory
$o.cpu   = @(Get-CimInstance Win32_Processor | Select-Object Name,Manufacturer,NumberOfCores,NumberOfLogicalProcessors,MaxClockSpeed,CurrentClockSpeed,L2CacheSize,L3CacheSize)
$o.disk  = @(Get-CimInstance Win32_DiskDrive | Select-Object DeviceID,Model,SerialNumber,FirmwareRevision,Size,InterfaceType,MediaType,Status)
$o.gpu   = @(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterCompatibility,DriverVersion,AdapterRAM,PNPDeviceID)
$o.net   = @(Get-CimInstance Win32_NetworkAdapter -Filter 'PhysicalAdapter=True' | Select-Object Name,MACAddress,Speed,NetConnectionStatus,ServiceName)
$o.temps = @(Get-CimInstance -Namespace root/wmi MSAcpi_ThermalZoneTemperature | Select-Object InstanceName,CurrentTemperature,CriticalTripPoint)
try { $o.secureboot = Confirm-SecureBootUEFI } catch { $o.secureboot = $null }
$o.tpm = Get-CimInstance -Namespace root/cimv2/security/microsofttpm Win32_Tpm | Select-Object IsEnabled_InitialValue,SpecVersion
$o | ConvertTo-Json -Depth 4 -Compress
"#;

fn s(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(x) if !x.trim().is_empty() => Some(x.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn n(v: &Value, k: &str) -> Option<f64> {
    match v.get(k)? {
        Value::Number(x) => x.as_f64(),
        Value::String(x) => x.trim().parse().ok(),
        _ => None,
    }
}

fn arr(v: &Value, k: &str) -> Vec<Value> {
    match v.get(k) {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Null) | None => Vec::new(),
        Some(x) => vec![x.clone()],
    }
}

/// Convert the CIM JSON document into an inventory.
pub fn parse_cim(json: &str, provider_name: &str) -> Inventory {
    let mut inv = Inventory {
        collected_utc: Some(UtcMillis::now()),
        platform: "windows".into(),
        provider: provider_name.into(),
        ..Default::default()
    };
    inv.os.arch = std::env::consts::ARCH.into();
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            inv.note_unavailable(
                "All inventory",
                format!("CIM output could not be parsed: {e}"),
            );
            return inv;
        }
    };
    let null = Value::Null;
    let cs = v.get("cs").unwrap_or(&null);
    let bios = v.get("bios").unwrap_or(&null);
    let board = v.get("board").unwrap_or(&null);
    let os = v.get("os").unwrap_or(&null);
    inv.system = SystemIdentity {
        manufacturer: s(cs, "Manufacturer"),
        product: s(cs, "Model"),
        serial: s(bios, "SerialNumber"),
        board_vendor: s(board, "Manufacturer"),
        board_name: s(board, "Product"),
        chassis_type: None,
        hostname: s(cs, "Name"),
        virtualized: match cs.get("HypervisorPresent") {
            Some(Value::Bool(true)) => Some("hypervisor present".into()),
            _ => None,
        },
    };
    inv.os.name = s(os, "Caption");
    inv.os.version = s(os, "Version");
    inv.os.kernel = s(os, "BuildNumber").map(|b| format!("build {b}"));
    inv.firmware = FirmwareInfo {
        bios_vendor: s(bios, "Manufacturer"),
        bios_version: s(bios, "SMBIOSBIOSVersion"),
        bios_date: s(bios, "ReleaseDate"),
        uefi: match v.get("secureboot") {
            Some(Value::Bool(_)) => Some(true),
            _ => None,
        },
        secure_boot: v.get("secureboot").and_then(|b| b.as_bool()),
        tpm_present: v.get("tpm").map(|t| !t.is_null()),
        tpm_version: v.get("tpm").and_then(|t| s(t, "SpecVersion")),
    };
    if inv.firmware.secure_boot.is_none() {
        inv.note_unavailable(
            "Secure Boot state",
            "Confirm-SecureBootUEFI unavailable (legacy BIOS or requires administrator)",
        );
    }
    if v.get("tpm").map(|t| t.is_null()).unwrap_or(true) {
        inv.firmware.tpm_present = None;
        inv.note_unavailable(
            "TPM",
            "Win32_Tpm query requires administrator or TPM not present",
        );
    }

    let cpus = arr(&v, "cpu");
    if let Some(c) = cpus.first() {
        let mut cpu = CpuInfo {
            vendor: s(c, "Manufacturer"),
            model: s(c, "Name"),
            packages: Some(cpus.len() as u32),
            physical_cores: Some(
                cpus.iter()
                    .filter_map(|c| n(c, "NumberOfCores"))
                    .sum::<f64>() as u32,
            ),
            logical_cpus: cpus
                .iter()
                .filter_map(|c| n(c, "NumberOfLogicalProcessors"))
                .sum::<f64>() as u32,
            max_mhz: n(c, "MaxClockSpeed"),
            ..Default::default()
        };
        cpu.current_mhz = vec![n(c, "CurrentClockSpeed")];
        if let Some(l2) = n(c, "L2CacheSize") {
            cpu.cache.push(format!("L2 {l2} KiB"));
        }
        if let Some(l3) = n(c, "L3CacheSize") {
            cpu.cache.push(format!("L3 {l3} KiB"));
        }
        inv.cpu = Some(cpu);
    } else {
        inv.note_unavailable("CPU", "Win32_Processor returned no data");
    }
    if let Some(total_kib) = n(os, "TotalVisibleMemorySize") {
        inv.memory = Some(MemoryInfo {
            total_bytes: (total_kib as u64) * 1024,
            available_bytes: n(os, "FreePhysicalMemory").map(|f| f as u64 * 1024),
            swap_total_bytes: None,
            ecc_controllers: Vec::new(),
        });
        inv.note_unavailable(
            "ECC error counters",
            "Not yet implemented on Windows (WHEA event review planned)",
        );
    }
    for d in arr(&v, "disk") {
        inv.storage.push(StorageDevice {
            name: s(&d, "DeviceID").unwrap_or_default(),
            model: s(&d, "Model"),
            serial: s(&d, "SerialNumber"),
            firmware: s(&d, "FirmwareRevision"),
            size_bytes: n(&d, "Size").map(|x| x as u64),
            rotational: None,
            removable: s(&d, "MediaType").map(|m| m.to_ascii_lowercase().contains("removable")),
            transport: s(&d, "InterfaceType"),
            state: s(&d, "Status"),
        });
    }
    for (i, g) in arr(&v, "gpu").into_iter().enumerate() {
        inv.gpus.push(GpuDevice {
            id: s(&g, "PNPDeviceID").unwrap_or(format!("gpu{i}")),
            vendor: s(&g, "AdapterCompatibility"),
            name: s(&g, "Name"),
            driver: s(&g, "DriverVersion"),
            // AdapterRAM is a 32-bit field and saturates at 4 GiB: treat
            // values at the limit as unknown rather than reporting them.
            vram_total_bytes: n(&g, "AdapterRAM")
                .map(|x| x as u64)
                .filter(|x| *x > 0 && *x < 0xFFF0_0000),
            vram_used_bytes: None,
            pci_address: None,
            telemetry_source: Some("Win32_VideoController".into()),
        });
    }
    for a in arr(&v, "net") {
        inv.network.push(NetworkInterface {
            name: s(&a, "Name").unwrap_or_default(),
            mac: s(&a, "MACAddress"),
            driver: s(&a, "ServiceName"),
            oper_state: n(&a, "NetConnectionStatus").map(|c| {
                if c == 2.0 {
                    "up".into()
                } else {
                    format!("status {c}")
                }
            }),
            speed_mbps: n(&a, "Speed").map(|b| (b / 1e6) as i64).filter(|x| *x > 0),
            rx_errors: None,
            tx_errors: None,
            is_virtual: false,
        });
    }
    let now = UtcMillis::now();
    for t in arr(&v, "temps") {
        let name = s(&t, "InstanceName").unwrap_or("ACPI thermal zone".into());
        // Tenths of Kelvin.
        let k = |key: &str| n(&t, key).map(|x| x / 10.0 - 273.15);
        inv.sensors.push(SensorReading {
            id: format!("acpi:{name}"),
            chip: "ACPI".into(),
            label: name,
            kind: SensorKind::Temperature,
            value: k("CurrentTemperature"),
            unit: "°C".into(),
            crit: k("CriticalTripPoint"),
            max: None,
            ts: now,
            source: "MSAcpi_ThermalZoneTemperature".into(),
        });
    }
    if inv.sensors.is_empty() {
        inv.note_unavailable(
            "Temperature sensors",
            "No ACPI thermal zones via WMI (often requires administrator; vendor sensor interfaces not yet implemented on Windows)",
        );
    }
    inv.note_unavailable(
        "PCIe link status",
        "Not yet implemented on Windows (SetupAPI/DEVPKEY_PciDevice_* planned)",
    );
    inv
}

#[cfg(windows)]
pub struct WindowsProvider;

#[cfg(windows)]
impl WindowsProvider {
    pub fn new() -> Self {
        WindowsProvider
    }

    fn run_cim() -> Result<String, String> {
        use std::process::Command;
        let out = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                CIM_SCRIPT,
            ])
            .output()
            .map_err(|e| format!("could not start PowerShell: {e}"))?;
        if !out.status.success() {
            return Err(format!("PowerShell exited with {}", out.status));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(windows)]
impl crate::PlatformProvider for WindowsProvider {
    fn name(&self) -> &'static str {
        "windows-cim"
    }
    fn inventory(&self) -> Inventory {
        let mut inv = match Self::run_cim() {
            Ok(j) => parse_cim(&j, self.name()),
            Err(e) => {
                let mut inv = Inventory {
                    platform: "windows".into(),
                    provider: self.name().into(),
                    ..Default::default()
                };
                inv.note_unavailable("All inventory", e);
                inv
            }
        };
        crate::gpu::merge_nvml(&mut inv);
        inv.sensors.extend(crate::gpu::nvml_sensors());
        inv
    }
    fn sample_sensors(&self) -> Vec<SensorReading> {
        // Fast path: NVML only; ACPI zones via PowerShell are too slow for
        // safety supervision sampling.
        crate::gpu::nvml_sensors()
    }
}

#[cfg(test)]
mod tests {
    //! FIXTURE TESTS (synthetic CIM JSON). Not evidence of Windows support.
    use super::*;

    const FIXTURE: &str = r#"{"cs":{"Manufacturer":"Ballin Electric","Model":"BE-01","Name":"BE01-FIX","HypervisorPresent":false},
      "bios":{"Manufacturer":"FixtureBIOS","SMBIOSBIOSVersion":"1.02","ReleaseDate":"2026-01-01","SerialNumber":"FIX-1"},
      "board":{"Manufacturer":"FixBoard","Product":"X1"},
      "os":{"Caption":"Microsoft Windows 11 Pro","Version":"10.0.26100","BuildNumber":"26100","TotalVisibleMemorySize":67108864,"FreePhysicalMemory":50000000},
      "cpu":{"Name":"Fixture CPU","Manufacturer":"FixtureVendor","NumberOfCores":16,"NumberOfLogicalProcessors":32,"MaxClockSpeed":4500,"CurrentClockSpeed":4400,"L2CacheSize":16384,"L3CacheSize":65536},
      "disk":[{"DeviceID":"\\\\.\\PHYSICALDRIVE0","Model":"Fixture NVMe","SerialNumber":"S1","FirmwareRevision":"F1","Size":2000398934016,"InterfaceType":"SCSI","MediaType":"Fixed hard disk media","Status":"OK"}],
      "gpu":[{"Name":"GPU A","AdapterCompatibility":"NVIDIA","DriverVersion":"32.0","AdapterRAM":4293918720,"PNPDeviceID":"PCI\\VEN_10DE"}],
      "net":{"Name":"10GbE","MACAddress":"00:11:22:33:44:55","Speed":10000000000,"NetConnectionStatus":2,"ServiceName":"ixgbe"},
      "temps":[{"InstanceName":"TZ0","CurrentTemperature":3132,"CriticalTripPoint":3732}],
      "secureboot":true,"tpm":null}"#;

    #[test]
    fn parses_fixture_document() {
        let inv = parse_cim(FIXTURE, "windows-cim (FIXTURE)");
        let cpu = inv.cpu.as_ref().unwrap();
        assert_eq!(cpu.logical_cpus, 32);
        assert_eq!(cpu.physical_cores, Some(16));
        assert_eq!(inv.memory.as_ref().unwrap().total_bytes, 67_108_864 * 1024);
        assert_eq!(inv.storage[0].size_bytes, Some(2_000_398_934_016));
        assert_eq!(
            inv.gpus[0].vram_total_bytes, None,
            "saturated AdapterRAM must be unknown"
        );
        assert_eq!(
            inv.network.len(),
            1,
            "single object is accepted as one-element array"
        );
        assert_eq!(inv.network[0].speed_mbps, Some(10000));
        let t = inv.sensors[0].value.unwrap();
        assert!((t - 40.05).abs() < 1e-6, "{t}");
        assert_eq!(inv.firmware.secure_boot, Some(true));
        assert_eq!(inv.firmware.tpm_present, None);
        assert!(inv.unavailable.iter().any(|u| u.item == "TPM"));
    }

    #[test]
    fn garbage_is_reported_not_guessed() {
        let inv = parse_cim("not json", "x");
        assert!(inv.cpu.is_none());
        assert_eq!(inv.unavailable[0].item, "All inventory");
    }
}
