const SEED: u64 = 0x6d61_6e66_6e6f_6973;
const BASES: usize = 32;
const COUNT: usize = BASES * 24;
const SCALE: f64 = 1.456;

const AXIS_X: u64 = 0xa24b_aed4_963e_e407;
const AXIS_Y: u64 = 0x9fb2_1c65_1e98_df25;
const AXIS_Z: u64 = 0xd6e8_feb8_6659_fd93;
const MIX_A: u64 = 0xff51_afd7_ed55_8ccd;
const MIX_B: u64 = 0xc4ce_b9fe_1a85_ec53;

const fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(MIX_A);
    h ^= h >> 29;
    h = h.wrapping_mul(MIX_B);
    h ^= h >> 32;
    h
}

const fn root(value: f64) -> f64 {
    let mut guess = 1.0;
    let mut step = 0;
    while step < 40 {
        guess = 0.5 * (guess + value / guess);
        step += 1;
    }
    guess
}

const fn gradient_table() -> [[f64; 3]; COUNT] {
    let mut table = [[0.0; 3]; COUNT];
    let range: i64 = 32768;
    let mut base = 0;
    let mut counter: u64 = 0;
    while base < BASES {
        counter += 1;
        let h = mix(SEED ^ mix(counter));
        let x = (h & 0xffff) as i64 - range;
        let y = ((h >> 16) & 0xffff) as i64 - range;
        let z = ((h >> 32) & 0xffff) as i64 - range;
        let squared = x * x + y * y + z * z;
        if squared > range * range || squared < range * range / 4 {
            continue;
        }
        let inverse = 1.0 / root(squared as f64);
        let v = [x as f64 * inverse, y as f64 * inverse, z as f64 * inverse];
        let mut turn = 0;
        while turn < 3 {
            let a = v[turn];
            let b = v[(turn + 1) % 3];
            let c = v[(turn + 2) % 3];
            let mut signs = 0;
            while signs < 8 {
                let sx = if signs & 1 == 0 { 1.0 } else { -1.0 };
                let sy = if signs & 2 == 0 { 1.0 } else { -1.0 };
                let sz = if signs & 4 == 0 { 1.0 } else { -1.0 };
                table[base * 24 + turn * 8 + signs] = [sx * a, sy * b, sz * c];
                signs += 1;
            }
            turn += 1;
        }
        base += 1;
    }
    table
}

static TABLE: [[f64; 3]; COUNT] = gradient_table();

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

fn axis(cell: f64, multiplier: u64) -> u64 {
    (cell as i64 as u64).wrapping_mul(multiplier)
}

fn corner(hx: u64, hy: u64, hz: u64, dx: f64, dy: f64, dz: f64) -> f64 {
    let h = mix(SEED.wrapping_add(hx).wrapping_add(hy).wrapping_add(hz));
    let g = &TABLE[(((h >> 32) * COUNT as u64) >> 32) as usize];
    g[0] * dx + g[1] * dy + g[2] * dz
}

pub fn perlin(p: [f64; 3]) -> f64 {
    let fx = p[0].floor();
    let fy = p[1].floor();
    let fz = p[2].floor();
    let x = p[0] - fx;
    let y = p[1] - fy;
    let z = p[2] - fz;
    let x0 = axis(fx, AXIS_X);
    let y0 = axis(fy, AXIS_Y);
    let z0 = axis(fz, AXIS_Z);
    let x1 = x0.wrapping_add(AXIS_X);
    let y1 = y0.wrapping_add(AXIS_Y);
    let z1 = z0.wrapping_add(AXIS_Z);
    let u = fade(x);
    let v = fade(y);
    let w = fade(z);
    let xm = x - 1.0;
    let ym = y - 1.0;
    let zm = z - 1.0;
    let near = lerp(
        v,
        lerp(u, corner(x0, y0, z0, x, y, z), corner(x1, y0, z0, xm, y, z)),
        lerp(
            u,
            corner(x0, y1, z0, x, ym, z),
            corner(x1, y1, z0, xm, ym, z),
        ),
    );
    let far = lerp(
        v,
        lerp(
            u,
            corner(x0, y0, z1, x, y, zm),
            corner(x1, y0, z1, xm, y, zm),
        ),
        lerp(
            u,
            corner(x0, y1, z1, x, ym, zm),
            corner(x1, y1, z1, xm, ym, zm),
        ),
    );
    SCALE * lerp(w, near, far)
}

pub fn fractal(p: [f64; 3], octaves: u32) -> f64 {
    let mut total = 0.0;
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    for _ in 0..octaves {
        total += amplitude * perlin([p[0] * frequency, p[1] * frequency, p[2] * frequency]);
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(count: usize, seed: u64, span: f64) -> impl Iterator<Item = [f64; 3]> {
        (0..count as u64).map(move |i| {
            let h = |k: u64| (mix(seed ^ mix(i * 3 + k)) >> 11) as f64 / (1u64 << 53) as f64;
            [
                h(0) * span - span / 2.0,
                h(1) * span - span / 2.0,
                h(2) * span - span / 2.0,
            ]
        })
    }

    fn moments(values: impl Iterator<Item = f64>) -> (f64, f64, f64, f64) {
        let mut n = 0.0;
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        let mut low = f64::MAX;
        let mut high = f64::MIN;
        for v in values {
            n += 1.0;
            sum += v;
            sum2 += v * v;
            low = low.min(v);
            high = high.max(v);
        }
        let mean = sum / n;
        (mean, (sum2 / n - mean * mean).sqrt(), low, high)
    }

    #[test]
    fn the_gradients_are_unit_and_come_in_opposite_pairs() {
        for g in &TABLE {
            assert!(((g[0] * g[0] + g[1] * g[1] + g[2] * g[2]) - 1.0).abs() < 1e-12);
            assert!(
                TABLE
                    .iter()
                    .any(|h| h[0] == -g[0] && h[1] == -g[1] && h[2] == -g[2])
            );
        }
    }

    #[test]
    fn the_gradients_point_every_way() {
        let mut octants = [0usize; 8];
        for g in &TABLE {
            let o =
                (g[0] > 0.0) as usize | ((g[1] > 0.0) as usize) << 1 | ((g[2] > 0.0) as usize) << 2;
            octants[o] += 1;
        }
        assert!(octants.iter().all(|c| *c == COUNT / 8), "{octants:?}");
        let mean = TABLE
            .iter()
            .fold([0.0; 3], |a, g| [a[0] + g[0], a[1] + g[1], a[2] + g[2]]);
        assert!(mean.iter().all(|m| m.abs() < 1e-9));
        let second =
            |a: usize, b: usize| TABLE.iter().map(|g| g[a] * g[b]).sum::<f64>() / COUNT as f64;
        for a in 0..3 {
            assert!((second(a, a) - 1.0 / 3.0).abs() < 1e-9, "axis {a}");
            for b in a + 1..3 {
                assert!(second(a, b).abs() < 1e-9, "pair {a} {b}");
            }
        }
    }

    #[test]
    fn every_gradient_is_picked() {
        let mut seen = vec![false; COUNT];
        for [x, y, z] in sample(40_000, 29, 300.0) {
            let h = mix(SEED
                .wrapping_add(axis(x.floor(), AXIS_X))
                .wrapping_add(axis(y.floor(), AXIS_Y))
                .wrapping_add(axis(z.floor(), AXIS_Z)));
            seen[(((h >> 32) * COUNT as u64) >> 32) as usize] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn the_same_point_gives_the_same_value_every_time() {
        let a: Vec<f64> = sample(1000, 7, 200.0).map(perlin).collect();
        let b: Vec<f64> = sample(1000, 7, 200.0).map(perlin).collect();
        assert_eq!(a, b);
        assert_eq!(
            perlin([0.3, 0.7, 0.1]).to_bits(),
            perlin([0.3, 0.7, 0.1]).to_bits()
        );
    }

    #[test]
    fn lattice_points_are_zero() {
        for [x, y, z] in sample(2000, 3, 400.0) {
            assert_eq!(perlin([x.floor(), y.floor(), z.floor()]), 0.0);
        }
        assert_eq!(perlin([0.0, 0.0, 0.0]), 0.0);
        assert_eq!(perlin([-5.0, 17.0, 3.0]), 0.0);
    }

    #[test]
    fn small_steps_give_small_changes() {
        let step = 1e-4;
        let mut worst: f64 = 0.0;
        for p in sample(20000, 5, 100.0) {
            let v = perlin(p);
            for axis in 0..3 {
                let mut q = p;
                q[axis] += step;
                worst = worst.max((perlin(q) - v).abs());
            }
        }
        assert!(worst < 5.0 * step * 3.0, "{worst}");
    }

    #[test]
    fn it_is_continuous_across_cell_walls() {
        let eps = 1e-9;
        for p in sample(2000, 9, 60.0) {
            for axis in 0..3 {
                let mut a = p;
                let mut b = p;
                a[axis] = a[axis].floor() + 1.0 - eps;
                b[axis] = b[axis].floor() + 1.0 + eps;
                assert!((perlin(a) - perlin(b)).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn the_mean_and_spread_over_a_million_points_match_blenders() {
        let (mean, spread, low, high) = moments(sample(1_000_000, 11, 200.0).map(perlin));
        assert!(mean.abs() < 0.002, "mean {mean}");
        assert!(
            (spread - BLENDER_SPREAD).abs() < 0.2 * BLENDER_SPREAD,
            "spread {spread}"
        );
        assert!(low > -1.1 && high < 1.1, "range {low} {high}");
        assert!(low < -0.6 && high > 0.6, "range {low} {high}");
    }

    const BLENDER_SPREAD: f64 = 0.2778;

    #[test]
    fn no_axis_is_special() {
        let n = 400_000;
        let step = 0.25;
        let mut corr = [0.0; 3];
        let mut var = 0.0;
        for p in sample(n, 13, 200.0) {
            let v = perlin(p);
            var += v * v;
            for (axis, c) in corr.iter_mut().enumerate() {
                let mut q = p;
                q[axis] += step;
                *c += v * perlin(q);
            }
        }
        let r: Vec<f64> = corr.iter().map(|c| c / var).collect();
        let spread =
            r.iter().cloned().fold(f64::MIN, f64::max) - r.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread < 0.01, "{r:?}");
        assert!(r.iter().all(|c| *c > 0.5 && *c < 0.99), "{r:?}");
    }

    #[test]
    fn the_slope_is_the_same_in_every_direction() {
        let h = 1e-5;
        let root3 = 3f64.sqrt();
        let directions = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0 / root3, 1.0 / root3, 1.0 / root3],
            [1.0 / root3, -1.0 / root3, 1.0 / root3],
        ];
        let slopes: Vec<f64> = directions
            .iter()
            .map(|d| {
                let sum: f64 = sample(300_000, 23, 200.0)
                    .map(|p| {
                        let q = [p[0] + d[0] * h, p[1] + d[1] * h, p[2] + d[2] * h];
                        let slope = (perlin(q) - perlin(p)) / h;
                        slope * slope
                    })
                    .sum();
                (sum / 300_000.0).sqrt()
            })
            .collect();
        let low = slopes.iter().cloned().fold(f64::MAX, f64::min);
        let high = slopes.iter().cloned().fold(f64::MIN, f64::max);
        assert!(high / low < 1.05, "{slopes:?}");
    }

    #[test]
    fn the_fractal_is_the_scrolls_own_sum_over_perlin() {
        let p = [1.37, -2.51, 0.83];
        let by_hand = 0.5 * perlin(p)
            + 0.25 * perlin([p[0] * 2.03, p[1] * 2.03, p[2] * 2.03])
            + 0.125 * perlin([p[0] * 2.03 * 2.03, p[1] * 2.03 * 2.03, p[2] * 2.03 * 2.03]);
        assert!((fractal(p, 3) - by_hand).abs() < 1e-12);
        assert_eq!(fractal(p, 0), 0.0);
        assert_eq!(fractal(p, 1), 0.5 * perlin(p));
    }

    #[test]
    fn the_fractal_spread_is_near_blenders_four_octave_figure() {
        let (mean, spread, _, _) = moments(sample(200_000, 17, 200.0).map(|p| fractal(p, 4)));
        assert!(mean.abs() < 0.003, "mean {mean}");
        assert!((spread - 0.1596).abs() < 0.2 * 0.1596, "spread {spread}");
    }

    #[test]
    fn negative_and_huge_coordinates_work() {
        for p in [
            [-0.5, -0.5, -0.5],
            [-1e6 + 0.25, 3e6 + 0.5, -7e5 + 0.125],
            [1e9 + 0.5, 0.5, 0.5],
        ] {
            let v = perlin(p);
            assert!(v.is_finite() && v.abs() <= 1.2, "{p:?} {v}");
        }
    }

    #[test]
    #[ignore = "release: cargo test --release -p manfred-generators noise -- --ignored"]
    fn it_runs_at_twenty_million_calls_a_second() {
        let points: Vec<[f64; 3]> = sample(1 << 16, 19, 300.0).collect();
        let (rounds, floor) = if cfg!(debug_assertions) {
            (4, 0.0)
        } else {
            (400, 20e6)
        };
        let start = std::time::Instant::now();
        let mut sum = 0.0;
        for _ in 0..rounds {
            for p in &points {
                sum += perlin(std::hint::black_box(*p));
            }
        }
        let seconds = start.elapsed().as_secs_f64();
        let rate = (rounds * points.len()) as f64 / seconds;
        eprintln!("{:.1} million calls a second ({sum})", rate / 1e6);
        assert!(rate >= floor, "{rate}");
    }

    #[test]
    fn it_writes_a_slice_at_a_few_frequencies() {
        let size = 512usize;
        let frequencies = [4.0, 16.0, 64.0, 260.0];
        let mut pixels = vec![0u8; size * size * frequencies.len() * 3];
        let width = size * frequencies.len();
        for (tile, frequency) in frequencies.iter().enumerate() {
            for row in 0..size {
                for col in 0..size {
                    let u = col as f64 / size as f64 * frequency;
                    let v = row as f64 / size as f64 * frequency;
                    let value = (0.5 + 0.5 * perlin([u, v, 0.5])).clamp(0.0, 1.0);
                    let grey = (value * 255.0).round() as u8;
                    let at = (row * width + tile * size + col) * 3;
                    pixels[at..at + 3].fill(grey);
                }
            }
        }
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join("noise.png")).unwrap();
        let mut encoder =
            png::Encoder::new(std::io::BufWriter::new(file), width as u32, size as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
}
