//! Linux provider: procfs, sysfs (hwmon, thermal, PCI, block, net, DMI,
//! EDAC, DRM, USB, TPM, EFI). Read-only. The filesystem root is
//! configurable so that fixture trees can exercise every parsing path in
//! automated tests; production always uses `/`.

use crate::pci_ids;
use be_core::inventory::*;
use be_core::time::UtcMillis;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct LinuxProvider {
    root: PathBuf,
    /// True when `root` is a fixture tree; recorded in the provider name.
    fixture: bool,
}

impl LinuxProvider {
    pub fn system() -> Self {
        LinuxProvider {
            root: PathBuf::from("/"),
            fixture: false,
        }
    }

    /// Fixture provider reading a fake filesystem tree (tests only).
    pub fn with_fixture_root(root: impl Into<PathBuf>) -> Self {
        LinuxProvider {
            root: root.into(),
            fixture: true,
        }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_start_matches('/'))
    }

    fn read(&self, rel: &str) -> io::Result<String> {
        fs::read_to_string(self.p(rel))
    }

    fn read_opt(&self, rel: &str) -> Option<String> {
        read_trim(&self.p(rel))
    }

    fn exists(&self, rel: &str) -> bool {
        self.p(rel).exists()
    }

    fn list(&self, rel: &str) -> Vec<String> {
        list_dir(&self.p(rel))
    }

    pub fn collect(&self) -> Inventory {
        let mut inv = Inventory {
            collected_utc: Some(UtcMillis::now()),
            platform: "linux".into(),
            provider: if self.fixture {
                "linux-sysfs (FIXTURE)".into()
            } else {
                "linux-sysfs".into()
            },
            ..Default::default()
        };
        inv.os = self.os_info();
        self.system_identity(&mut inv);
        inv.cpu = self.cpu(&mut inv);
        inv.memory = self.memory(&mut inv);
        inv.storage = self.storage(&mut inv);
        inv.pci = self.pci();
        if inv.pci.is_empty() {
            inv.note_unavailable(
                "PCI devices",
                "/sys/bus/pci/devices is empty or not readable",
            );
        }
        inv.gpus = self.gpus(&inv.pci);
        inv.network = self.network();
        inv.usb = self.usb();
        inv.firmware = self.firmware(&mut inv);
        inv.sensors = self.sensors();
        if !inv
            .sensors
            .iter()
            .any(|s| s.kind == SensorKind::Temperature)
        {
            inv.note_unavailable(
                "Temperature sensors",
                "No hwmon or thermal-zone temperature sensors exposed by the kernel (driver not loaded, virtual machine or unsupported platform)",
            );
        }
        if !inv.sensors.iter().any(|s| s.kind == SensorKind::Fan) {
            inv.note_unavailable(
                "Fan tachometers",
                "No hwmon fan inputs exposed by the kernel",
            );
        }
        if !inv.sensors.iter().any(|s| s.kind == SensorKind::Power) {
            inv.note_unavailable(
                "Power telemetry",
                "No hwmon power inputs exposed by the kernel",
            );
        }
        inv
    }

    fn os_info(&self) -> OsInfo {
        let mut os = OsInfo {
            arch: std::env::consts::ARCH.into(),
            ..Default::default()
        };
        if let Ok(s) = self.read("etc/os-release") {
            for line in s.lines() {
                if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
                    os.name = Some(unquote(v));
                } else if let Some(v) = line.strip_prefix("VERSION_ID=") {
                    os.version = Some(unquote(v));
                }
            }
        }
        os.kernel = self.read_opt("proc/sys/kernel/osrelease");
        os
    }

    fn system_identity(&self, inv: &mut Inventory) {
        let dmi = "sys/class/dmi/id";
        let mut id = SystemIdentity {
            hostname: self.read_opt("proc/sys/kernel/hostname"),
            ..Default::default()
        };
        if self.exists(dmi) {
            id.manufacturer = self.read_opt(&format!("{dmi}/sys_vendor"));
            id.product = self.read_opt(&format!("{dmi}/product_name"));
            id.board_vendor = self.read_opt(&format!("{dmi}/board_vendor"));
            id.board_name = self.read_opt(&format!("{dmi}/board_name"));
            id.chassis_type = self
                .read_opt(&format!("{dmi}/chassis_type"))
                .map(|c| chassis_name(&c));
            match self.read(&format!("{dmi}/product_serial")) {
                Ok(s) => id.serial = nonempty(s.trim()),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => inv.note_unavailable(
                    "System serial number",
                    "DMI product_serial requires root privileges",
                ),
                Err(_) => {
                    inv.note_unavailable("System serial number", "Not exposed by firmware/kernel")
                }
            }
        } else {
            inv.note_unavailable(
                "DMI/SMBIOS identity",
                "/sys/class/dmi/id not present (virtual machine or non-SMBIOS platform)",
            );
        }
        if let Some(t) = self.read_opt("sys/hypervisor/type") {
            id.virtualized = Some(t);
        } else if self
            .read("proc/cpuinfo")
            .map(|c| c.contains(" hypervisor"))
            .unwrap_or(false)
        {
            id.virtualized = Some("hypervisor CPU flag present".into());
        }
        inv.system = id;
    }

    fn cpu(&self, inv: &mut Inventory) -> Option<CpuInfo> {
        let text = match self.read("proc/cpuinfo") {
            Ok(t) => t,
            Err(e) => {
                inv.note_unavailable("CPU", format!("/proc/cpuinfo unreadable: {e}"));
                return None;
            }
        };
        let mut cpu = CpuInfo::default();
        let mut packages = BTreeSet::new();
        let mut cores = BTreeSet::new();
        let mut cur_pkg = String::new();
        let mut flags = String::new();
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "processor" => cpu.logical_cpus += 1,
                "vendor_id" if cpu.vendor.is_none() => cpu.vendor = Some(v.into()),
                "model name" if cpu.model.is_none() => cpu.model = Some(v.into()),
                "microcode" if cpu.microcode.is_none() => cpu.microcode = Some(v.into()),
                "physical id" => {
                    cur_pkg = v.into();
                    packages.insert(v.to_string());
                }
                "core id" => {
                    cores.insert((cur_pkg.clone(), v.to_string()));
                }
                "flags" if flags.is_empty() => flags = v.into(),
                // aarch64
                "CPU implementer" if cpu.vendor.is_none() => {
                    cpu.vendor = Some(format!("ARM implementer {v}"))
                }
                "Features" if flags.is_empty() => flags = v.into(),
                _ => {}
            }
        }
        if cpu.logical_cpus == 0 {
            inv.note_unavailable("CPU", "/proc/cpuinfo contained no processors");
            return None;
        }
        if !packages.is_empty() {
            cpu.packages = Some(packages.len() as u32);
        }
        if !cores.is_empty() {
            cpu.physical_cores = Some(cores.len() as u32);
        }
        const INTERESTING: &[&str] = &[
            "sse2",
            "sse4_1",
            "sse4_2",
            "avx",
            "avx2",
            "fma",
            "avx512f",
            "avx512bw",
            "avx512vl",
            "avx_vnni",
            "amx_tile",
            "aes",
            "sha_ni",
            "rdrand",
            "rdseed",
            "vmx",
            "svm",
            "hypervisor",
            "asimd",
            "sve",
            "sve2",
        ];
        let set: BTreeSet<&str> = flags.split_whitespace().collect();
        cpu.features = INTERESTING
            .iter()
            .filter(|f| set.contains(**f))
            .map(|s| s.to_string())
            .collect();

        for i in 0..cpu.logical_cpus {
            let khz = self
                .read_opt(&format!(
                    "sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"
                ))
                .and_then(|s| s.parse::<f64>().ok());
            cpu.current_mhz.push(khz.map(|k| k / 1000.0));
        }
        cpu.max_mhz = self
            .read_opt("sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|k| k / 1000.0);
        cpu.base_mhz = self
            .read_opt("sys/devices/system/cpu/cpu0/cpufreq/base_frequency")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|k| k / 1000.0);
        if cpu.current_mhz.iter().all(|m| m.is_none()) {
            inv.note_unavailable(
                "CPU clock telemetry",
                "cpufreq not exposed (virtual machine or driver not loaded)",
            );
        }
        for idx in self.list("sys/devices/system/cpu/cpu0/cache") {
            if !idx.starts_with("index") {
                continue;
            }
            let base = format!("sys/devices/system/cpu/cpu0/cache/{idx}");
            if let (Some(l), Some(t), Some(s)) = (
                self.read_opt(&format!("{base}/level")),
                self.read_opt(&format!("{base}/type")),
                self.read_opt(&format!("{base}/size")),
            ) {
                cpu.cache.push(format!("L{l} {t} {s}"));
            }
        }
        Some(cpu)
    }

    fn memory(&self, inv: &mut Inventory) -> Option<MemoryInfo> {
        let text = match self.read("proc/meminfo") {
            Ok(t) => t,
            Err(e) => {
                inv.note_unavailable("Memory", format!("/proc/meminfo unreadable: {e}"));
                return None;
            }
        };
        let kib = |key: &str| -> Option<u64> {
            text.lines()
                .find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
                .map(|v| v * 1024)
        };
        let mut mem = MemoryInfo {
            total_bytes: kib("MemTotal")?,
            available_bytes: kib("MemAvailable"),
            swap_total_bytes: kib("SwapTotal"),
            ecc_controllers: Vec::new(),
        };
        for mc in self.list("sys/devices/system/edac/mc") {
            if !mc.starts_with("mc") {
                continue;
            }
            let base = format!("sys/devices/system/edac/mc/{mc}");
            mem.ecc_controllers.push(EccController {
                name: self
                    .read_opt(&format!("{base}/mc_name"))
                    .unwrap_or(mc.clone()),
                corrected: self
                    .read_opt(&format!("{base}/ce_count"))
                    .and_then(|s| s.parse().ok()),
                uncorrected: self
                    .read_opt(&format!("{base}/ue_count"))
                    .and_then(|s| s.parse().ok()),
            });
        }
        if mem.ecc_controllers.is_empty() {
            inv.note_unavailable("ECC error counters", "No EDAC memory controllers registered (non-ECC memory, driver not loaded or virtual machine)");
        }
        Some(mem)
    }

    fn storage(&self, inv: &mut Inventory) -> Vec<StorageDevice> {
        let mut out = Vec::new();
        for name in self.list("sys/block") {
            if name.starts_with("loop")
                || name.starts_with("ram")
                || name.starts_with("zram")
                || name.starts_with("dm-")
                || name.starts_with("md")
            {
                continue;
            }
            let base = format!("sys/block/{name}");
            let mut d = StorageDevice {
                name: name.clone(),
                ..Default::default()
            };
            d.size_bytes = self
                .read_opt(&format!("{base}/size"))
                .and_then(|s| s.parse::<u64>().ok())
                .map(|s| s * 512);
            d.rotational = self
                .read_opt(&format!("{base}/queue/rotational"))
                .map(|s| s == "1");
            d.removable = self
                .read_opt(&format!("{base}/removable"))
                .map(|s| s == "1");
            d.state = self.read_opt(&format!("{base}/device/state"));
            if name.starts_with("nvme") {
                d.transport = Some("NVMe".into());
                // device -> controller directory (nvmeX)
                d.model = self.read_opt(&format!("{base}/device/model"));
                d.serial = self.read_opt(&format!("{base}/device/serial"));
                d.firmware = self.read_opt(&format!("{base}/device/firmware_rev"));
            } else {
                d.transport = Some(
                    if name.starts_with("vd") {
                        "virtio"
                    } else if name.starts_with("sd") {
                        "SCSI/SATA/USB"
                    } else if name.starts_with("mmcblk") {
                        "MMC"
                    } else {
                        "other"
                    }
                    .into(),
                );
                d.model = self.read_opt(&format!("{base}/device/model"));
                d.firmware = self.read_opt(&format!("{base}/device/rev"));
                d.serial = self.read_opt(&format!("{base}/device/serial"));
            }
            out.push(d);
        }
        if out.is_empty() {
            inv.note_unavailable("Storage devices", "No block devices found in /sys/block");
        }
        out
    }

    fn pci(&self) -> Vec<PciDevice> {
        let mut out = Vec::new();
        for addr in self.list("sys/bus/pci/devices") {
            let base = format!("sys/bus/pci/devices/{addr}");
            let hex = |f: &str| {
                self.read_opt(&format!("{base}/{f}"))
                    .and_then(|s| parse_hex(&s))
            };
            let class = hex("class").map(|c| c as u32);
            let aer = |f: &str, key: &str| -> Option<u64> {
                let t = self.read(&format!("{base}/{f}")).ok()?;
                t.lines()
                    .find(|l| l.starts_with(key))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse().ok())
            };
            out.push(PciDevice {
                address: addr.clone(),
                vendor_id: hex("vendor").map(|v| v as u16),
                device_id: hex("device").map(|v| v as u16),
                class_code: class,
                class_name: class
                    .map(pci_ids::class_name)
                    .unwrap_or("Unknown")
                    .to_string(),
                driver: link_name(&self.p(&format!("{base}/driver"))),
                current_link_speed: self
                    .read_opt(&format!("{base}/current_link_speed"))
                    .filter(|s| !s.contains("Unknown")),
                current_link_width: self
                    .read_opt(&format!("{base}/current_link_width"))
                    .and_then(|s| s.parse().ok())
                    .filter(|w| *w > 0),
                max_link_speed: self
                    .read_opt(&format!("{base}/max_link_speed"))
                    .filter(|s| !s.contains("Unknown")),
                max_link_width: self
                    .read_opt(&format!("{base}/max_link_width"))
                    .and_then(|s| s.parse().ok())
                    .filter(|w| *w > 0),
                aer_correctable: aer("aer_dev_correctable", "TOTAL_ERR_COR"),
                aer_nonfatal: aer("aer_dev_nonfatal", "TOTAL_ERR_NONFATAL"),
                aer_fatal: aer("aer_dev_fatal", "TOTAL_ERR_FATAL"),
            });
        }
        out.sort_by(|a, b| a.address.cmp(&b.address));
        out
    }

    fn gpus(&self, pci: &[PciDevice]) -> Vec<GpuDevice> {
        let mut out = Vec::new();
        for dev in pci
            .iter()
            .filter(|d| d.class_code.map(pci_ids::is_display_class).unwrap_or(false))
        {
            let base = format!("sys/bus/pci/devices/{}", dev.address);
            let vendor = dev.vendor_id.map(|v| {
                pci_ids::vendor_name(v)
                    .map(String::from)
                    .unwrap_or(format!("0x{v:04x}"))
            });
            let mut g = GpuDevice {
                id: dev.address.clone(),
                vendor,
                name: dev
                    .device_id
                    .map(|d| format!("{} device 0x{d:04x}", dev.class_name)),
                driver: dev.driver.clone(),
                pci_address: Some(dev.address.clone()),
                ..Default::default()
            };
            // amdgpu exposes VRAM accounting in sysfs.
            if let Some(t) = self
                .read_opt(&format!("{base}/mem_info_vram_total"))
                .and_then(|s| s.parse().ok())
            {
                g.vram_total_bytes = Some(t);
                g.vram_used_bytes = self
                    .read_opt(&format!("{base}/mem_info_vram_used"))
                    .and_then(|s| s.parse().ok());
                g.telemetry_source = Some("amdgpu sysfs".into());
            }
            out.push(g);
        }
        out
    }

    fn network(&self) -> Vec<NetworkInterface> {
        let mut out = Vec::new();
        for name in self.list("sys/class/net") {
            let base = format!("sys/class/net/{name}");
            let is_virtual = !self.exists(&format!("{base}/device"));
            out.push(NetworkInterface {
                name: name.clone(),
                mac: self.read_opt(&format!("{base}/address")),
                driver: link_name(&self.p(&format!("{base}/device/driver"))),
                oper_state: self.read_opt(&format!("{base}/operstate")),
                // `speed` returns EINVAL (or -1) when the link is down: unknown, not zero.
                speed_mbps: self
                    .read_opt(&format!("{base}/speed"))
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|s| *s > 0),
                rx_errors: self
                    .read_opt(&format!("{base}/statistics/rx_errors"))
                    .and_then(|s| s.parse().ok()),
                tx_errors: self
                    .read_opt(&format!("{base}/statistics/tx_errors"))
                    .and_then(|s| s.parse().ok()),
                is_virtual,
            });
        }
        out
    }

    fn usb(&self) -> Vec<UsbDevice> {
        let mut out = Vec::new();
        for d in self.list("sys/bus/usb/devices") {
            let base = format!("sys/bus/usb/devices/{d}");
            let Some(vid) = self.read_opt(&format!("{base}/idVendor")) else {
                continue;
            };
            out.push(UsbDevice {
                path: d.clone(),
                vendor_id: Some(vid),
                product_id: self.read_opt(&format!("{base}/idProduct")),
                manufacturer: self.read_opt(&format!("{base}/manufacturer")),
                product: self.read_opt(&format!("{base}/product")),
                speed: self
                    .read_opt(&format!("{base}/speed"))
                    .map(|s| format!("{s} Mb/s")),
            });
        }
        out
    }

    fn firmware(&self, inv: &mut Inventory) -> FirmwareInfo {
        let dmi = "sys/class/dmi/id";
        let mut fw = FirmwareInfo {
            bios_vendor: self.read_opt(&format!("{dmi}/bios_vendor")),
            bios_version: self.read_opt(&format!("{dmi}/bios_version")),
            bios_date: self.read_opt(&format!("{dmi}/bios_date")),
            ..Default::default()
        };
        let efi = self.exists("sys/firmware/efi");
        fw.uefi = Some(efi);
        if efi {
            let sb =
                self.p("sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c");
            match fs::read(&sb) {
                // 4-byte attribute header followed by a 1-byte value.
                Ok(b) if b.len() >= 5 => fw.secure_boot = Some(b[4] == 1),
                Ok(_) => {
                    inv.note_unavailable("Secure Boot state", "SecureBoot EFI variable malformed")
                }
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => inv.note_unavailable(
                    "Secure Boot state",
                    "efivars not readable with current privileges",
                ),
                Err(_) => {
                    inv.note_unavailable("Secure Boot state", "SecureBoot EFI variable not present")
                }
            }
        } else {
            inv.note_unavailable(
                "Secure Boot state",
                "System not booted via UEFI (or /sys/firmware/efi unavailable)",
            );
        }
        let tpms: Vec<_> = self
            .list("sys/class/tpm")
            .into_iter()
            .filter(|t| t.starts_with("tpm"))
            .collect();
        fw.tpm_present = Some(!tpms.is_empty());
        if let Some(t) = tpms.first() {
            fw.tpm_version = self
                .read_opt(&format!("sys/class/tpm/{t}/tpm_version_major"))
                .map(|v| format!("{v}.x"));
        }
        fw
    }

    pub fn sensors(&self) -> Vec<SensorReading> {
        let now = UtcMillis::now();
        let mut out = Vec::new();
        for hw in self.list("sys/class/hwmon") {
            let base = format!("sys/class/hwmon/{hw}");
            let chip = self.read_opt(&format!("{base}/name")).unwrap_or(hw.clone());
            let mut files = self.list(&base);
            // Some drivers place attributes under device/.
            let mut dir = base.clone();
            if !files.iter().any(|f| f.ends_with("_input")) {
                dir = format!("{base}/device");
                files = self.list(&dir);
            }
            for f in files {
                let (prefix, kind, scale) = if let Some(p) = f.strip_suffix("_input") {
                    match sensor_kind(p) {
                        Some(k) => (p.to_string(), k.0, k.1),
                        None => continue,
                    }
                } else if let Some(p) = f
                    .strip_suffix("_average")
                    .filter(|p| p.starts_with("power"))
                {
                    if files_contains(&self.list(&dir), &format!("{p}_input")) {
                        continue;
                    }
                    (p.to_string(), SensorKind::Power, 1e-6)
                } else {
                    continue;
                };
                let raw = self
                    .read_opt(&format!("{dir}/{f}"))
                    .and_then(|s| s.parse::<f64>().ok());
                let label = self
                    .read_opt(&format!("{dir}/{prefix}_label"))
                    .unwrap_or(prefix.clone());
                let lim = |s: &str| {
                    self.read_opt(&format!("{dir}/{prefix}_{s}"))
                        .and_then(|v| v.parse::<f64>().ok())
                        .map(|v| v * scale)
                };
                out.push(SensorReading {
                    id: format!("hwmon:{chip}:{hw}:{prefix}"),
                    chip: chip.clone(),
                    label,
                    kind,
                    value: raw.map(|v| v * scale),
                    unit: kind.unit().into(),
                    crit: lim("crit"),
                    max: lim("max"),
                    ts: now,
                    source: "hwmon".into(),
                });
            }
        }
        for tz in self.list("sys/class/thermal") {
            if !tz.starts_with("thermal_zone") {
                continue;
            }
            let base = format!("sys/class/thermal/{tz}");
            let t = self.read_opt(&format!("{base}/type")).unwrap_or(tz.clone());
            out.push(SensorReading {
                id: format!("thermal:{tz}"),
                chip: "thermal".into(),
                label: t,
                kind: SensorKind::Temperature,
                value: self
                    .read_opt(&format!("{base}/temp"))
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v / 1000.0),
                unit: "°C".into(),
                crit: None,
                max: None,
                ts: now,
                source: "thermal_zone".into(),
            });
        }
        let ncpu = self
            .list("sys/devices/system/cpu")
            .iter()
            .filter(|c| c.starts_with("cpu") && c[3..].parse::<u32>().is_ok())
            .count();
        for i in 0..ncpu {
            if let Some(khz) = self
                .read_opt(&format!(
                    "sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"
                ))
                .and_then(|s| s.parse::<f64>().ok())
            {
                out.push(SensorReading {
                    id: format!("cpufreq:cpu{i}"),
                    chip: "cpufreq".into(),
                    label: format!("CPU{i} clock"),
                    kind: SensorKind::Frequency,
                    value: Some(khz / 1000.0),
                    unit: "MHz".into(),
                    crit: None,
                    max: None,
                    ts: now,
                    source: "cpufreq".into(),
                });
            }
        }
        if !self.fixture {
            out.extend(crate::gpu::nvml_sensors());
        }
        out
    }
}

impl crate::PlatformProvider for LinuxProvider {
    fn name(&self) -> &'static str {
        if self.fixture {
            "linux-sysfs (FIXTURE)"
        } else {
            "linux-sysfs"
        }
    }
    fn inventory(&self) -> Inventory {
        let mut inv = self.collect();
        if !self.fixture {
            crate::gpu::merge_nvml(&mut inv);
        }
        inv
    }
    fn sample_sensors(&self) -> Vec<SensorReading> {
        self.sensors()
    }
}

fn files_contains(files: &[String], name: &str) -> bool {
    files.iter().any(|f| f == name)
}

fn sensor_kind(prefix: &str) -> Option<(SensorKind, f64)> {
    let stem: String = prefix
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if prefix.len() == stem.len() {
        return None; // need an index, e.g. temp1
    }
    Some(match stem.as_str() {
        "temp" => (SensorKind::Temperature, 1e-3),
        "fan" => (SensorKind::Fan, 1.0),
        "in" => (SensorKind::Voltage, 1e-3),
        "power" => (SensorKind::Power, 1e-6),
        "curr" => (SensorKind::Current, 1e-3),
        "freq" => (SensorKind::Frequency, 1e-6),
        _ => return None,
    })
}

pub(crate) fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().and_then(|s| nonempty(s.trim()))
}

fn nonempty(s: &str) -> Option<String> {
    let s = s.trim_matches(char::from(0)).trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn list_dir(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| natural_cmp(a, b));
    v
}

/// Natural ordering so that hwmon10 sorts after hwmon9.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let i = s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        (s[..i].to_string(), s[i..].parse::<u64>().unwrap_or(0))
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

fn link_name(p: &Path) -> Option<String> {
    fs::read_link(p)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
}

fn parse_hex(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

fn chassis_name(code: &str) -> String {
    let name = match code.trim().parse::<u32>().unwrap_or(0) {
        3 => "Desktop",
        4 => "Low Profile Desktop",
        6 => "Mini Tower",
        7 => "Tower",
        8 => "Portable",
        9 => "Laptop",
        10 => "Notebook",
        13 => "All in One",
        17 => "Main Server Chassis",
        23 => "Rack Mount Chassis",
        35 => "Mini PC",
        _ => return format!("type {code}"),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    //! FIXTURE TESTS: these build a fake sysfs/procfs tree. They validate
    //! parsing logic only and are not evidence of physical hardware support.
    use super::*;
    use crate::PlatformProvider;

    fn w(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    pub(crate) fn workstation_fixture(root: &Path) {
        w(
            root,
            "etc/os-release",
            "NAME=\"Fixture Linux\"\nPRETTY_NAME=\"Fixture Linux 1.0\"\nVERSION_ID=\"1.0\"\n",
        );
        w(root, "proc/sys/kernel/osrelease", "6.8.0-fixture\n");
        w(root, "proc/sys/kernel/hostname", "be01-fixture\n");
        let mut cpuinfo = String::new();
        for i in 0..4 {
            cpuinfo.push_str(&format!(
                "processor\t: {i}\nvendor_id\t: FixtureVendor\nmodel name\t: Fixture CPU 9000\nphysical id\t: 0\ncore id\t\t: {}\nmicrocode\t: 0x2b\nflags\t\t: fpu sse2 sse4_2 avx avx2 fma aes\n\n",
                i / 2
            ));
        }
        w(root, "proc/cpuinfo", &cpuinfo);
        w(
            root,
            "proc/meminfo",
            "MemTotal:       65536000 kB\nMemFree:  1000 kB\nMemAvailable:   60000000 kB\nSwapTotal:  0 kB\n",
        );
        for i in 0..4 {
            w(
                root,
                &format!("sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq"),
                "3500000\n",
            );
        }
        w(
            root,
            "sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq",
            "5000000\n",
        );
        w(root, "sys/devices/system/cpu/cpu0/cache/index0/level", "1");
        w(
            root,
            "sys/devices/system/cpu/cpu0/cache/index0/type",
            "Data",
        );
        w(root, "sys/devices/system/cpu/cpu0/cache/index0/size", "48K");
        w(root, "sys/devices/system/edac/mc/mc0/ce_count", "2\n");
        w(root, "sys/devices/system/edac/mc/mc0/ue_count", "0\n");
        w(root, "sys/class/dmi/id/sys_vendor", "Ballin Electric\n");
        w(root, "sys/class/dmi/id/product_name", "BE-01 (fixture)\n");
        w(root, "sys/class/dmi/id/product_serial", "FIXTURE-0001\n");
        w(root, "sys/class/dmi/id/chassis_type", "8\n");
        w(root, "sys/class/dmi/id/bios_version", "0.9.1\n");
        w(root, "sys/block/nvme0n1/size", "2000409264\n");
        w(root, "sys/block/nvme0n1/queue/rotational", "0\n");
        w(root, "sys/block/nvme0n1/device/model", "Fixture NVMe 2TB\n");
        w(root, "sys/block/nvme0n1/device/serial", "S0FIXTURE\n");
        w(root, "sys/block/nvme0n1/device/firmware_rev", "1B2QGXA7\n");
        w(root, "sys/block/loop0/size", "0\n");
        let gpu = "sys/bus/pci/devices/0000:01:00.0";
        w(root, &format!("{gpu}/vendor"), "0x1002\n");
        w(root, &format!("{gpu}/device"), "0x7551\n");
        w(root, &format!("{gpu}/class"), "0x030000\n");
        w(
            root,
            &format!("{gpu}/current_link_speed"),
            "8.0 GT/s PCIe\n",
        );
        w(root, &format!("{gpu}/current_link_width"), "8\n");
        w(root, &format!("{gpu}/max_link_speed"), "32.0 GT/s PCIe\n");
        w(root, &format!("{gpu}/max_link_width"), "16\n");
        w(root, &format!("{gpu}/mem_info_vram_total"), "34359738368\n");
        w(root, &format!("{gpu}/mem_info_vram_used"), "1073741824\n");
        w(
            root,
            &format!("{gpu}/aer_dev_correctable"),
            "RxErr 3\nBadTLP 0\nTOTAL_ERR_COR 3\n",
        );
        let nv = "sys/bus/pci/devices/0000:02:00.0";
        w(root, &format!("{nv}/vendor"), "0x144d\n");
        w(root, &format!("{nv}/device"), "0xa80c\n");
        w(root, &format!("{nv}/class"), "0x010802\n");
        w(root, "sys/class/hwmon/hwmon0/name", "k10temp\n");
        w(root, "sys/class/hwmon/hwmon0/temp1_input", "45250\n");
        w(root, "sys/class/hwmon/hwmon0/temp1_label", "Tctl\n");
        w(root, "sys/class/hwmon/hwmon0/temp1_crit", "95000\n");
        w(root, "sys/class/hwmon/hwmon1/name", "nct6798\n");
        w(root, "sys/class/hwmon/hwmon1/fan1_input", "1180\n");
        w(root, "sys/class/hwmon/hwmon1/in0_input", "1032\n");
        w(root, "sys/class/hwmon/hwmon1/power1_average", "65000000\n");
        w(root, "sys/class/hwmon/hwmon1/temp2_input", "garbage\n");
        w(root, "sys/class/net/eth0/address", "00:11:22:33:44:55\n");
        w(root, "sys/class/net/eth0/operstate", "up\n");
        w(root, "sys/class/net/eth0/speed", "10000\n");
        w(root, "sys/class/net/eth0/statistics/rx_errors", "0\n");
        w(root, "sys/class/net/eth0/statistics/tx_errors", "0\n");
        w(root, "sys/class/net/eth0/device/vendor", "0x8086\n");
        w(root, "sys/class/net/lo/operstate", "unknown\n");
        w(root, "sys/class/net/lo/speed", "-1\n");
        w(
            root,
            "sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c",
            "\x06\0\0\0\x01",
        );
        w(root, "sys/class/tpm/tpm0/tpm_version_major", "2\n");
        w(root, "sys/bus/usb/devices/1-1/idVendor", "046d\n");
        w(root, "sys/bus/usb/devices/1-1/idProduct", "c52b\n");
        w(root, "sys/bus/usb/devices/1-1/speed", "12\n");
    }

    #[test]
    fn fixture_workstation_inventory() {
        let dir = tempfile::tempdir().unwrap();
        workstation_fixture(dir.path());
        let p = LinuxProvider::with_fixture_root(dir.path());
        let inv = p.inventory();
        assert!(inv.provider.contains("FIXTURE"));
        assert_eq!(inv.os.name.as_deref(), Some("Fixture Linux 1.0"));
        let cpu = inv.cpu.as_ref().unwrap();
        assert_eq!(cpu.logical_cpus, 4);
        assert_eq!(cpu.physical_cores, Some(2));
        assert_eq!(cpu.packages, Some(1));
        assert_eq!(cpu.max_mhz, Some(5000.0));
        assert!(cpu.features.contains(&"avx2".to_string()));
        assert!(!cpu.features.contains(&"avx512f".to_string()));
        assert_eq!(cpu.cache, vec!["L1 Data 48K".to_string()]);
        let mem = inv.memory.as_ref().unwrap();
        assert_eq!(mem.total_bytes, 65_536_000 * 1024);
        assert_eq!(mem.ecc_controllers[0].corrected, Some(2));
        assert_eq!(inv.system.serial.as_deref(), Some("FIXTURE-0001"));
        assert_eq!(inv.system.chassis_type.as_deref(), Some("Portable"));
        assert_eq!(inv.storage.len(), 1, "loop devices are excluded");
        assert_eq!(inv.storage[0].size_bytes, Some(2_000_409_264 * 512));
        assert_eq!(inv.storage[0].serial.as_deref(), Some("S0FIXTURE"));
        assert_eq!(inv.pci.len(), 2);
        let g = &inv.pci[0];
        assert_eq!(g.current_link_width, Some(8));
        assert_eq!(g.max_link_speed.as_deref(), Some("32.0 GT/s PCIe"));
        assert_eq!(g.aer_correctable, Some(3));
        assert_eq!(inv.gpus.len(), 1);
        assert_eq!(inv.gpus[0].vram_total_bytes, Some(34_359_738_368));
        assert_eq!(inv.gpus[0].vendor.as_deref(), Some("AMD/ATI"));
        assert_eq!(inv.firmware.secure_boot, Some(true));
        assert_eq!(inv.firmware.tpm_version.as_deref(), Some("2.x"));
        let eth = inv.network.iter().find(|n| n.name == "eth0").unwrap();
        assert_eq!(eth.speed_mbps, Some(10000));
        assert!(!eth.is_virtual);
        let lo = inv.network.iter().find(|n| n.name == "lo").unwrap();
        assert_eq!(lo.speed_mbps, None, "-1 speed is unknown, not a value");
        assert!(lo.is_virtual);
        assert_eq!(inv.usb.len(), 1);
    }

    #[test]
    fn fixture_sensor_parsing_and_units() {
        let dir = tempfile::tempdir().unwrap();
        workstation_fixture(dir.path());
        let s = LinuxProvider::with_fixture_root(dir.path()).sensors();
        let t = s.iter().find(|r| r.label == "Tctl").unwrap();
        assert_eq!(t.value, Some(45.25));
        assert_eq!(t.crit, Some(95.0));
        let f = s.iter().find(|r| r.kind == SensorKind::Fan).unwrap();
        assert_eq!(f.value, Some(1180.0));
        let v = s.iter().find(|r| r.kind == SensorKind::Voltage).unwrap();
        assert!((v.value.unwrap() - 1.032).abs() < 1e-9);
        let p = s.iter().find(|r| r.kind == SensorKind::Power).unwrap();
        assert!((p.value.unwrap() - 65.0).abs() < 1e-9);
        let bad = s.iter().find(|r| r.id.ends_with(":temp2")).unwrap();
        assert_eq!(
            bad.value, None,
            "unparseable reading must be unknown, never zero"
        );
        assert_eq!(
            s.iter().filter(|r| r.kind == SensorKind::Frequency).count(),
            4
        );
    }

    #[test]
    fn empty_root_reports_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let inv = LinuxProvider::with_fixture_root(dir.path()).inventory();
        assert!(inv.cpu.is_none());
        assert!(inv.memory.is_none());
        assert!(inv.sensors.is_empty());
        let items: Vec<_> = inv.unavailable.iter().map(|u| u.item.as_str()).collect();
        for needed in [
            "CPU",
            "Memory",
            "Temperature sensors",
            "Storage devices",
            "DMI/SMBIOS identity",
        ] {
            assert!(
                items.contains(&needed),
                "missing unavailable note for {needed}: {items:?}"
            );
        }
    }

    #[test]
    fn natural_sort() {
        let mut v = vec!["hwmon10".to_string(), "hwmon9".into(), "hwmon1".into()];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["hwmon1", "hwmon9", "hwmon10"]);
    }
}
