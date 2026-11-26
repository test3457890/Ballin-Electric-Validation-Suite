//! BE Validation GPU diagnostic engine (Vulkan backend via `ash`).
//!
//! Backend-neutral entry points (`enumerate`, `compute_kat`, `vram_patterns`,
//! `graphics_load`) so CUDA/HIP backends can be added behind the same API
//! later. Every result is checked against an independently computed known
//! answer; nothing is assumed to have passed.
//!
//! Software Vulkan devices (e.g. Mesa llvmpipe) are reported with
//! `software = true` and are excluded from GPU tests unless explicitly
//! requested; results on them are never evidence about GPU hardware.

pub mod reference;
pub mod vk;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
pub use vk::{DeviceInfo, VkError};

/// Enumerate Vulkan physical devices. `Err` means Vulkan is unavailable.
pub fn enumerate() -> Result<Vec<DeviceInfo>, VkError> {
    let inst = vk::Instance::new()?;
    let pds = inst.physical_devices()?;
    Ok(pds
        .iter()
        .enumerate()
        .map(|(i, pd)| inst.describe(i, *pd))
        .collect())
}

/// Outcome of a compute known-answer run.
#[derive(Debug, Clone, Default)]
pub struct KatOutcome {
    pub elements: u64,
    pub dispatches: u64,
    pub mismatches: u64,
    pub first: Vec<(u32, u32, u32)>,
    pub elapsed: Duration,
    pub error: Option<String>,
    pub device_lost: bool,
    pub cancelled: bool,
}

/// Deterministic compute kernel compared element-by-element with the CPU
/// reference. Repeats `rounds` times with different seeds.
pub fn compute_kat(
    device_index: usize,
    elements: u32,
    iterations: u32,
    rounds: u32,
    cancel: &AtomicBool,
) -> KatOutcome {
    let t0 = Instant::now();
    let mut out = KatOutcome::default();
    let r = (|| -> Result<(), VkError> {
        let inst = vk::Instance::new()?;
        let pd = *inst
            .physical_devices()?
            .get(device_index)
            .ok_or_else(|| VkError::Other("device index out of range".into()))?;
        let info = inst.describe(device_index, pd);
        let groups_max = info.max_workgroup_count_x as u64 * 64;
        let n = (elements as u64)
            .min(groups_max)
            .min(info.max_storage_buffer_range as u64 / 4) as u32;
        let mut dev = vk::Device::new(&inst, pd, false)?;
        let buf = dev.buffer(n as u64 * 4, true)?;
        let (pipe, layout, sl) = dev.compute_pipeline(vk::KAT_SPV, 1, 16)?;
        let set = dev.descriptor_sets(sl, &[vec![buf]])?[0];
        for round in 0..rounds {
            if cancel.load(Ordering::Relaxed) {
                out.cancelled = true;
                return Ok(());
            }
            let seed = 0xBE01_0000u32 ^ round.wrapping_mul(0x9E37_79B9);
            dev.zero(buf);
            dev.submit(|d, cb| {
                dispatch(
                    d,
                    cb,
                    pipe,
                    layout,
                    set,
                    &pc(&[seed, iterations, n, 0]),
                    n.div_ceil(64),
                    true,
                )
            })?;
            let got = dev.read_u32(buf, n as usize);
            for (i, g) in got.iter().enumerate() {
                let want = reference::kat_element(i as u32, seed, iterations);
                if *g != want {
                    out.mismatches += 1;
                    if out.first.len() < 8 {
                        out.first.push((i as u32, want, *g));
                    }
                }
            }
            out.elements += n as u64;
            out.dispatches += 1;
        }
        Ok(())
    })();
    if let Err(e) = r {
        out.device_lost = matches!(e, VkError::DeviceLost);
        out.error = Some(e.to_string());
    }
    out.elapsed = t0.elapsed();
    out
}

#[derive(Debug, Clone, Default)]
pub struct VramOutcome {
    pub requested_mib: u64,
    pub tested_mib: u64,
    pub chunks: usize,
    pub patterns_completed: u64,
    pub words_verified: u64,
    pub mismatches: u64,
    /// (word offset, expected, actual)
    pub first: Vec<(u64, u32, u32)>,
    pub allocation_limited: bool,
    pub elapsed: Duration,
    pub error: Option<String>,
    pub device_lost: bool,
    pub cancelled: bool,
}

/// Pattern names in the order the VRAM shader implements them.
pub const VRAM_PATTERNS: &[&str] = &[
    "own address",
    "walking ones",
    "checkerboard",
    "pseudo-random",
    "zeros",
    "ones",
];

/// VRAM pattern test over owned device-local allocations (moving inversions
/// per pattern: write, verify+invert, verify inverted+restore, verify).
///
/// Coverage is limited to memory the driver lets this process allocate;
/// driver/OS reservations and other processes' memory are not covered.
/// `inject_fault_at` is a TEST-ONLY hook (not reachable from profiles): it
/// flips one bit between write and verify to prove detection works.
pub fn vram_patterns(
    device_index: usize,
    mib: u64,
    passes: u32,
    cancel: &AtomicBool,
    inject_fault_at: Option<u32>,
) -> VramOutcome {
    let t0 = Instant::now();
    let mut out = VramOutcome {
        requested_mib: mib,
        ..Default::default()
    };
    let r = (|| -> Result<(), VkError> {
        let inst = vk::Instance::new()?;
        let pd = *inst
            .physical_devices()?
            .get(device_index)
            .ok_or_else(|| VkError::Other("device index out of range".into()))?;
        let info = inst.describe(device_index, pd);
        // Never take more than half of the largest device-local heap.
        let cap = (info.device_local_bytes / 2) / (1 << 20);
        let want = mib.min(cap.max(1));
        if want < mib {
            out.allocation_limited = true;
        }
        let max_words_dispatch = info.max_workgroup_count_x as u64 * 256;
        let chunk_bytes = (64u64 << 20)
            .min(max_words_dispatch * 4)
            .min(info.max_storage_buffer_range as u64 & !3)
            .min(info.max_alloc_bytes.unwrap_or(u64::MAX));
        let mut dev = vk::Device::new(&inst, pd, false)?;
        dev.timeout = Duration::from_secs(60);
        let result = dev.buffer(4 * 26, true)?;
        let mut chunks = Vec::new();
        let mut remaining = want << 20;
        while remaining > 0 {
            let sz = remaining.min(chunk_bytes);
            match dev.buffer(sz, false) {
                Ok(b) => chunks.push(b),
                Err(VkError::OutOfMemory) => {
                    out.allocation_limited = true;
                    break;
                }
                Err(e) => return Err(e),
            }
            remaining -= sz;
        }
        if chunks.is_empty() {
            return Err(VkError::OutOfMemory);
        }
        out.chunks = chunks.len();
        out.tested_mib = chunks.iter().map(|c| dev.buf(*c).size).sum::<u64>() >> 20;
        let (pipe, layout, sl) = dev.compute_pipeline(vk::VRAM_SPV, 2, 20)?;
        let sets = dev.descriptor_sets(
            sl,
            &chunks.iter().map(|c| vec![*c, result]).collect::<Vec<_>>(),
        )?;
        let words: Vec<u32> = chunks
            .iter()
            .map(|c| (dev.buf(*c).size / 4) as u32)
            .collect();
        let mut offsets = Vec::with_capacity(words.len());
        let mut acc = 0u32;
        for w in &words {
            offsets.push(acc);
            acc = acc.wrapping_add(*w);
        }
        dev.zero(result);
        for pass in 0..passes {
            for pat in 0..VRAM_PATTERNS.len() as u32 {
                if cancel.load(Ordering::Relaxed) {
                    out.cancelled = true;
                    return Ok(());
                }
                let seed = 0x5EED_0000 ^ pass.wrapping_mul(0x0101_0101) ^ pat;
                dev.submit(|d, cb| {
                    for (i, s) in sets.iter().enumerate() {
                        dispatch(
                            d,
                            cb,
                            pipe,
                            layout,
                            *s,
                            &pc(&[0, pat, seed, words[i], offsets[i]]),
                            words[i].div_ceil(256),
                            false,
                        );
                    }
                    vk::Device::compute_barrier(d, cb, false);
                    if let (Some(at), true) = (inject_fault_at, pass == 0 && pat == 0) {
                        dispatch(
                            d,
                            cb,
                            pipe,
                            layout,
                            sets[0],
                            &pc(&[4, pat, at.min(words[0] - 1), words[0], offsets[0]]),
                            1,
                            false,
                        );
                        vk::Device::compute_barrier(d, cb, false);
                    }
                    for mode in [2u32, 3, 1] {
                        for (i, s) in sets.iter().enumerate() {
                            dispatch(
                                d,
                                cb,
                                pipe,
                                layout,
                                *s,
                                &pc(&[mode, pat, seed, words[i], offsets[i]]),
                                words[i].div_ceil(256),
                                false,
                            );
                        }
                        vk::Device::compute_barrier(d, cb, mode == 1);
                    }
                })?;
                out.patterns_completed += 1;
                out.words_verified += 3 * words.iter().map(|w| *w as u64).sum::<u64>();
            }
        }
        let r = dev.read_u32(result, 26);
        out.mismatches = r[0] as u64;
        for k in 0..(r[1].min(8) as usize) {
            out.first.push((r[2 + k] as u64, r[10 + k], r[18 + k]));
        }
        Ok(())
    })();
    if let Err(e) = r {
        out.device_lost = matches!(e, VkError::DeviceLost);
        out.error = Some(e.to_string());
    }
    out.elapsed = t0.elapsed();
    out
}

#[derive(Debug, Clone, Default)]
pub struct GraphicsOutcome {
    pub frames: u64,
    pub verification_frames: u64,
    pub inconsistent_frames: u64,
    pub reference_hash: u64,
    pub resolution: (u32, u32),
    pub layers: u32,
    pub elapsed: Duration,
    pub error: Option<String>,
    pub device_lost: bool,
    pub cancelled: bool,
}

/// Parameters for the sustained graphics workload.
#[derive(Debug, Clone, Copy)]
pub struct GraphicsParams {
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub frames_per_submit: u32,
    pub verify_every: u32,
}

/// Sustained offscreen fragment workload (original "fur ring" shader). Every
/// `verify_every`-th submission renders the fixed reference frame (t = 0),
/// reads it back and compares its hash with the first reference frame; any
/// difference indicates non-deterministic rendering (possible corruption).
/// The run continues past `duration` until at least two reference frames
/// have been compared.
pub fn graphics_load(
    device_index: usize,
    p: GraphicsParams,
    cancel: &AtomicBool,
    progress: &dyn Fn(f64),
) -> GraphicsOutcome {
    let t0 = Instant::now();
    let mut out = GraphicsOutcome {
        resolution: (p.width, p.height),
        layers: p.layers,
        ..Default::default()
    };
    let r = (|| -> Result<(), VkError> {
        let inst = vk::Instance::new()?;
        let pd = *inst
            .physical_devices()?
            .get(device_index)
            .ok_or_else(|| VkError::Other("device index out of range".into()))?;
        let mut dev = vk::Device::new(&inst, pd, true)?;
        let tgt = dev.graphics_target(p.width, p.height)?;
        let rb = dev.buf(tgt.readback).buffer;
        let mut submit_no = 0u64;
        let mut reference: Option<u64> = None;
        loop {
            if cancel.load(Ordering::Relaxed) {
                out.cancelled = true;
                break;
            }
            let verify =
                reference.is_none() || submit_no.is_multiple_of(p.verify_every.max(1) as u64);
            let t_base = out.frames as f32 * 0.016;
            dev.submit(|d, cb| {
                if verify {
                    tgt.record(d, cb, 0.0, p.layers, Some(rb));
                } else {
                    for f in 0..p.frames_per_submit {
                        tgt.record(d, cb, t_base + f as f32 * 0.016, p.layers, None);
                    }
                }
            })?;
            if verify {
                let hash = reference::fnv1a64(
                    &dev.read_bytes(tgt.readback, (p.width * p.height * 4) as usize),
                );
                out.verification_frames += 1;
                match reference {
                    None => {
                        reference = Some(hash);
                        out.reference_hash = hash;
                    }
                    Some(r) if r != hash => out.inconsistent_frames += 1,
                    _ => {}
                }
                out.frames += 1;
            } else {
                out.frames += p.frames_per_submit as u64;
            }
            submit_no += 1;
            progress((t0.elapsed().as_secs_f64() / p.duration.as_secs_f64().max(0.001)).min(1.0));
            // Always compare at least two reference frames, whatever the
            // duration, so every run actually verifies rendering.
            if t0.elapsed() >= p.duration && out.verification_frames >= 2 {
                break;
            }
        }
        Ok(())
    })();
    if let Err(e) = r {
        out.device_lost = matches!(e, VkError::DeviceLost);
        out.error = Some(e.to_string());
    }
    out.elapsed = t0.elapsed();
    out
}

fn pc(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[allow(clippy::too_many_arguments)]
fn dispatch(
    d: &ash::Device,
    cb: ash::vk::CommandBuffer,
    pipe: ash::vk::Pipeline,
    layout: ash::vk::PipelineLayout,
    set: ash::vk::DescriptorSet,
    pc: &[u8],
    groups: u32,
    host_barrier: bool,
) {
    // SAFETY: called while recording `cb`; pipeline/layout/set come from the
    // same Device and outlive the submission (Device waits before Drop).
    unsafe {
        d.cmd_bind_pipeline(cb, ash::vk::PipelineBindPoint::COMPUTE, pipe);
        d.cmd_bind_descriptor_sets(
            cb,
            ash::vk::PipelineBindPoint::COMPUTE,
            layout,
            0,
            &[set],
            &[],
        );
        d.cmd_push_constants(cb, layout, ash::vk::ShaderStageFlags::COMPUTE, 0, pc);
        d.cmd_dispatch(cb, groups.max(1), 1, 1);
    }
    if host_barrier {
        vk::Device::compute_barrier(d, cb, true);
    }
}

#[cfg(test)]
mod tests {
    //! These tests need a Vulkan implementation. Without one they print
    //! SKIPPED and return. CI installs Mesa lavapipe, a SOFTWARE device:
    //! passing there validates the code paths, not GPU hardware.
    use super::*;

    fn first_device() -> Option<usize> {
        match enumerate() {
            Ok(d) if !d.is_empty() => Some(0),
            Ok(_) => {
                eprintln!("SKIPPED: no Vulkan devices");
                None
            }
            Err(e) => {
                eprintln!("SKIPPED: {e}");
                None
            }
        }
    }

    #[test]
    fn enumerate_reports_identity() {
        if let Ok(devs) = enumerate() {
            for d in devs {
                assert!(!d.name.is_empty());
                assert!(d.has_compute);
            }
        }
    }

    #[test]
    fn compute_matches_reference() {
        let Some(i) = first_device() else { return };
        let o = compute_kat(i, 100_000, 64, 2, &AtomicBool::new(false));
        assert_eq!(o.error, None);
        assert_eq!(o.mismatches, 0, "{:?}", o.first);
        assert_eq!(o.elements, 200_000);
    }

    #[test]
    fn vram_clean_and_injected_fault_detected() {
        let Some(i) = first_device() else { return };
        let clean = vram_patterns(i, 8, 1, &AtomicBool::new(false), None);
        assert_eq!(clean.error, None);
        assert_eq!(clean.mismatches, 0, "{:?}", clean.first);
        assert_eq!(clean.patterns_completed, VRAM_PATTERNS.len() as u64);
        let bad = vram_patterns(i, 8, 1, &AtomicBool::new(false), Some(12_345));
        assert_eq!(bad.error, None);
        assert!(bad.mismatches >= 1, "injected fault must be detected");
        assert_eq!(bad.first[0].0, 12_345);
        assert_eq!((bad.first[0].1 ^ bad.first[0].2).count_ones(), 1);
    }

    #[test]
    fn graphics_frames_are_consistent() {
        let Some(i) = first_device() else { return };
        let p = GraphicsParams {
            duration: Duration::from_millis(800),
            width: 128,
            height: 96,
            layers: 8,
            frames_per_submit: 2,
            verify_every: 2,
        };
        let o = graphics_load(i, p, &AtomicBool::new(false), &|_| {});
        assert_eq!(o.error, None);
        assert!(o.verification_frames >= 2);
        assert_eq!(o.inconsistent_frames, 0);
        assert_ne!(o.reference_hash, 0);
    }

    #[test]
    fn cancellation_stops_work() {
        let Some(i) = first_device() else { return };
        let o = compute_kat(i, 1000, 4, 10, &AtomicBool::new(true));
        assert!(o.cancelled);
        assert_eq!(o.dispatches, 0);
    }
}
