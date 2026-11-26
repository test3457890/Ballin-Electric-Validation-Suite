//! Safety layer: authorization and hardware safety profiles, preflight
//! checks, the sensor watchdog, and the isolated-worker supervisor.

pub mod policy;
pub mod supervisor;
pub mod watchdog;
#[cfg(windows)]
pub mod winjob;

pub use policy::{Authorization, HardwareSafetyProfile, SensorRule};
pub use supervisor::{SupervisorConfig, SupervisorEvent, run_supervised};
pub use watchdog::{SafetyMonitor, SafetyVerdict, SensorWatchdog};
