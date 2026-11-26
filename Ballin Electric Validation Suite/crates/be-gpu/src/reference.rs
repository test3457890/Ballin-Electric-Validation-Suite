//! CPU reference implementations for GPU known-answer tests. They mirror the
//! GLSL in `shaders/` exactly (wrapping u32 arithmetic, exact small-integer
//! floating point).

/// Known answer for element `idx` of `kat.comp`.
pub fn kat_element(idx: u32, seed: u32, iterations: u32) -> u32 {
    let mut x = idx ^ seed;
    for _ in 0..iterations {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        x ^= x >> 13;
    }
    let f = (idx & 0xFFF) as f32;
    let mut acc = 0.0f32;
    for k in 0..16u32 {
        acc = f.mul_add((k + 1) as f32, acc);
    }
    x ^ acc.to_bits()
}

/// FNV-1a 64-bit hash used to compare rendered frames.
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn float_part_is_exact() {
        // 4095 * (1+2+...+16) = 4095 * 136 = 556920, exactly representable.
        let a = kat_element(0xFFF, 0, 0) ^ 0xFFF;
        assert_eq!(f32::from_bits(a), 556_920.0);
    }
    #[test]
    fn fnv_known_vector() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
