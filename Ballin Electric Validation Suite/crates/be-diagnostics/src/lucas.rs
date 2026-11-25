//! Lucas-Lehmer primality test for Mersenne numbers M_p = 2^p - 1.
//!
//! Used as a verified integer workload (in the spirit of Prime95): for any
//! prime p, M_p is prime iff the final LL residue is zero. The list of
//! Mersenne prime exponents below 23,209 is an established mathematical
//! fact, so every run has a known answer.

/// All Mersenne prime exponents p <= 23209 (complete list).
pub const MERSENNE_EXPONENTS: &[u32] = &[
    2, 3, 5, 7, 13, 17, 19, 31, 61, 89, 107, 127, 521, 607, 1279, 2203, 2281, 3217, 4253, 4423,
    9689, 9941, 11213, 19937, 21701, 23209,
];
/// Upper bound for which `expected_prime` is authoritative.
pub const KNOWN_LIMIT: u32 = 23209;

pub fn is_prime_u32(n: u32) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2u32;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 1;
    }
    true
}

/// Known answer: whether M_p is prime. `None` if p is outside the known range
/// or not prime.
pub fn expected_prime(p: u32) -> Option<bool> {
    if p > KNOWN_LIMIT || !is_prime_u32(p) {
        return None;
    }
    Some(MERSENNE_EXPONENTS.contains(&p))
}

/// Compute the Lucas-Lehmer residue for odd prime p >= 3. Returns `true`
/// when the residue is zero (M_p prime). `cancel` is polled periodically.
pub fn lucas_lehmer(p: u32, cancel: &dyn Fn() -> bool) -> Option<bool> {
    assert!(p >= 3);
    let n = (p as usize).div_ceil(64);
    let mut s = vec![0u64; n];
    s[0] = 4;
    let mut prod = vec![0u64; 2 * n];
    for i in 0..(p - 2) {
        if i % 256 == 0 && cancel() {
            return None;
        }
        square(&s, &mut prod);
        reduce_mersenne(&prod, p, &mut s);
        sub2_mod(&mut s, p);
    }
    Some(is_zero_mod(&s, p))
}

fn square(a: &[u64], out: &mut [u64]) {
    out.iter_mut().for_each(|x| *x = 0);
    let n = a.len();
    for i in 0..n {
        let mut carry: u128 = 0;
        let ai = a[i] as u128;
        if ai == 0 {
            continue;
        }
        for j in 0..n {
            let t = ai * a[j] as u128 + out[i + j] as u128 + carry;
            out[i + j] = t as u64;
            carry = t >> 64;
        }
        let mut k = i + n;
        while carry != 0 {
            let t = out[k] as u128 + carry;
            out[k] = t as u64;
            carry = t >> 64;
            k += 1;
        }
    }
}

/// Extract `count` bits starting at bit `start` from `x` into `dst`.
fn bits(x: &[u64], start: usize, count: usize, dst: &mut [u64]) {
    dst.iter_mut().for_each(|d| *d = 0);
    let words = count.div_ceil(64);
    for w in 0..words {
        let bit = start + w * 64;
        let (wi, off) = (bit / 64, bit % 64);
        let lo = x.get(wi).copied().unwrap_or(0) >> off;
        let hi = if off == 0 {
            0
        } else {
            x.get(wi + 1).copied().unwrap_or(0) << (64 - off)
        };
        dst[w] = lo | hi;
    }
    let rem = count % 64;
    if rem != 0 {
        dst[words - 1] &= (1u64 << rem) - 1;
    }
}

fn add_in_place(a: &mut [u64], b: &[u64]) -> u64 {
    let mut carry = 0u64;
    for i in 0..a.len() {
        let (s1, c1) = a[i].overflowing_add(b[i]);
        let (s2, c2) = s1.overflowing_add(carry);
        a[i] = s2;
        carry = (c1 as u64) + (c2 as u64);
    }
    carry
}

/// x mod (2^p - 1) for x < 2^(2p), using 2^p ≡ 1.
fn reduce_mersenne(x: &[u64], p: u32, out: &mut [u64]) {
    let p = p as usize;
    let n = out.len();
    let mut hi = vec![0u64; n];
    bits(x, 0, p, out);
    bits(x, p, p, &mut hi);
    let carry = add_in_place(out, &hi);
    debug_assert_eq!(carry, 0);
    // Result < 2^(p+1); fold the bit at position p once more.
    fold_once(out, p);
}

fn fold_once(v: &mut [u64], p: usize) {
    let (wi, off) = (p / 64, p % 64);
    let top = if wi < v.len() { (v[wi] >> off) & 1 } else { 0 };
    if top == 1 {
        v[wi] &= !(1u64 << off);
        let mut i = 0;
        let mut carry = 1u64;
        while carry != 0 && i < v.len() {
            let (s, c) = v[i].overflowing_add(carry);
            v[i] = s;
            carry = c as u64;
            i += 1;
        }
    }
}

fn is_all_ones(v: &[u64], p: usize) -> bool {
    let full = p / 64;
    let rem = p % 64;
    v[..full].iter().all(|w| *w == u64::MAX) && (rem == 0 || v[full] == (1u64 << rem) - 1)
}

fn is_zero_mod(v: &[u64], p: u32) -> bool {
    v.iter().all(|w| *w == 0) || is_all_ones(v, p as usize)
}

/// s = (s - 2) mod M_p, with s in [0, M_p].
fn sub2_mod(s: &mut [u64], p: u32) {
    let small = s[1..].iter().all(|w| *w == 0) && s[0] < 2;
    if small {
        // s + M_p - 2: since M_p = all ones (p bits), M_p - 2 + s.
        let add = s[0];
        let pz = p as usize;
        for (i, w) in s.iter_mut().enumerate() {
            let bits_here = pz.saturating_sub(i * 64).min(64);
            *w = if bits_here == 64 {
                u64::MAX
            } else if bits_here == 0 {
                0
            } else {
                (1u64 << bits_here) - 1
            };
        }
        // now s = M_p; subtract (2 - add)
        let sub = 2 - add;
        s[0] -= sub; // M_p low word is all-ones in low bits (>= 3 for p >= 2)
    } else {
        let mut borrow = 2u64;
        for w in s.iter_mut() {
            let (r, b) = w.overflowing_sub(borrow);
            *w = r;
            borrow = b as u64;
            if borrow == 0 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answers_small_and_medium() {
        let never = || false;
        for p in (3..=1279).filter(|p| is_prime_u32(*p)) {
            let exp = expected_prime(p).unwrap();
            assert_eq!(lucas_lehmer(p, &never), Some(exp), "p={p}");
        }
    }

    #[test]
    fn cancellation_returns_none() {
        assert_eq!(lucas_lehmer(4423, &|| true), None);
    }

    #[test]
    fn expected_table() {
        assert_eq!(expected_prime(11), Some(false));
        assert_eq!(expected_prime(4423), Some(true));
        assert_eq!(expected_prime(4), None);
        assert_eq!(expected_prime(30_011), None);
    }
}
