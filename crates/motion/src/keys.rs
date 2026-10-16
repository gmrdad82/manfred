#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interp {
    #[default]
    Linear,
    Smooth,
    Step,
}

impl Interp {
    pub const NAMES: [(&'static str, Interp); 3] = [
        ("linear", Interp::Linear),
        ("smooth", Interp::Smooth),
        ("step", Interp::Step),
    ];

    pub fn name(self) -> &'static str {
        match self {
            Interp::Linear => "linear",
            Interp::Smooth => "smooth",
            Interp::Step => "step",
        }
    }

    pub fn of(name: &str) -> Option<Interp> {
        Interp::NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, interp)| *interp)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Channel {
    pub keys: Vec<(f64, Vec<f64>)>,
    pub interp: Interp,
}

impl Channel {
    pub fn at(&self, time: f64) -> Vec<f64> {
        sample_keys(&self.keys, self.interp, time)
    }

    pub fn number(&self, time: f64) -> f64 {
        self.at(time).first().copied().unwrap_or(0.0)
    }

    pub fn vec2(&self, time: f64) -> [f64; 2] {
        let v = self.at(time);
        [0, 1].map(|i| v.get(i).copied().unwrap_or(0.0))
    }

    pub fn vec3(&self, time: f64) -> [f64; 3] {
        let v = self.at(time);
        [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0.0))
    }

    pub fn linear(&self, step: f64) -> Vec<(f64, Vec<f64>)> {
        match self.interp {
            Interp::Linear => self.keys.clone(),
            _ if self.keys.len() < 2 => self.keys.clone(),
            Interp::Step => {
                let gap = (step * 0.5).min(STEP_GAP);
                let mut out: Vec<(f64, Vec<f64>)> = Vec::with_capacity(self.keys.len() * 2);
                for (i, (t, value)) in self.keys.iter().enumerate() {
                    if i > 0 {
                        let before = t - gap;
                        if out.last().is_none_or(|(last, _)| before > *last) {
                            out.push((before, self.keys[i - 1].1.clone()));
                        }
                    }
                    out.push((*t, value.clone()));
                }
                out
            }
            Interp::Smooth => {
                let first = self.keys[0].0;
                let last = self.keys[self.keys.len() - 1].0.max(first);
                let step = if step.is_finite() && step > 0.0 {
                    step
                } else {
                    last - first
                };
                let count = (((last - first) / step).ceil() as usize).clamp(1, 1 << 20);
                (0..=count)
                    .map(|n| {
                        let t = if n == count {
                            last
                        } else {
                            first + step * n as f64
                        };
                        (t, self.at(t))
                    })
                    .collect()
            }
        }
    }
}

const STEP_GAP: f64 = 1e-3;

pub fn sample_keys(keys: &[(f64, Vec<f64>)], interp: Interp, time: f64) -> Vec<f64> {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return Vec::new();
    };
    if !time.is_finite() || time <= first.0 {
        return first.1.clone();
    }
    if time >= last.0 {
        return last.1.clone();
    }
    let i = keys.partition_point(|k| k.0 <= time) - 1;
    let ((t0, a), (t1, b)) = (&keys[i], &keys[i + 1]);
    let span = t1 - t0;
    if span <= 0.0 {
        return b.clone();
    }
    let u = (time - t0) / span;
    match interp {
        Interp::Step => a.clone(),
        Interp::Linear => a.iter().zip(b).map(|(a, b)| a + (b - a) * u).collect(),
        Interp::Smooth => {
            let end = keys.len() - 1;
            let slope = |j: usize, c: usize| {
                if j == 0 || j == end {
                    0.0
                } else {
                    let (before, after) = (&keys[j - 1], &keys[j + 1]);
                    (after.1[c] - before.1[c]) / (after.0 - before.0)
                }
            };
            let (u2, u3) = (u * u, u * u * u);
            (0..a.len().min(b.len()))
                .map(|c| {
                    (2.0 * u3 - 3.0 * u2 + 1.0) * a[c]
                        + (u3 - 2.0 * u2 + u) * span * slope(i, c)
                        + (3.0 * u2 - 2.0 * u3) * b[c]
                        + (u3 - u2) * span * slope(i + 1, c)
                })
                .collect()
        }
    }
}
