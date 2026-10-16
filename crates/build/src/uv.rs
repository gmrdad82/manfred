use crate::{Error, fail};
use glam::{DMat4, DVec3};
use manfred_mesh::math::Real;
use manfred_mesh::{Mesh, bounds, triangle_normal};
use manfred_recipe::{Camera, Object, Projection, Recipe, Uv};
use std::collections::BTreeMap;

const SENTINEL: [f32; 2] = [0.0005, 1.0 - 0.9995];

struct Eye {
    position: DVec3,
    right: DVec3,
    up: DVec3,
    forward: DVec3,
    half: [f64; 2],
    shift: [f64; 2],
    ortho: bool,
}

impl Eye {
    fn new(camera: &Camera, aspect: f64) -> Eye {
        let position = DVec3::from(camera.position);
        let forward = (DVec3::from(camera.target) - position)
            .try_normalize()
            .unwrap_or(DVec3::NEG_Z);
        let right = forward
            .cross(DVec3::from(camera.up))
            .try_normalize()
            .unwrap_or_else(|| forward.any_orthonormal_vector());
        let up = right.cross(forward);
        let (half_y, ortho) = match camera.kind {
            Projection::Orthographic => {
                ((camera.ortho_height.unwrap_or(1.0) * 0.5).max(1e-9), true)
            }
            Projection::Perspective => (
                (camera.fov_y_deg(aspect).to_radians() * 0.5)
                    .tan_of()
                    .max(1e-9),
                false,
            ),
        };
        Eye {
            position,
            right,
            up,
            forward,
            half: [half_y * aspect, half_y],
            shift: camera.shift,
            ortho,
        }
    }

    fn facing(&self, normal: DVec3, centre: DVec3) -> bool {
        let toward = if self.ortho {
            -self.forward
        } else {
            self.position - centre
        };
        normal.dot(toward) > 0.0
    }

    fn project(&self, p: DVec3) -> Option<[f64; 2]> {
        let v = p - self.position;
        let depth = if self.ortho { 1.0 } else { v.dot(self.forward) };
        if depth <= 1e-6 {
            return None;
        }
        let x = v.dot(self.right) / (depth * self.half[0]) - self.shift[0];
        let y = v.dot(self.up) / (depth * self.half[1]) - self.shift[1];
        Some([(x + 1.0) * 0.5, (1.0 - y) * 0.5])
    }
}

fn eye(recipe: &Recipe, object: &Object) -> Result<Eye, Error> {
    let Some(plate) = &recipe.plate else {
        return Err(fail(
            object,
            "uv",
            "uv \"plate\" projects onto the plate, so the recipe needs a `[plate]`",
        ));
    };
    let camera = match &plate.camera {
        Some(name) => recipe.cameras.iter().find(|c| &c.name == name),
        None => recipe.cameras.first(),
    };
    let Some(camera) = camera else {
        return Err(fail(
            object,
            "uv",
            "uv \"plate\" projects through the plate's camera, and the recipe has none",
        ));
    };
    Ok(Eye::new(camera, plate.width / plate.height.max(1e-9)))
}

fn normal(tri: &[DVec3; 3], flip: bool) -> DVec3 {
    let n = triangle_normal(tri);
    if flip { -n } else { n }
}

fn centre(tri: &[DVec3; 3]) -> DVec3 {
    (tri[0] + tri[1] + tri[2]) / 3.0
}

fn atlas(atlas: [f64; 4], s: f64, t: f64) -> [f32; 2] {
    [
        (atlas[0] + (atlas[2] - atlas[0]) * s) as f32,
        (atlas[1] + (atlas[3] - atlas[1]) * t) as f32,
    ]
}

fn away(away: [f64; 2]) -> [f32; 2] {
    away.map(|x| x as f32)
}

fn plate(m: &Mesh, eye: &Eye, matrix: DMat4) -> Vec<[f32; 2]> {
    let flip = matrix.determinant() < 0.0;
    let mut out = Vec::with_capacity(m.indices.len());
    for tri in m.world_triangles(matrix) {
        let facing = eye.facing(normal(&tri, flip), centre(&tri));
        for p in tri {
            let uv = if facing { eye.project(p) } else { None };
            out.push(uv.map_or(SENTINEL, |[u, v]| [u as f32, v as f32]));
        }
    }
    out
}

fn rect(
    m: &Mesh,
    matrix: DMat4,
    [x0, y0, x1, y1]: [f64; 4],
    map: [f64; 4],
    facing: f64,
    clamp: bool,
    back: [f64; 2],
) -> Vec<[f32; 2]> {
    let flip = matrix.determinant() < 0.0;
    let (w, h) = ((x1 - x0).max(1e-12), (y1 - y0).max(1e-12));
    let mut out = Vec::with_capacity(m.indices.len());
    for tri in m.world_triangles(matrix) {
        let front = normal(&tri, flip).z > facing;
        for p in tri {
            if !front {
                out.push(away(back));
                continue;
            }
            let (mut s, mut t) = ((p.x - x0) / w, (y1 - p.y) / h);
            if clamp {
                (s, t) = (s.clamp(0.0, 1.0), t.clamp(0.0, 1.0));
            }
            out.push(atlas(map, s, t));
        }
    }
    out
}

fn cylinder(
    m: &Mesh,
    degrees: f64,
    map: [f64; 4],
    clamped: Option<[f64; 2]>,
    wall: Option<[f64; 2]>,
    back: [f64; 2],
) -> Vec<[f32; 2]> {
    let band = clamped.unwrap_or_else(|| bounds(m).map_or([0.0, 1.0], |[lo, hi]| [lo[1], hi[1]]));
    let height = (band[1] - band[0]).max(1e-12);
    let mut out = Vec::with_capacity(m.indices.len());
    for tri in m.world_triangles(DMat4::IDENTITY) {
        let outer = wall.is_none_or(|[lo, hi]| {
            let (n, o) = (normal(&tri, false), centre(&tri));
            n.x * o.x + n.z * o.z > 0.0 && lo < o.y && o.y < hi
        });
        for p in tri {
            if !outer {
                out.push(away(back));
                continue;
            }
            let theta = p.x.atan2_of(p.z).to_degrees();
            let s = ((theta - degrees) / 360.0 + 0.5).rem_euclid(1.0);
            let t = (band[1] - p.y) / height;
            let t = if clamped.is_some() {
                t.clamp(0.0, 1.0)
            } else {
                t
            };
            out.push(atlas(map, s, t));
        }
    }
    out
}

fn assign(m: &mut Mesh, corner: &[[f32; 2]]) -> Vec<u32> {
    let mut sources = Vec::new();
    let mut seen = BTreeMap::<(u32, [u32; 2]), u32>::new();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut uvs1 = Vec::new();
    let mut indices = Vec::with_capacity(m.indices.len());
    for (&i, &uv) in m.indices.iter().zip(corner) {
        let id = *seen.entry((i, uv.map(f32::to_bits))).or_insert_with(|| {
            sources.push(i);
            positions.push(m.positions[i as usize]);
            normals.push(m.normals.get(i as usize).copied().unwrap_or([0.0; 3]));
            uvs.push(uv);
            if let Some(u) = &m.uvs1 {
                uvs1.push(u[i as usize]);
            }
            (positions.len() - 1) as u32
        });
        indices.push(id);
    }
    m.positions = positions;
    m.normals = normals;
    m.uvs = Some(uvs);
    if m.uvs1.is_some() {
        m.uvs1 = Some(uvs1);
    }
    m.indices = indices;
    sources
}

pub(crate) fn apply(
    recipe: &Recipe,
    object: &Object,
    matrix: DMat4,
    m: &mut Mesh,
) -> Result<Option<Vec<u32>>, Error> {
    if m.indices.is_empty() {
        return Ok(None);
    }
    let corner = match &object.uv {
        Some(Uv::Plate) => plate(m, &eye(recipe, object)?, matrix),
        &Some(Uv::Rect {
            rect: r,
            atlas,
            facing,
            clamp,
            away,
        }) => rect(m, matrix, r, atlas, facing, clamp, away),
        &Some(Uv::Cylinder {
            degrees,
            atlas,
            band,
            wall,
            away,
        }) => cylinder(m, degrees, atlas, band, wall, away),
        _ => return Ok(None),
    };
    Ok(Some(assign(m, &corner)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::parse;

    const HEAD: &str = "manfred = 1\nname = \"t\"\n[plate]\nwidth = 200\nheight = 100\nmetres_per_pixel = 0.01\norigin = \"top_left\"\n[[camera]]\nname = \"main\"\nposition = [0.0, 0.0, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 90.0\n";

    fn run(extra: &str, uv: &str, m: &mut Mesh) {
        let r = parse(&format!(
            "{HEAD}{extra}[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmaterial = \"m\"\nuv = {uv}\n"
        ))
        .unwrap();
        apply(&r, &r.objects[0], DMat4::IDENTITY, m).unwrap();
    }

    fn quad(corners: [[f32; 3]; 4]) -> Mesh {
        Mesh {
            positions: corners.to_vec(),
            normals: vec![[0.0; 3]; 4],
            uvs: None,
            uvs1: None,
            indices: vec![0, 1, 2, 0, 2, 3],
            slots: Vec::new(),
        }
    }

    fn uv_at(m: &Mesh, p: [f32; 3]) -> Vec<[f32; 2]> {
        m.positions
            .iter()
            .zip(m.uvs.as_ref().unwrap())
            .filter(|(q, _)| **q == p)
            .map(|(_, uv)| *uv)
            .collect()
    }

    fn close(a: [f32; 2], b: [f64; 2]) -> bool {
        (f64::from(a[0]) - b[0]).abs() < 1e-6 && (f64::from(a[1]) - b[1]).abs() < 1e-6
    }

    const FRONT: [[f32; 3]; 4] = [
        [-1.0, -0.5, 0.0],
        [1.0, -0.5, 0.0],
        [1.0, 0.5, 0.0],
        [-1.0, 0.5, 0.0],
    ];

    #[test]
    fn a_facing_quad_takes_its_projected_corners() {
        let mut m = quad(FRONT);
        run("", "\"plate\"", &mut m);
        assert_eq!(m.positions.len(), 4);
        assert!(close(uv_at(&m, FRONT[0])[0], [0.375, 0.625]));
        assert!(close(uv_at(&m, FRONT[1])[0], [0.625, 0.625]));
        assert!(close(uv_at(&m, FRONT[2])[0], [0.625, 0.375]));
        assert!(close(uv_at(&m, FRONT[3])[0], [0.375, 0.375]));
    }

    #[test]
    fn the_plate_camera_shift_and_name_move_the_projection() {
        let mut m = quad(FRONT);
        let extra = "[[camera]]\nname = \"shifted\"\nposition = [0.0, 0.0, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 90.0\nshift = [0.0, 0.5]\n";
        let r = parse(&format!(
            "{}camera = \"shifted\"\n{}{extra}[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmaterial = \"m\"\nuv = \"plate\"\n",
            &HEAD[..HEAD.find("[[camera]]").unwrap()],
            &HEAD[HEAD.find("[[camera]]").unwrap()..]
        ))
        .unwrap();
        apply(&r, &r.objects[0], DMat4::IDENTITY, &mut m).unwrap();
        assert!(close(uv_at(&m, FRONT[0])[0], [0.375, 0.875]));
        assert!(close(uv_at(&m, FRONT[2])[0], [0.625, 0.625]));
    }

    #[test]
    fn a_quad_turned_away_takes_the_sentinel() {
        let mut m = quad([FRONT[0], FRONT[3], FRONT[2], FRONT[1]]);
        run("", "\"plate\"", &mut m);
        assert!(m.uvs.as_ref().unwrap().iter().all(|&uv| uv == SENTINEL));
        assert_eq!(SENTINEL[1].to_bits(), (1.0f32 - 0.9995f32).to_bits());
    }

    #[test]
    fn the_object_transform_is_projected_and_a_mirror_keeps_the_facing() {
        let mut moved = quad(FRONT);
        let r = parse(&format!(
            "{HEAD}[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmaterial = \"m\"\nuv = \"plate\"\n"
        ))
        .unwrap();
        apply(
            &r,
            &r.objects[0],
            DMat4::from_translation(DVec3::new(1.0, 0.0, 0.0)),
            &mut moved,
        )
        .unwrap();
        assert!(close(uv_at(&moved, FRONT[0])[0], [0.5, 0.625]));
        let mut mirrored = quad([FRONT[0], FRONT[3], FRONT[2], FRONT[1]]);
        apply(
            &r,
            &r.objects[0],
            DMat4::from_scale(DVec3::new(1.0, 1.0, -1.0)),
            &mut mirrored,
        )
        .unwrap();
        assert!(
            mirrored
                .uvs
                .as_ref()
                .unwrap()
                .iter()
                .all(|&uv| uv != SENTINEL)
        );
    }

    #[test]
    fn a_cube_splits_its_vertices_exactly_where_faces_disagree() {
        let positions: Vec<[f32; 3]> = (0..8)
            .map(|i| {
                [
                    if i & 1 == 0 { -0.5 } else { 0.5 },
                    if i & 2 == 0 { -0.5 } else { 0.5 },
                    if i & 4 == 0 { -0.5 } else { 0.5 },
                ]
            })
            .collect();
        let faces = [
            [4, 5, 7, 6],
            [1, 0, 2, 3],
            [5, 1, 3, 7],
            [0, 4, 6, 2],
            [6, 7, 3, 2],
            [0, 1, 5, 4],
        ];
        let mut m = Mesh {
            positions: positions.clone(),
            normals: vec![[0.0; 3]; 8],
            uvs1: None,
            uvs: None,
            indices: faces
                .iter()
                .flat_map(|f| [f[0], f[1], f[2], f[0], f[2], f[3]])
                .collect(),
            slots: Vec::new(),
        };
        run("", "\"plate\"", &mut m);
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.positions.len(), 12);
        for (i, p) in positions.iter().enumerate() {
            let uvs = uv_at(&m, *p);
            if i & 4 == 0 {
                assert_eq!(uvs, [SENTINEL]);
            } else {
                assert_eq!(uvs.len(), 2);
                assert!(uvs.contains(&SENTINEL));
            }
        }
        let uvs = m.uvs.as_ref().unwrap();
        for t in m.indices[..6].chunks_exact(3) {
            assert!(t.iter().all(|&i| uvs[i as usize] != SENTINEL));
        }
        for t in m.indices[6..].chunks_exact(3) {
            assert!(t.iter().all(|&i| uvs[i as usize] == SENTINEL));
        }
    }

    #[test]
    fn a_rect_maps_its_front_into_the_atlas_and_its_back_away() {
        let mut m = quad(FRONT);
        m.append(&quad([
            [-1.0, -0.5, -0.1],
            [-1.0, 0.5, -0.1],
            [1.0, 0.5, -0.1],
            [1.0, -0.5, -0.1],
        ]));
        run(
            "",
            "{ kind = \"rect\", rect_px = [0, 0, 200, 100], atlas = [0.5, 0.25, 0.75, 0.5] }",
            &mut m,
        );
        assert_eq!(uv_at(&m, FRONT[0]), [[0.5, 0.5]]);
        assert_eq!(uv_at(&m, FRONT[2]), [[0.75, 0.25]]);
        assert_eq!(uv_at(&m, [1.0, -0.5, -0.1]), [[-1.0, -1.0]]);
    }

    #[test]
    fn a_rect_clamps_and_takes_its_facing_threshold() {
        let wide = [
            [-2.0, -0.5, 0.0],
            [2.0, -0.5, 0.0],
            [2.0, 1.5, 0.0],
            [-2.0, 1.5, 0.0],
        ];
        let mut free = quad(wide);
        run(
            "",
            "{ kind = \"rect\", rect = [-1.0, -0.5, 1.0, 0.5] }",
            &mut free,
        );
        assert_eq!(uv_at(&free, wide[2]), [[1.5, -1.0]]);
        let mut clamped = quad(wide);
        run(
            "",
            "{ kind = \"rect\", rect = [-1.0, -0.5, 1.0, 0.5], clamp = true }",
            &mut clamped,
        );
        assert_eq!(uv_at(&clamped, wide[2]), [[1.0, 0.0]]);
        let tilted = [
            [-1.0, -0.5, 0.0],
            [1.0, -0.5, 0.0],
            [1.0, 0.5, -2.0],
            [-1.0, 0.5, -2.0],
        ];
        let mut steep = quad(tilted);
        run(
            "",
            "{ kind = \"rect\", rect = [-1.0, -0.5, 1.0, 0.5], away = [-1.0, 2.0] }",
            &mut steep,
        );
        assert_eq!(uv_at(&steep, tilted[0]), [[-1.0, 2.0]]);
        let mut loose = quad(tilted);
        run(
            "",
            "{ kind = \"rect\", rect = [-1.0, -0.5, 1.0, 0.5], facing = 0.0 }",
            &mut loose,
        );
        assert_eq!(uv_at(&loose, tilted[0]), [[0.0, 1.0]]);
    }

    #[test]
    fn a_cylinder_wraps_its_outer_wall_and_sends_the_rest_away() {
        let (r, s) = (0.5f32, std::f32::consts::FRAC_1_SQRT_2 * 0.5);
        let outer = [[0.0, 0.0, r], [s, 0.0, s], [s, 1.0, s], [0.0, 1.0, r]];
        let mut m = quad(outer);
        m.append(&quad([
            [0.0, 0.0, 0.4],
            [0.0, 1.0, 0.4],
            [0.3, 1.0, 0.3],
            [0.3, 0.0, 0.3],
        ]));
        run(
            "",
            "{ kind = \"cylinder\", degrees = 90.0, atlas = [0.0, 0.0, 0.5, 0.25], band = [0.25, 0.75], wall = [0.1, 0.9] }",
            &mut m,
        );
        assert!(close(uv_at(&m, outer[0])[0], [0.125, 0.25]));
        assert!(close(uv_at(&m, outer[1])[0], [0.1875, 0.25]));
        assert!(close(uv_at(&m, outer[2])[0], [0.1875, 0.0]));
        assert_eq!(uv_at(&m, [0.3, 0.0, 0.3]), [[-1.0, -1.0]]);
        let mut low = quad(outer);
        run(
            "",
            "{ kind = \"cylinder\", degrees = 90.0, wall = [0.7, 0.9] }",
            &mut low,
        );
        assert!(
            low.uvs
                .as_ref()
                .unwrap()
                .iter()
                .all(|&uv| uv == [-1.0, -1.0])
        );
        let mut whole = quad(outer);
        run("", "{ kind = \"cylinder\", degrees = 180.0 }", &mut whole);
        assert!(close(uv_at(&whole, outer[0])[0], [0.0, 1.0]));
        assert!(close(uv_at(&whole, outer[2])[0], [0.125, 0.0]));
    }
}
