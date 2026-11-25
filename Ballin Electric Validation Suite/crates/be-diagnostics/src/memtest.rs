//! Allocated-buffer memory pattern tests.
//!
//! Coverage is limited to the virtual buffer the OS gives this process; it
//! cannot reach kernel-reserved or other processes' physical memory. A
//! preboot test is required for broad physical coverage.

/// Volatile accessors. Volatile access guarantees every write and read is
/// actually performed against memory (not optimized away or kept in
/// registers). The only unsafe code in this module.
mod volatile {
    #[inline(always)]
    pub fn write(buf: &mut [u64], i: usize, v: u64) {
        let p: *mut u64 = &mut buf[i];
        // SAFETY: `p` is derived from a valid, aligned, exclusive reference
        // to an element of `buf` (bounds checked by indexing above).
        unsafe { std::ptr::write_volatile(p, v) }
    }
    #[inline(always)]
    pub fn read(buf: &[u64], i: usize) -> u64 {
        let p: *const u64 = &buf[i];
        // SAFETY: `p` is derived from a valid, aligned shared reference to an
        // element of `buf` (bounds checked by indexing above).
        unsafe { std::ptr::read_volatile(p) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Zeros,
    Ones,
    Checkerboard,
    WalkingOnes,
    WalkingZeros,
    OwnAddress,
    Random(u64),
    MovingInversions(u64),
}

impl Pattern {
    pub fn name(&self) -> String {
        match self {
            Pattern::Zeros => "solid zeros".into(),
            Pattern::Ones => "solid ones".into(),
            Pattern::Checkerboard => "checkerboard".into(),
            Pattern::WalkingOnes => "walking ones".into(),
            Pattern::WalkingZeros => "walking zeros".into(),
            Pattern::OwnAddress => "own address".into(),
            Pattern::Random(s) => format!("random (seed {s:#x})"),
            Pattern::MovingInversions(p) => format!("moving inversions ({p:#018x})"),
        }
    }

    pub fn standard_set(pass: u64) -> Vec<Pattern> {
        vec![
            Pattern::Zeros,
            Pattern::Ones,
            Pattern::Checkerboard,
            Pattern::WalkingOnes,
            Pattern::WalkingZeros,
            Pattern::OwnAddress,
            Pattern::Random(0x9E37_79B9_7F4A_7C15 ^ pass),
            Pattern::MovingInversions(0x5555_5555_5555_5555 ^ pass.rotate_left(17)),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub word_index: usize,
    pub expected: u64,
    pub actual: u64,
}

impl Mismatch {
    pub fn bit_flips(&self) -> u32 {
        (self.expected ^ self.actual).count_ones()
    }
}

#[derive(Debug, Default, Clone)]
pub struct PatternOutcome {
    pub mismatches: Vec<Mismatch>,
    pub mismatch_count: u64,
    pub words_verified: u64,
    pub cancelled: bool,
}

const MAX_RECORDED: usize = 32;
const CHUNK: usize = 1 << 16;

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn value(p: Pattern, base: u64, i: usize) -> u64 {
    match p {
        Pattern::Zeros => 0,
        Pattern::Ones => u64::MAX,
        Pattern::Checkerboard => {
            if i.is_multiple_of(2) {
                0x5555_5555_5555_5555
            } else {
                0xAAAA_AAAA_AAAA_AAAA
            }
        }
        Pattern::WalkingOnes => 1u64 << (i % 64),
        Pattern::WalkingZeros => !(1u64 << (i % 64)),
        Pattern::OwnAddress => base.wrapping_add((i * 8) as u64),
        Pattern::Random(seed) => splitmix(seed ^ i as u64),
        Pattern::MovingInversions(p) => p,
    }
}

fn verify(
    buf: &[u64],
    p: Pattern,
    expected_inverted: bool,
    out: &mut PatternOutcome,
    idx: impl Iterator<Item = usize>,
) {
    let base = buf.as_ptr() as u64;
    for i in idx {
        let mut e = value(p, base, i);
        if expected_inverted {
            e = !e;
        }
        let a = volatile::read(buf, i);
        if a != e {
            out.mismatch_count += 1;
            if out.mismatches.len() < MAX_RECORDED {
                out.mismatches.push(Mismatch {
                    word_index: i,
                    expected: e,
                    actual: a,
                });
            }
        }
        out.words_verified += 1;
    }
}

/// Run one pattern over `buf`. `between` is invoked after the write phase
/// and before verification (a no-op in production; tests use it to inject
/// faults). `cancel` is polled per chunk.
pub fn run_pattern(
    buf: &mut [u64],
    p: Pattern,
    cancel: &dyn Fn() -> bool,
    between: &mut dyn FnMut(&mut [u64]),
) -> PatternOutcome {
    let mut out = PatternOutcome::default();
    let n = buf.len();
    let base = buf.as_ptr() as u64;
    match p {
        Pattern::MovingInversions(_) => {
            // Ascending fill.
            for start in (0..n).step_by(CHUNK) {
                if cancel() {
                    out.cancelled = true;
                    return out;
                }
                for i in start..(start + CHUNK).min(n) {
                    volatile::write(buf, i, value(p, base, i));
                }
            }
            between(buf);
            // Ascending: verify p, write !p.
            for start in (0..n).step_by(CHUNK) {
                if cancel() {
                    out.cancelled = true;
                    return out;
                }
                let end = (start + CHUNK).min(n);
                verify(buf, p, false, &mut out, start..end);
                for i in start..end {
                    volatile::write(buf, i, !value(p, base, i));
                }
            }
            // Descending: verify !p, write p.
            let mut end = n;
            while end > 0 {
                if cancel() {
                    out.cancelled = true;
                    return out;
                }
                let start = end.saturating_sub(CHUNK);
                verify(buf, p, true, &mut out, (start..end).rev());
                for i in (start..end).rev() {
                    volatile::write(buf, i, value(p, base, i));
                }
                end = start;
            }
            verify(buf, p, false, &mut out, 0..n);
        }
        _ => {
            for start in (0..n).step_by(CHUNK) {
                if cancel() {
                    out.cancelled = true;
                    return out;
                }
                for i in start..(start + CHUNK).min(n) {
                    volatile::write(buf, i, value(p, base, i));
                }
            }
            between(buf);
            for start in (0..n).step_by(CHUNK) {
                if cancel() {
                    out.cancelled = true;
                    return out;
                }
                verify(buf, p, false, &mut out, start..(start + CHUNK).min(n));
            }
        }
    }
    out
}

/// Try to allocate `words` u64s, halving on failure down to `min_words`.
pub fn allocate(words: usize, min_words: usize) -> Option<Vec<u64>> {
    let mut w = words;
    while w >= min_words.max(1) {
        let mut v: Vec<u64> = Vec::new();
        if v.try_reserve_exact(w).is_ok() {
            v.resize(w, 0);
            return Some(v);
        }
        w /= 2;
    }
    None
}

#[cfg(test)]
mod tests {
    //! Fault paths are exercised by injecting corruption between the write
    //! and verify phases (FIXTURE fault injection, test-only).
    use super::*;

    #[test]
    fn clean_buffer_passes_all_patterns() {
        let mut buf = vec![0u64; 100_003];
        for p in Pattern::standard_set(1) {
            let o = run_pattern(&mut buf, p, &|| false, &mut |_| {});
            assert_eq!(o.mismatch_count, 0, "{}", p.name());
            assert!(o.words_verified >= buf.len() as u64);
        }
    }

    #[test]
    fn injected_bit_flip_is_detected_for_every_pattern() {
        let mut buf = vec![0u64; 70_000];
        for p in Pattern::standard_set(7) {
            let o = run_pattern(&mut buf, p, &|| false, &mut |b| b[12_345] ^= 1 << 13);
            assert!(o.mismatch_count >= 1, "{} missed fault", p.name());
            assert_eq!(o.mismatches[0].word_index, 12_345);
            assert_eq!(o.mismatches[0].bit_flips(), 1);
        }
    }

    #[test]
    fn cancellation_is_honoured() {
        let mut buf = vec![0u64; 200_000];
        let o = run_pattern(&mut buf, Pattern::Ones, &|| true, &mut |_| {});
        assert!(o.cancelled);
        assert_eq!(o.words_verified, 0);
    }

    #[test]
    fn allocation_reduces_gracefully() {
        let v = allocate(1024, 16).unwrap();
        assert_eq!(v.len(), 1024);
        assert!(allocate(usize::MAX / 16, usize::MAX / 16).is_none());
    }
}
