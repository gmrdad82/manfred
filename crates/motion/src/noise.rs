const MIX_A: u64 = 0xff51_afd7_ed55_8ccd;
const MIX_B: u64 = 0xc4ce_b9fe_1a85_ec53;
const CELL: u64 = 0x9e37_79b9_7f4a_7c15;
const OCTAVE: f64 = 2.031;
const SHIFT: f64 = 17.31;
const NORM: f64 = 2.0 / 1.5;

fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(MIX_A);
    h ^= h >> 29;
    h = h.wrapping_mul(MIX_B);
    h ^= h >> 32;
    h
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn fade_slope(t: f64) -> f64 {
    30.0 * t * t * (t - 1.0) * (t - 1.0)
}

fn gradient(seed: u64, cell: i64, octave: u64) -> f64 {
    let h = mix(seed ^ mix((cell as u64).wrapping_mul(CELL) ^ octave));
    (h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

fn one(seed: u64, x: f64, octave: u64) -> (f64, f64) {
    let cell = x.floor();
    let f = x - cell;
    let g0 = gradient(seed, cell as i64, octave);
    let g1 = gradient(seed, cell as i64 + 1, octave);
    let (a, b) = (g0 * f, g1 * (f - 1.0));
    let u = fade(f);
    let value = a + u * (b - a);
    let slope = g0 * (1.0 - u) + g1 * u + fade_slope(f) * (b - a);
    (value, slope)
}

fn both(seed: u64, x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (0.0, 0.0);
    }
    let seed = mix(seed);
    let (v0, s0) = one(seed, x, 0);
    let (v1, s1) = one(seed, x * OCTAVE + SHIFT, 1);
    ((v0 + 0.5 * v1) * NORM, (s0 + 0.5 * OCTAVE * s1) * NORM)
}

pub fn noise1(seed: u64, x: f64) -> f64 {
    both(seed, x).0.clamp(-1.0, 1.0)
}

pub fn noise1_slope(seed: u64, x: f64) -> f64 {
    both(seed, x).1
}

#[cfg(test)]
mod tests {
    use super::{noise1, noise1_slope};

    #[test]
    fn a_seed_gives_the_same_smooth_wander_every_time() {
        let at = |seed: u64| -> Vec<f64> {
            (0..400)
                .map(|i| noise1(seed, i as f64 * 0.037 - 3.0))
                .collect()
        };
        let wander = at(7);
        assert_eq!(wander, at(7));
        assert_ne!(wander, at(8));
        assert!(wander.iter().all(|v| (-1.0..=1.0).contains(v)));
        assert!(wander.iter().any(|v| v.abs() > 0.2));
        let h = 1e-6;
        for x in [0.1, 1.7, -4.4, 12.9] {
            let numeric = (noise1(7, x + h) - noise1(7, x - h)) / (2.0 * h);
            assert!((numeric - noise1_slope(7, x)).abs() < 1e-4, "{x}");
        }
    }
}
