use std::ops::Range;

use glam::DVec3;

const ALPHA: f64 = 0.5;
const BAKE_M: f64 = 0.1;
const MIN_DEPTH: u32 = 3;
const MAX_DEPTH: u32 = 12;
const SAME_M: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub position: DVec3,
    pub tangent: DVec3,
    pub right: DVec3,
    pub up: DVec3,
}

#[derive(Clone, Debug, PartialEq)]
struct Segment {
    cubic: [DVec3; 4],
    ts: Vec<f64>,
    ss: Vec<f64>,
    ps: Vec<DVec3>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    points: Vec<[f64; 3]>,
    closed: bool,
    segments: Vec<Segment>,
    starts: Vec<f64>,
    length: f64,
}

fn check(points: &[[f64; 3]], closed: bool) -> Result<(), String> {
    let least = if closed { 3 } else { 2 };
    if points.len() < least {
        let what = if closed { "a closed route" } else { "a route" };
        return Err(format!(
            "{what} needs at least {least} points, found {}",
            points.len()
        ));
    }
    if let Some(i) = points.iter().position(|p| p.iter().any(|c| !c.is_finite())) {
        return Err(format!("point {i} is not a finite [x, y, z]"));
    }
    let pairs = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    for i in 0..pairs {
        let j = (i + 1) % points.len();
        if DVec3::from_array(points[j]).distance(DVec3::from_array(points[i])) < SAME_M {
            return Err(format!(
                "points {i} and {j} are the same place; neighbouring points must differ"
            ));
        }
    }
    Ok(())
}

fn hermite(p0: DVec3, p1: DVec3, p2: DVec3, p3: DVec3) -> [DVec3; 4] {
    let d0 = libm::pow(p0.distance(p1), ALPHA);
    let d1 = libm::pow(p1.distance(p2), ALPHA);
    let d2 = libm::pow(p2.distance(p3), ALPHA);
    let m1 = ((p1 - p0) / d0 - (p2 - p0) / (d0 + d1) + (p2 - p1) / d1) * d1;
    let m2 = ((p2 - p1) / d1 - (p3 - p1) / (d1 + d2) + (p3 - p2) / d2) * d1;
    let a = 2.0 * (p1 - p2) + m1 + m2;
    let b = 3.0 * (p2 - p1) - 2.0 * m1 - m2;
    [a, b, m1, p1]
}

fn eval(c: &[DVec3; 4], t: f64) -> (DVec3, DVec3, DVec3) {
    let [a, b, m, d] = *c;
    let p = ((a * t + b) * t + m) * t + d;
    let d1 = 3.0 * t * t * a + 2.0 * t * b + m;
    let d2 = 6.0 * t * a + 2.0 * b;
    (p, d1, d2)
}

fn even(c: &[DVec3; 4], t0: f64, t1: f64, depth: u32, out: &mut Vec<(f64, DVec3)>) {
    let beg = eval(c, t0).0;
    let end = eval(c, t1).0;
    let long = beg.distance(end) > BAKE_M || depth < MIN_DEPTH;
    if long && depth < MAX_DEPTH {
        let mid = (t0 + t1) * 0.5;
        even(c, t0, mid, depth + 1, out);
        out.push((mid, eval(c, mid).0));
        even(c, mid, t1, depth + 1, out);
    }
}

fn bake(cubic: [DVec3; 4]) -> Segment {
    let mut inner = Vec::new();
    even(&cubic, 0.0, 1.0, 0, &mut inner);
    let mut ts = Vec::with_capacity(inner.len() + 2);
    let mut ps = Vec::with_capacity(inner.len() + 2);
    ts.push(0.0);
    ps.push(cubic[3]);
    for (t, p) in inner {
        ts.push(t);
        ps.push(p);
    }
    ts.push(1.0);
    ps.push(eval(&cubic, 1.0).0);
    let mut ss = Vec::with_capacity(ps.len());
    ss.push(0.0);
    for w in ps.windows(2) {
        let last = ss[ss.len() - 1];
        ss.push(last + w[0].distance(w[1]));
    }
    Segment { cubic, ts, ss, ps }
}

impl Route {
    pub fn new(points: &[[f64; 3]], closed: bool) -> Result<Route, String> {
        check(points, closed)?;
        let mut route = Route {
            points: points.to_vec(),
            closed,
            segments: Vec::new(),
            starts: Vec::new(),
            length: 0.0,
        };
        route.segments = (0..route.segments()).map(|i| route.baked(i)).collect();
        route.sum();
        Ok(route)
    }

    pub fn with_point(&self, index: usize, point: [f64; 3]) -> Result<Route, String> {
        if index >= self.points.len() {
            return Err(format!(
                "no point {index}, the route has {}",
                self.points.len()
            ));
        }
        let mut points = self.points.clone();
        points[index] = point;
        check(&points, self.closed)?;
        let mut route = Route {
            points,
            ..self.clone()
        };
        let n = self.segments();
        for s in self.segments_of_point(index) {
            route.segments[s % n] = route.baked(s % n);
        }
        route.sum();
        Ok(route)
    }

    fn sum(&mut self) {
        self.starts.clear();
        let mut at = 0.0;
        for s in &self.segments {
            self.starts.push(at);
            at += s.ss[s.ss.len() - 1];
        }
        self.length = at;
    }

    fn point(&self, i: usize) -> DVec3 {
        DVec3::from_array(self.points[i])
    }

    fn control(&self, i: isize) -> DVec3 {
        let n = self.points.len() as isize;
        if self.closed {
            return self.point(i.rem_euclid(n) as usize);
        }
        if i < 0 {
            return 2.0 * self.point(0) - self.point(1);
        }
        if i >= n {
            let last = (n - 1) as usize;
            return 2.0 * self.point(last) - self.point(last - 1);
        }
        self.point(i as usize)
    }

    fn baked(&self, segment: usize) -> Segment {
        let i = segment as isize;
        bake(hermite(
            self.control(i - 1),
            self.control(i),
            self.control(i + 1),
            self.control(i + 2),
        ))
    }

    pub fn points(&self) -> &[[f64; 3]] {
        &self.points
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn length(&self) -> f64 {
        self.length
    }

    pub fn segments(&self) -> usize {
        if self.closed {
            self.points.len()
        } else {
            self.points.len() - 1
        }
    }

    pub fn segments_of_point(&self, point: usize) -> Range<usize> {
        let n = self.segments();
        if self.closed {
            point + n - 2..point + n + 2
        } else {
            point.saturating_sub(2)..(point + 2).min(n)
        }
    }

    pub fn segment_span(&self, segment: usize) -> (f64, f64) {
        let s = &self.segments[segment % self.segments.len()];
        let start = self.starts[segment % self.segments.len()];
        (start, start + s.ss[s.ss.len() - 1])
    }

    pub fn wrap(&self, distance: f64) -> f64 {
        if !distance.is_finite() {
            return 0.0;
        }
        if self.closed {
            distance.rem_euclid(self.length)
        } else {
            distance.clamp(0.0, self.length)
        }
    }

    pub fn segment_of(&self, distance: f64) -> usize {
        let d = self.wrap(distance);
        self.starts
            .partition_point(|&s| s <= d)
            .saturating_sub(1)
            .min(self.segments.len() - 1)
    }

    fn locate(&self, distance: f64) -> (usize, f64) {
        let segment = self.segment_of(distance);
        let s = &self.segments[segment];
        let local = self.wrap(distance) - self.starts[segment];
        let j =
            s.ss.partition_point(|&x| x <= local)
                .saturating_sub(1)
                .min(s.ss.len() - 2);
        let span = s.ss[j + 1] - s.ss[j];
        let frac = if span > 0.0 {
            ((local - s.ss[j]) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (segment, s.ts[j] + frac * (s.ts[j + 1] - s.ts[j]))
    }

    fn derivatives(&self, distance: f64) -> (usize, DVec3, DVec3, DVec3) {
        let (segment, t) = self.locate(distance);
        let (p, d1, d2) = eval(&self.segments[segment].cubic, t);
        (segment, p, d1, d2)
    }

    pub fn at(&self, distance: f64) -> Frame {
        let (segment, position, d1, _) = self.derivatives(distance);
        let tangent = d1.try_normalize().unwrap_or_else(|| {
            let b = (segment + 1) % self.points.len();
            (self.point(b) - self.point(segment))
                .try_normalize()
                .unwrap_or(DVec3::Z)
        });
        let right = tangent
            .cross(DVec3::Y)
            .try_normalize()
            .or_else(|| tangent.cross(DVec3::Z).try_normalize())
            .unwrap_or(DVec3::X);
        let up = right.cross(tangent);
        Frame {
            position,
            tangent,
            right,
            up,
        }
    }

    pub fn curvature(&self, distance: f64) -> f64 {
        let (_, _, d1, d2) = self.derivatives(distance);
        let speed = d1.length_squared();
        if speed <= 1e-18 {
            return 0.0;
        }
        d2.dot(self.at(distance).right) / speed
    }

    pub fn nearest(&self, point: DVec3) -> (f64, usize) {
        let mut best = (f64::INFINITY, 0.0, 0);
        for (i, s) in self.segments.iter().enumerate() {
            for j in 0..s.ps.len() - 1 {
                let span = s.ss[j + 1] - s.ss[j];
                if span <= 0.0 {
                    continue;
                }
                let along = (s.ps[j + 1] - s.ps[j]) / span;
                let d = (point - s.ps[j]).dot(along).clamp(0.0, span);
                let far = point.distance_squared(s.ps[j] + along * d);
                if far < best.0 {
                    best = (far, self.starts[i] + s.ss[j] + d, i);
                }
            }
        }
        (best.1, best.2)
    }

    pub fn sample(&self, step: f64) -> Vec<(f64, Frame)> {
        let n = if step.is_finite() && step > 0.0 {
            ((self.length / step).ceil() as usize).clamp(1, 1 << 24)
        } else {
            1
        };
        (0..=n)
            .map(|k| {
                let d = self.length * k as f64 / n as f64;
                (d, self.at(d))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn a_straight_route_is_as_long_as_its_line_and_faces_along_it() {
        let route = Route::new(
            &[[0.0, 0.0, 0.0], [0.0, 0.0, 10.0], [0.0, 0.0, 30.0]],
            false,
        )
        .unwrap();
        assert!(close(route.length(), 30.0, 1e-9), "{}", route.length());
        let f = route.at(12.5);
        assert!(close(f.position.z, 12.5, 1e-3), "{:?}", f.position);
        assert_eq!(f.tangent.round(), DVec3::Z);
        assert_eq!(f.right.round(), -DVec3::X);
        assert_eq!(f.up.round(), DVec3::Y);
        assert_eq!(route.at(-5.0).position, route.at(0.0).position);
        assert_eq!(route.at(99.0).position, route.at(30.0).position);
        assert!(close(route.curvature(15.0), 0.0, 1e-9));
        let up = Route::new(&[[0.0, 0.0, 0.0], [0.0, 5.0, 0.0]], false).unwrap();
        let f = up.at(2.0);
        assert!(f.right.is_finite() && f.up.is_finite());
        assert!(close(f.right.dot(f.tangent), 0.0, 1e-9));
    }

    #[test]
    fn a_closed_ring_is_about_its_circle_and_wraps() {
        let n = 16;
        let r = 50.0;
        let points: Vec<[f64; 3]> = (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                [r * a.cos(), 0.0, r * a.sin()]
            })
            .collect();
        let ring = Route::new(&points, true).unwrap();
        let circle = std::f64::consts::TAU * r;
        assert!(
            close(ring.length(), circle, circle * 0.002),
            "{} vs {circle}",
            ring.length()
        );
        assert_eq!(ring.at(ring.length() + 3.0), ring.at(3.0));
        let f = ring.at(40.0);
        let k = ring.curvature(40.0);
        assert!(close(k.abs(), 1.0 / r, 0.002), "{k}");
        assert_eq!(k.signum(), (-f.position).dot(f.right).signum());
        let (d, s) = ring.nearest(DVec3::new(0.0, 0.0, 60.0));
        assert!(close(d, circle / 4.0, 0.5), "{d}");
        let (a, b) = ring.segment_span(s);
        assert!(a - 1e-9 <= d && d <= b + 1e-9, "{d} in {a}..{b}");
        let moved = ring.with_point(0, [52.0, 1.0, 0.0]).unwrap();
        assert_eq!(moved, Route::new(moved.points(), true).unwrap());
    }

    #[test]
    fn frames_move_continuously_across_segments() {
        let route = Route::new(
            &[
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 20.0],
                [-5.0, 2.0, 40.0],
                [30.0, 0.0, 55.0],
                [31.0, 0.0, 90.0],
            ],
            false,
        )
        .unwrap();
        let step = 0.01;
        let mut last = route.at(0.0);
        let mut d = step;
        while d <= route.length() {
            let f = route.at(d);
            assert!(f.position.distance(last.position) < step * 1.05, "{d}");
            assert!(f.tangent.dot(last.tangent) > 0.999, "{d}");
            last = f;
            d += step;
        }
        for s in 0..route.segments() {
            let (a, _) = route.segment_span(s);
            let p = DVec3::from_array(route.points()[s]);
            assert!(route.at(a).position.distance(p) < 1e-6, "{s}");
        }
        let moved = route.with_point(2, [-4.0, 2.0, 41.0]).unwrap();
        assert_eq!(moved, Route::new(moved.points(), false).unwrap());
        let ends = route.sample(7.0);
        assert_eq!(ends[0].0, 0.0);
        assert_eq!(ends[ends.len() - 1].0, route.length());
    }

    #[test]
    fn bad_points_are_refused() {
        assert!(Route::new(&[[0.0; 3]], false).is_err());
        assert!(Route::new(&[[0.0; 3], [0.0; 3]], false).is_err());
        assert!(Route::new(&[[0.0; 3], [f64::NAN, 0.0, 0.0]], false).is_err());
        assert!(Route::new(&[[0.0; 3], [1.0, 0.0, 0.0]], true).is_err());
        assert!(Route::new(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0; 3]], true).is_err());
    }
}
