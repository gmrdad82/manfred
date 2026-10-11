use manfred_mesh::math::{cos, ln, sin};

const N: usize = 624;
const M: usize = 397;

#[derive(Clone)]
pub struct Random {
    state: [u32; N],
    index: usize,
    gauss_next: Option<f64>,
}

impl Random {
    pub fn new(seed: i64) -> Random {
        let n = seed.unsigned_abs();
        let key: Vec<u32> = if n >> 32 == 0 {
            vec![n as u32]
        } else {
            vec![n as u32, (n >> 32) as u32]
        };
        Random::from_key(&key)
    }

    pub fn from_key(key: &[u32]) -> Random {
        let mut r = Random::init_genrand(19650218);
        let mt = &mut r.state;
        let len = key.len().max(1);
        let word = |j: usize| key.get(j).copied().unwrap_or(0);
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..N.max(len) {
            let prev = mt[i - 1] ^ (mt[i - 1] >> 30);
            mt[i] = (mt[i] ^ prev.wrapping_mul(1664525))
                .wrapping_add(word(j))
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
            if j >= len {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            let prev = mt[i - 1] ^ (mt[i - 1] >> 30);
            mt[i] = (mt[i] ^ prev.wrapping_mul(1566083941)).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        r
    }

    pub fn init_genrand(seed: u32) -> Random {
        let mut state = [0u32; N];
        state[0] = seed;
        for i in 1..N {
            let prev = state[i - 1] ^ (state[i - 1] >> 30);
            state[i] = 1812433253u32.wrapping_mul(prev).wrapping_add(i as u32);
        }
        Random {
            state,
            index: N,
            gauss_next: None,
        }
    }

    fn twist(&mut self) {
        let mt = &mut self.state;
        for k in 0..N {
            let y = (mt[k] & 0x8000_0000) | (mt[(k + 1) % N] & 0x7fff_ffff);
            let mag = if y & 1 == 0 { 0 } else { 0x9908_b0df };
            mt[k] = mt[(k + M) % N] ^ (y >> 1) ^ mag;
        }
        self.index = 0;
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    pub fn random(&mut self) -> f64 {
        let a = (self.next_u32() >> 5) as f64;
        let b = (self.next_u32() >> 6) as f64;
        (a * 67108864.0 + b) * (1.0 / 9007199254740992.0)
    }

    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }

    pub fn gauss(&mut self, mu: f64, sigma: f64) -> f64 {
        let z = match self.gauss_next.take() {
            Some(z) => z,
            None => {
                let x2pi = self.random() * std::f64::consts::TAU;
                let g2rad = (-2.0 * ln(1.0 - self.random())).sqrt();
                self.gauss_next = Some(sin(x2pi) * g2rad);
                cos(x2pi) * g2rad
            }
        };
        mu + z * sigma
    }

    pub fn getrandbits(&mut self, k: u32) -> u64 {
        assert!(k <= 64, "getrandbits takes at most 64 bits here");
        let mut out = 0u64;
        let mut left = k;
        let mut shift = 0;
        while left > 0 {
            let mut word = self.next_u32();
            if left < 32 {
                word >>= 32 - left;
            }
            out |= u64::from(word) << shift;
            shift += 32;
            left = left.saturating_sub(32);
        }
        out
    }

    pub fn randbelow(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let k = 64 - n.leading_zeros();
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    pub fn randrange(&mut self, start: i64, stop: i64) -> Result<i64, String> {
        if stop <= start {
            return Err(format!("empty range in randrange({start}, {stop})"));
        }
        let width = stop.abs_diff(start);
        Ok(start.wrapping_add(self.randbelow(width) as i64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEVEN: [(char, u64); 24] = [
        ('u', 4578620696950502336),
        ('g', 4605037505287289770),
        ('g', 4594262360113094154),
        ('g', 4593979658289145819),
        ('u', 4579585834107151568),
        ('g', 4602502154076679803),
        ('g', 4593975345765105125),
        ('g', 4587495938534341622),
        ('u', 4564559551050487173),
        ('g', 13822685238978272709),
        ('g', 4580987438172443972),
        ('g', 4592317422972513250),
        ('u', 4584927299014115704),
        ('g', 4602773632644756726),
        ('g', 4588918150712380143),
        ('g', 4588787291682034993),
        ('u', 4582891033956585681),
        ('g', 4607712083647492492),
        ('g', 13810853999431918802),
        ('g', 13821753484391832896),
        ('u', 4566235810775172956),
        ('g', 4609380649945396720),
        ('g', 4589169244391288620),
        ('g', 13813589804423961511),
    ];

    #[test]
    fn gauss_matches_cpython_between_uniforms() {
        let mut r = Random::new(7);
        for (k, (kind, want)) in SEVEN.iter().enumerate() {
            let got = match (kind, k % 4) {
                ('u', _) => r.uniform(0.0, 0.04),
                (_, 1) => r.gauss(0.0, 0.9),
                _ => r.gauss(0.0, 0.12),
            };
            assert_eq!(got.to_bits(), *want, "call {k}");
        }
    }

    #[test]
    fn gauss_keeps_its_pair_across_other_draws() {
        let mut r = Random::new(41);
        for _ in 0..997 {
            r.gauss(0.0, 1.0);
        }
        assert_eq!(r.gauss(0.5, 2.0).to_bits(), 13813924015812664584);
        assert_eq!(r.random().to_bits(), 4605200630389522199);
        assert_eq!(r.gauss(0.0, 1.0).to_bits(), 13823903433258845711);
    }

    #[test]
    fn init_genrand_seeds_like_numpys_random_state() {
        let mut r = Random::init_genrand(22);
        assert_eq!(r.random().to_bits(), 4596678602677501320);
        assert_eq!(r.random().to_bits(), 4602348814518981598);
        let mut r = Random::init_genrand(u32::MAX);
        assert_eq!(r.random().to_bits(), 4591699549773514784);
    }
}
