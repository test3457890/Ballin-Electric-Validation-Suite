//! Core contracts for Ballin Electric System Validation Suite (BE Validation).
//!
//! This crate contains only platform-independent data models and pure logic:
//! test status semantics, typed results, the versioned fault-code catalog,
//! test profiles, the release (shipment disposition) policy and the worker
//! IPC protocol. It performs no I/O against hardware.

pub mod capability;
pub mod codes;
pub mod exit;
pub mod fsutil;
pub mod inventory;
pub mod ipc;
pub mod isolation;
pub mod model;
pub mod policy;
pub mod profile;
pub mod status;
pub mod time;

/// Version of the diagnostic engine. Stored with every run and result.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Version of the persisted/exported result schema.
pub const RESULT_SCHEMA_VERSION: u32 = 1;

pub const PRODUCT_NAME: &str = "Ballin Electric System Validation Suite";
pub const PRODUCT_SHORT_NAME: &str = "BE Validation";
pub const PRODUCT_SUBTITLE: &str = "Hardware Diagnostics & Manufacturing Quality Assurance";
