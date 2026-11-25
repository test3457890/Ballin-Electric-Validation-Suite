//! GPU tests. Inventory/telemetry is implemented. Compute, VRAM and graphics
//! workloads require a GPU backend (Vulkan compute via `ash`, CUDA, HIP) that
//! is not yet implemented; those tests honestly report UNSUPPORTED.

use crate::{DiagnosticTest, TestContext, desc, finish};
use be_core::inventory::{Inventory, SensorKind, format_bytes};
use be_core::model::*;
use be_core::status::TestStatus;
use serde_json::json;
use std::time::Instant;

pub struct GpuInventory;

pub fn evaluate_gpus(
    inv: &Inventory,
    expected_count: Option<u64>,
    r: &mut TestResult,
) -> TestStatus {
    r.measurements
        .push(Measurement::num("GPUs detected", inv.gpus.len() as f64, ""));
    if inv.gpus.is_empty() {
        if expected_count.unwrap_or(0) > 0 {
            r.faults.push(FaultRecord::new(
                "BE-CFG-001",
                "Expected GPU not detected",
                format!("manifest requires {}", expected_count.unwrap()),
            ));
            return TestStatus::Fail;
        }
        r.notes
            .push("No display/3D controllers found on the PCI bus".into());
        return TestStatus::Unsupported;
    }
    let mut status = TestStatus::Pass;
    for (i, g) in inv.gpus.iter().enumerate() {
        let name = g.name.clone().unwrap_or("unknown".into());
        r.measurements.push(Measurement::text(
            format!("GPU {i} ({})", g.id),
            format!("{} {}", g.vendor.clone().unwrap_or_default(), name),
        ));
        match g.vram_total_bytes {
            Some(v) => r
                .measurements
                .push(Measurement::text(format!("GPU {i} VRAM"), format_bytes(v))),
            None => r
                .measurements
                .push(Measurement::unavailable(format!("GPU {i} VRAM"), "bytes")),
        }
        let has_temp = inv.sensors.iter().any(|s| {
            s.kind == SensorKind::Temperature
                && (s.chip == format!("GPU {i}") || s.chip.contains("amdgpu"))
        });
        if g.telemetry_source.is_none() || !has_temp {
            r.faults.push(
                FaultRecord::new(
                    "BE-GPU-020",
                    "GPU telemetry incomplete",
                    format!(
                        "GPU {i} ({}): {}",
                        g.id,
                        if g.telemetry_source.is_none() {
                            "no supported telemetry interface"
                        } else {
                            "no temperature sensor"
                        }
                    ),
                )
                .on(g.id.clone()),
            );
            status = TestStatus::Warning;
        }
    }
    if let Some(n) = expected_count {
        if inv.gpus.len() as u64 != n {
            r.faults.push(FaultRecord::new(
                "BE-CFG-001",
                "GPU count mismatch",
                format!("{} detected, {n} expected", inv.gpus.len()),
            ));
            status = TestStatus::Fail;
        }
    }
    r.coverage = "PCI display-class enumeration plus NVML/amdgpu telemetry where available".into();
    status
}

impl DiagnosticTest for GpuInventory {
    fn descriptor(&self) -> TestDescriptor {
        desc(
            "BE-T-GPU-001",
            "1.0.0",
            "GPU Inventory & Telemetry",
            Subsystem::Gpu,
            LoadClass::ReadOnly,
            Privilege::Standard,
            2,
            "Enumerates GPUs by PCI address and checks that required telemetry (temperature, VRAM accounting) is exposed through NVML (NVIDIA) or amdgpu sysfs (AMD).",
            "GPU identities, VRAM size, telemetry availability",
            "Consumer and workstation cards expose different metrics. Missing telemetry blocks high-load GPU profiles.",
            json!({"expected_count": null}),
        )
    }
    fn run(&self, ctx: &TestContext) -> TestResult {
        let started = Instant::now();
        let mut r = TestResult::new(&self.descriptor(), "Graphics devices", ctx.provenance);
        let inv = ctx.platform.inventory();
        let st = evaluate_gpus(
            &inv,
            ctx.params.get("expected_count").and_then(|v| v.as_u64()),
            &mut r,
        );
        finish(r, started, st)
    }
}

fn unsupported(d: TestDescriptor, ctx: &TestContext, what: &str) -> TestResult {
    let mut r = TestResult::not_executed(
        &d.id,
        &d.version,
        TestStatus::Unsupported,
        format!(
            "{what}: no GPU compute/graphics backend is implemented in this build (planned: Vulkan compute via ash, optional CUDA/HIP adapters; Milestone 3)"
        ),
        ctx.provenance,
    );
    r.faults.push(FaultRecord::new(
        "BE-GPU-001",
        "GPU backend unavailable",
        "Backend not implemented",
    ));
    r
}

macro_rules! planned_gpu_test {
    ($ty:ident, $id:expr, $name:expr, $sub:ident, $load:ident, $descr:expr) => {
        pub struct $ty;
        impl DiagnosticTest for $ty {
            fn descriptor(&self) -> TestDescriptor {
                desc($id, "0.1.0", $name, Subsystem::$sub, LoadClass::$load, Privilege::Standard, 600, $descr,
                    "Planned: per-device mismatch counts, coverage of allocated VRAM, telemetry",
                    "NOT IMPLEMENTED: always reports UNSUPPORTED until a GPU backend exists.",
                    json!({"devices": "all"}))
            }
            fn run(&self, ctx: &TestContext) -> TestResult {
                unsupported(self.descriptor(), ctx, $name)
            }
        }
    };
}

planned_gpu_test!(
    GpuComputePlanned,
    "BE-T-GPU-002",
    "GPU Compute Verification",
    Gpu,
    Moderate,
    "Deterministic compute kernels compared with CPU reference results, per GPU and simultaneously."
);
planned_gpu_test!(
    GpuVramPlanned,
    "BE-T-GPU-003",
    "GPU VRAM Pattern Test",
    Vram,
    Moderate,
    "Pattern writes/reads in owned VRAM allocations with mismatch detection and ECC counter review."
);
planned_gpu_test!(
    GpuGraphicsPlanned,
    "BE-T-GPU-004",
    "GPU Graphics Stress (FurMark-style)",
    Gpu,
    High,
    "Sustained rendering load with repeated scenes and deterministic frame verification where possible."
);

#[cfg(test)]
mod tests {
    use super::*;
    use be_core::inventory::GpuDevice;

    #[test]
    fn gpu_evaluation() {
        let mut r =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(
            evaluate_gpus(&Inventory::default(), None, &mut r),
            TestStatus::Unsupported
        );
        let mut r =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(
            evaluate_gpus(&Inventory::default(), Some(2), &mut r),
            TestStatus::Fail
        );
        let inv = Inventory {
            gpus: vec![GpuDevice {
                id: "0000:01:00.0".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut r =
            TestResult::not_executed("X", "1", TestStatus::Running, "", Provenance::Fixture);
        assert_eq!(evaluate_gpus(&inv, None, &mut r), TestStatus::Warning);
        assert_eq!(r.faults[0].code, "BE-GPU-020");
    }
}
