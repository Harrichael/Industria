//! Tiny deterministic PRNG (SplitMix64 seeded xoshiro256**).
//!
//! We avoid an external `rand` dependency so the crate builds trivially for
//! `wasm32-unknown-unknown`, and so a seed always reproduces the same map on
//! every platform.

#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

fn splitmix(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        Rng {
            s: [
                splitmix(&mut x),
                splitmix(&mut x),
                splitmix(&mut x),
                splitmix(&mut x),
            ],
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform float in `[0, 1)`.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform integer in `[0, n)`. `n` must be > 0.
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        (((self.next_u64() >> 32) * n as u64) >> 32) as usize
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let x = a.below(17);
            assert_eq!(x, b.below(17));
            assert!(x < 17);
            let f = a.f64();
            assert_eq!(f, b.f64());
            assert!((0.0..1.0).contains(&f));
        }
    }
}
