//! Platform providers. Each provider returns only data actually read from
//! the operating system; anything unavailable is recorded with a reason.

use be_core::inventory::{Inventory, SensorReading};

pub mod gpu;
pub mod linux;
pub mod pci_ids;
pub mod windows_cim;

/// A source of hardware inventory and live sensor readings.
pub trait PlatformProvider: Send + Sync {
    fn name(&self) -> &'static str;
    /// Full read-only inventory. May take up to a few seconds.
    fn inventory(&self) -> Inventory;
    /// Fast sensor sample for monitoring and safety supervision.
    fn sample_sensors(&self) -> Vec<SensorReading>;
}

/// The provider for the running operating system.
pub fn native() -> Box<dyn PlatformProvider> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxProvider::system())
    }
    #[cfg(windows)]
    {
        Box::new(windows_cim::WindowsProvider::new())
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Box::new(UnsupportedProvider)
    }
}

/// Provider used on platforms with no implemented backend.
pub struct UnsupportedProvider;

impl PlatformProvider for UnsupportedProvider {
    fn name(&self) -> &'static str {
        "unsupported"
    }
    fn inventory(&self) -> Inventory {
        let mut inv = Inventory {
            platform: std::env::consts::OS.to_string(),
            provider: self.name().into(),
            ..Default::default()
        };
        inv.os.arch = std::env::consts::ARCH.to_string();
        inv.note_unavailable(
            "All inventory",
            "No platform provider is implemented for this operating system",
        );
        inv
    }
    fn sample_sensors(&self) -> Vec<SensorReading> {
        Vec::new()
    }
}
