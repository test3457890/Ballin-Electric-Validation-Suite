//! Minimal PCI vendor and class naming. Only well-known public IDs; unknown
//! values are shown numerically rather than guessed.

pub fn vendor_name(id: u16) -> Option<&'static str> {
    Some(match id {
        0x8086 => "Intel",
        0x1022 => "AMD",
        0x1002 => "AMD/ATI",
        0x10de => "NVIDIA",
        0x144d => "Samsung",
        0x15b7 => "Sandisk/WD",
        0x1c5c => "SK hynix",
        0x1987 => "Phison",
        0x126f => "Silicon Motion",
        0x1b4b => "Marvell",
        0x10ec => "Realtek",
        0x14e4 => "Broadcom",
        0x15b3 => "Mellanox/NVIDIA",
        0x1d6a => "Aquantia/Marvell",
        0x1af4 => "Red Hat (virtio)",
        0x1b36 => "Red Hat (QEMU)",
        0x1234 => "QEMU",
        0x15ad => "VMware",
        0x1414 => "Microsoft (Hyper-V)",
        0x1b21 => "ASMedia",
        0x1106 => "VIA",
        0x168c => "Qualcomm Atheros",
        0x17cb => "Qualcomm",
        _ => return None,
    })
}

pub fn class_name(class: u32) -> &'static str {
    let base = (class >> 16) & 0xff;
    let sub = (class >> 8) & 0xff;
    match (base, sub) {
        (0x01, 0x08) => "NVMe controller",
        (0x01, 0x06) => "SATA controller",
        (0x01, _) => "Storage controller",
        (0x02, _) => "Network controller",
        (0x03, 0x00) => "VGA compatible controller",
        (0x03, 0x02) => "3D controller",
        (0x03, _) => "Display controller",
        (0x04, 0x03) => "Audio device",
        (0x04, _) => "Multimedia controller",
        (0x05, _) => "Memory controller",
        (0x06, 0x00) => "Host bridge",
        (0x06, 0x01) => "ISA bridge",
        (0x06, 0x04) => "PCI bridge",
        (0x06, _) => "Bridge",
        (0x07, _) => "Communication controller",
        (0x08, _) => "System peripheral",
        (0x0c, 0x03) => "USB controller",
        (0x0c, 0x05) => "SMBus controller",
        (0x0c, _) => "Serial bus controller",
        (0x0d, _) => "Wireless controller",
        (0x10, _) => "Encryption controller",
        (0x11, _) => "Signal processing controller",
        (0x12, _) => "Processing accelerator",
        (0x13, _) => "Non-essential instrumentation",
        (0xff, _) => "Unassigned class",
        _ => "Other device",
    }
}

pub fn is_display_class(class: u32) -> bool {
    (class >> 16) & 0xff == 0x03
}
