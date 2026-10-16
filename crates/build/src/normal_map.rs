use manfred_mesh::Mesh;
use pfx_trace::bvh::{Bvh, Ray, Triangle};

pub const OVERLAP_SHARE: f64 = 0.005;
const STRICT: f64 = 1e-3;
const DILATE: usize = 8;
const FLAT: [u8; 3] = [128, 128, 255];
const NOBODY: u32 = u32::MAX;
const THREADS: usize = 16;

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}

fn blend(list: [V3; 3], w: [f32; 3]) -> V3 {
    add(
        add(scale(list[0], w[0]), scale(list[1], w[1])),
        scale(list[2], w[2]),
    )
}

fn edge(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

struct Raster {
    owner: Vec<u32>,
    weights: Vec<[f32; 3]>,
}

fn raster(low: &Mesh, uvs: &[[f32; 2]], size: usize) -> Result<Raster, String> {
    let mut owner = vec![NOBODY; size * size];
    let mut weights = vec![[0.0f32; 3]; size * size];
    let mut strict = vec![false; size * size];
    let mut covered = 0usize;
    let mut overlap = 0usize;
    let side = size as f64;
    for (t, tri) in low.indices.chunks_exact(3).enumerate() {
        let uv = [0, 1, 2].map(|k| {
            let p = uvs[tri[k] as usize];
            [f64::from(p[0]) * side, f64::from(p[1]) * side]
        });
        let area = edge(uv[0], uv[1], uv[2]);
        if area.abs() < 1e-12 {
            continue;
        }
        let lo = |axis: usize| uv.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
        let hi = |axis: usize| uv.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
        let first = |v: f64| ((v - 0.5).floor().max(0.0) as usize).min(size);
        let last = |v: f64| ((v - 0.5).ceil().max(-1.0) + 1.0).min(side) as usize;
        for y in first(lo(1))..last(hi(1)) {
            for x in first(lo(0))..last(hi(0)) {
                let c = [x as f64 + 0.5, y as f64 + 0.5];
                let w0 = edge(uv[1], uv[2], c) / area;
                let w1 = edge(uv[2], uv[0], c) / area;
                let w2 = 1.0 - w0 - w1;
                let least = w0.min(w1).min(w2);
                if least < -1e-9 {
                    continue;
                }
                let at = y * size + x;
                if owner[at] == NOBODY {
                    owner[at] = t as u32;
                    weights[at] = [w0 as f32, w1 as f32, w2 as f32];
                    strict[at] = least > STRICT;
                    covered += 1;
                } else if strict[at] && least > STRICT {
                    overlap += 1;
                }
            }
        }
    }
    if covered == 0 {
        return Err("the level's TEXCOORD_0 covers no texel of the normal map".to_string());
    }
    if overlap as f64 > covered as f64 * OVERLAP_SHARE {
        return Err(format!(
            "the normal map needs a level whose TEXCOORD_0 doesn't overlap, and {overlap} of the {covered} texels it covers lie under two triangles"
        ));
    }
    Ok(Raster { owner, weights })
}

struct Master<'a> {
    mesh: &'a Mesh,
    triangles: Vec<Triangle>,
    bvh: Bvh,
    smooth: bool,
}

impl Master<'_> {
    fn normal(&self, index: usize, point: V3, fallback: V3) -> V3 {
        let tri = &self.mesh.indices[index * 3..index * 3 + 3];
        let p = [0, 1, 2].map(|k| self.mesh.positions[tri[k] as usize]);
        let face = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        if !self.smooth {
            let n = normalize(face);
            return if dot(n, fallback) < 0.0 {
                scale(n, -1.0)
            } else {
                n
            };
        }
        let whole = dot(face, face);
        if whole <= 0.0 {
            return fallback;
        }
        let w1 = dot(cross(sub(point, p[0]), sub(p[2], p[0])), face) / whole;
        let w2 = dot(cross(sub(p[1], p[0]), sub(point, p[0])), face) / whole;
        let w = [1.0 - w1 - w2, w1, w2];
        normalize(blend(
            [0, 1, 2].map(|k| self.mesh.normals[tri[k] as usize]),
            w,
        ))
    }
}

fn texel(
    master: &Master,
    low: &Mesh,
    tangents: &[[f32; 4]],
    triangle: usize,
    w: [f32; 3],
    distance: f32,
) -> [u8; 3] {
    let tri = &low.indices[triangle * 3..triangle * 3 + 3];
    let at = |k: usize| tri[k] as usize;
    let p = blend([0, 1, 2].map(|k| low.positions[at(k)]), w);
    let vn = blend([0, 1, 2].map(|k| low.normals[at(k)]), w);
    let vt = blend(
        [0, 1, 2].map(|k| {
            let t = tangents[at(k)];
            [t[0], t[1], t[2]]
        }),
        w,
    );
    let sign = if (0..3).map(|k| w[k] * tangents[at(k)][3]).sum::<f32>() < 0.0 {
        -1.0
    } else {
        1.0
    };
    let n = normalize(vn);
    let vb = scale(cross(vn, vt), sign);
    let ray = Ray {
        origin: add(p, scale(n, distance)),
        direction: scale(n, -1.0),
    };
    let Some(hit) = master.bvh.intersect(&master.triangles, ray, distance * 2.0) else {
        return FLAT;
    };
    let point = add(ray.origin, scale(ray.direction, hit.distance));
    let target = master.normal(hit.index as usize, point, n);
    let det = dot(vt, cross(vb, vn));
    let local = if det.abs() > 1e-12 {
        [
            dot(cross(vb, vn), target) / det,
            dot(cross(vn, vt), target) / det,
            dot(cross(vt, vb), target) / det,
        ]
    } else {
        [
            dot(normalize(vt), target),
            dot(normalize(vb), target),
            dot(n, target),
        ]
    };
    normalize(local).map(|c| ((c * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn dilate(pixels: &mut [u8], filled: &mut [bool], size: usize) {
    for _ in 0..DILATE {
        let before = filled.to_vec();
        let mut grew = false;
        for y in 0..size {
            for x in 0..size {
                let at = y * size + x;
                if before[at] {
                    continue;
                }
                let mut sum = [0u32; 3];
                let mut count = 0u32;
                let around = [
                    (x > 0).then(|| at - 1),
                    (x + 1 < size).then(|| at + 1),
                    (y > 0).then(|| at - size),
                    (y + 1 < size).then(|| at + size),
                ];
                for n in around.into_iter().flatten() {
                    if before[n] {
                        for c in 0..3 {
                            sum[c] += u32::from(pixels[n * 3 + c]);
                        }
                        count += 1;
                    }
                }
                if count == 0 {
                    continue;
                }
                for c in 0..3 {
                    let mean = (sum[c] + count / 2).checked_div(count).unwrap_or(0);
                    pixels[at * 3 + c] = mean as u8;
                }
                filled[at] = true;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    for (at, done) in filled.iter().enumerate() {
        if !done {
            pixels[at * 3..at * 3 + 3].copy_from_slice(&FLAT);
        }
    }
}

pub(crate) fn bake(
    master: &Mesh,
    low: &Mesh,
    tangents: &[[f32; 4]],
    size: u32,
    distance: f32,
) -> Result<Vec<u8>, String> {
    let n = low.positions.len();
    let uvs = low
        .uvs
        .as_ref()
        .filter(|u| u.len() == n)
        .ok_or("the level has no TEXCOORD_0")?;
    if low.normals.len() != n || tangents.len() != n {
        return Err("the level has no normals".to_string());
    }
    let side = size as usize;
    let raster = raster(low, uvs, side)?;
    let triangles: Vec<Triangle> = master
        .indices
        .chunks_exact(3)
        .map(|t| Triangle {
            vertices: [0, 1, 2].map(|k| master.positions[t[k] as usize]),
            material: 0,
        })
        .collect();
    let bvh = Bvh::build(&triangles);
    let master = Master {
        mesh: master,
        triangles,
        bvh,
        smooth: master.normals.len() == master.positions.len(),
    };
    let mut pixels = vec![0u8; side * side * 3];
    let mut filled = vec![false; side * side];
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, THREADS);
    let rows = side.div_ceil(threads);
    std::thread::scope(|scope| {
        for (chunk, (px, fl)) in pixels
            .chunks_mut(rows * side * 3)
            .zip(filled.chunks_mut(rows * side))
            .enumerate()
        {
            let master = &master;
            let raster = &raster;
            scope.spawn(move || {
                let first = chunk * rows * side;
                for (i, done) in fl.iter_mut().enumerate() {
                    let at = first + i;
                    let owner = raster.owner[at];
                    if owner == NOBODY {
                        continue;
                    }
                    let rgb = texel(
                        master,
                        low,
                        tangents,
                        owner as usize,
                        raster.weights[at],
                        distance,
                    );
                    px[i * 3..i * 3 + 3].copy_from_slice(&rgb);
                    *done = true;
                }
            });
        }
    });
    dilate(&mut pixels, &mut filled, side);
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, size, size);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&pixels)
        .map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cells: u32, z: f32) -> Mesh {
        let mut m = Mesh {
            uvs: Some(Vec::new()),
            ..Mesh::default()
        };
        let side = cells + 1;
        for y in 0..side {
            for x in 0..side {
                let (u, v) = (x as f32 / cells as f32, y as f32 / cells as f32);
                m.positions.push([u, 1.0 - v, z]);
                m.normals.push([0.0, 0.0, 1.0]);
                m.uvs.as_mut().unwrap().push([u, v]);
            }
        }
        for y in 0..cells {
            for x in 0..cells {
                let a = y * side + x;
                m.indices
                    .extend([a, a + side, a + 1, a + 1, a + side, a + side + 1]);
            }
        }
        m
    }

    #[test]
    fn a_flat_master_bakes_a_flat_map_onto_a_flat_level_and_the_bytes_repeat() {
        let master = grid(24, 0.0);
        let low = grid(2, 0.0);
        let spaced = manfred_mesh::tangents(&low).unwrap();
        let png = bake(&master, &spaced.mesh, &spaced.tangents, 32, 0.05).unwrap();
        let again = bake(&master, &spaced.mesh, &spaced.tangents, 32, 0.05).unwrap();
        assert_eq!(png, again);
        let decoder = png::Decoder::new(std::io::Cursor::new(&png));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        assert_eq!(buf.len(), 32 * 32 * 3);
        for texel in buf.chunks_exact(3) {
            assert_eq!(texel, FLAT, "every texel faces straight out");
        }
    }

    #[test]
    fn overlapping_uvs_are_refused() {
        let mut low = grid(2, 0.0);
        low.append(&grid(2, 0.5));
        let spaced = manfred_mesh::tangents(&low).unwrap();
        let e = bake(&low, &spaced.mesh, &spaced.tangents, 32, 0.05).unwrap_err();
        assert!(e.contains("overlap"), "{e}");
    }
}
