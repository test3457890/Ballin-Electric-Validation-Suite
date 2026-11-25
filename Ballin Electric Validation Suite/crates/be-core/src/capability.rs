//! Capability registry: what this build can actually do on this machine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum CapabilityState {
    /// Implemented and its backend is present on this machine.
    Available,
    /// Implemented but some data is missing (reason explains).
    Degraded(String),
    /// Implemented, but this machine lacks the hardware/driver/OS support.
    Unsupported(String),
    /// Implemented but needs elevated privileges for this machine.
    RequiresPrivilege(String),
    /// Implemented but blocked by policy (authorization, safety profile, license).
    Blocked(String),
    /// Not implemented in this build.
    Planned(String),
}

impl CapabilityState {
    pub fn label(&self) -> &'static str {
        match self {
            CapabilityState::Available => "Available",
            CapabilityState::Degraded(_) => "Degraded",
            CapabilityState::Unsupported(_) => "Unsupported",
            CapabilityState::RequiresPrivilege(_) => "Requires privilege",
            CapabilityState::Blocked(_) => "Blocked",
            CapabilityState::Planned(_) => "Planned",
        }
    }
    pub fn reason(&self) -> &str {
        match self {
            CapabilityState::Available => "",
            CapabilityState::Degraded(r)
            | CapabilityState::Unsupported(r)
            | CapabilityState::RequiresPrivilege(r)
            | CapabilityState::Blocked(r)
            | CapabilityState::Planned(r) => r,
        }
    }
    pub fn is_usable(&self) -> bool {
        matches!(
            self,
            CapabilityState::Available | CapabilityState::Degraded(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub area: String,
    pub backend: String,
    #[serde(flatten)]
    pub state: CapabilityState,
}

impl Capability {
    pub fn new(id: &str, name: &str, area: &str, backend: &str, state: CapabilityState) -> Self {
        Capability {
            id: id.into(),
            name: name.into(),
            area: area.into(),
            backend: backend.into(),
            state,
        }
    }
}
