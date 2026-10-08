use egui::{Pos2, Rect};
use glam::{DMat4, DVec3};
use manfred_build::Part;
use manfred_recipe::Shadow;
use manfred_scene::Eye;

const NEAR: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub direction: DVec3,
}

impl Ray {
    pub fn at(&self, distance: f64) -> DVec3 {
        self.origin + self.direction * distance
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Words {
    pub surface: Vec<(usize, [DVec3; 4])>,
    pub overlay: Vec<(usize, Rect)>,
}

impl Words {
    pub fn laid(surface: &[(usize, [[f64; 3]; 4])]) -> Words {
        Words {
            surface: surface
                .iter()
                .map(|(object, corners)| (*object, corners.map(DVec3::from_array)))
                .collect(),
            overlay: Vec::new(),
        }
    }

    pub fn with_overlay(
        mut self,
        image: Rect,
        size: [u32; 2],
        boxes: &[(usize, [f32; 4])],
    ) -> Words {
        let scale = [
            image.width() / size[0].max(1) as f32,
            image.height() / size[1].max(1) as f32,
        ];
        self.overlay = boxes
            .iter()
            .map(|(object, [left, top, right, bottom])| {
                let corner =
                    |x: f32, y: f32| image.min + egui::Vec2::new(x * scale[0], y * scale[1]);
                (
                    *object,
                    Rect::from_min_max(corner(*left, *top), corner(*right, *bottom)),
                )
            })
            .collect();
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    image: Rect,
    position: DVec3,
    right: DVec3,
    up: DVec3,
    forward: DVec3,
    shift: [f64; 2],
    half: [f64; 2],
    ortho: bool,
}

impl Lens {
    pub fn new(eye: &Eye, image: Rect) -> Lens {
        let [right, up, forward] = eye.basis().map(DVec3::from_array);
        let aspect = f64::from(image.width().max(1.0)) / f64::from(image.height().max(1.0));
        let (half_y, ortho) = match eye.ortho_height {
            Some(height) => ((height * 0.5).max(1e-9), true),
            None => ((eye.fov_y_deg.to_radians() * 0.5).tan().max(1e-9), false),
        };
        Lens {
            image,
            position: DVec3::from_array(eye.position),
            right,
            up,
            forward,
            shift: eye.shift,
            half: [half_y * aspect, half_y],
            ortho,
        }
    }

    pub fn image(&self) -> Rect {
        self.image
    }

    pub fn depth(&self, point: DVec3) -> f64 {
        (point - self.position).dot(self.forward)
    }

    pub fn ndc(&self, point: DVec3) -> Option<[f64; 2]> {
        let offset = point - self.position;
        let depth = offset.dot(self.forward);
        let scale = if self.ortho {
            1.0
        } else if depth > NEAR {
            depth
        } else {
            return None;
        };
        Some([
            offset.dot(self.right) / (scale * self.half[0]) - self.shift[0],
            offset.dot(self.up) / (scale * self.half[1]) - self.shift[1],
        ])
    }

    pub fn project(&self, point: DVec3) -> Option<Pos2> {
        let [x, y] = self.ndc(point)?;
        let (left, top) = (f64::from(self.image.left()), f64::from(self.image.top()));
        let (width, height) = (
            f64::from(self.image.width()),
            f64::from(self.image.height()),
        );
        Some(Pos2::new(
            (left + (x + 1.0) * 0.5 * width) as f32,
            (top + (1.0 - y) * 0.5 * height) as f32,
        ))
    }

    pub fn ray(&self, at: Pos2) -> Ray {
        let x = 2.0 * f64::from(at.x - self.image.left()) / f64::from(self.image.width().max(1.0))
            - 1.0;
        let y = 1.0
            - 2.0 * f64::from(at.y - self.image.top()) / f64::from(self.image.height().max(1.0));
        let across = (x + self.shift[0]) * self.half[0];
        let rise = (y + self.shift[1]) * self.half[1];
        if self.ortho {
            Ray {
                origin: self.position + self.right * across + self.up * rise,
                direction: self.forward,
            }
        } else {
            Ray {
                origin: self.position,
                direction: (self.forward + self.right * across + self.up * rise).normalize(),
            }
        }
    }

    pub fn metres_per_point(&self, point: DVec3) -> f64 {
        let tall = if self.ortho {
            2.0 * self.half[1]
        } else {
            2.0 * self.depth(point).max(NEAR) * self.half[1]
        };
        tall / f64::from(self.image.height().max(1.0))
    }

    pub fn toward_eye(&self, point: DVec3) -> DVec3 {
        if self.ortho {
            -self.forward
        } else {
            (self.position - point)
                .try_normalize()
                .unwrap_or(-self.forward)
        }
    }
}

pub fn uv(image: Rect, at: Pos2) -> Option<[f32; 2]> {
    image.contains(at).then(|| {
        [
            (at.x - image.left()) / image.width().max(1.0),
            (at.y - image.top()) / image.height().max(1.0),
        ]
    })
}

pub fn pixel(uv: [f32; 2], size: [u32; 2]) -> [u32; 2] {
    let side = |fraction: f32, length: u32| {
        ((fraction * length as f32).floor() as u32).min(length.saturating_sub(1))
    };
    [side(uv[0], size[0]), side(uv[1], size[1])]
}

pub fn triangle(ray: &Ray, corners: [DVec3; 3]) -> Option<f64> {
    let [a, b, c] = corners;
    let ab = b - a;
    let ac = c - a;
    let p = ray.direction.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inverse = 1.0 / det;
    let t = ray.origin - a;
    let u = t.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(ab);
    let v = ray.direction.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = ac.dot(q) * inverse;
    (distance > 0.0).then_some(distance)
}

pub fn cast(parts: &[Part], words: &Words, ray: &Ray, at: Pos2) -> Option<usize> {
    if let Some((object, _)) = words.overlay.iter().rev().find(|(_, rect)| {
        (rect.min.x..rect.max.x).contains(&at.x) && (rect.min.y..rect.max.y).contains(&at.y)
    }) {
        return Some(*object);
    }
    let mut found = nearest(parts, ray);
    for (object, [a, b, c, d]) in &words.surface {
        for corners in [[*a, *b, *c], [*a, *c, *d]] {
            if let Some(distance) = triangle(ray, corners)
                && found.is_none_or(|(best, _)| distance < best)
            {
                found = Some((distance, *object));
            }
        }
    }
    found.map(|(_, object)| object)
}

fn nearest(parts: &[Part], ray: &Ray) -> Option<(f64, usize)> {
    let mut nearest: Option<(f64, usize)> = None;
    for part in parts
        .iter()
        .filter(|part| !part.hidden && part.shadow != Shadow::Only)
    {
        let node = &part.node;
        let world = if node.keep_transform {
            node.transform
        } else {
            DMat4::IDENTITY
        };
        let corner = |index: u32| {
            let p = node.mesh.positions[index as usize];
            world.transform_point3(DVec3::new(
                f64::from(p[0]),
                f64::from(p[1]),
                f64::from(p[2]),
            ))
        };
        for face in node.mesh.indices.chunks_exact(3) {
            if let Some(distance) =
                triangle(ray, [corner(face[0]), corner(face[1]), corner(face[2])])
                && nearest.is_none_or(|(best, _)| distance < best)
            {
                nearest = Some((distance, part.object));
            }
        }
    }
    nearest
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Vec2;
    use std::path::Path;

    fn eye(shift: [f64; 2]) -> Eye {
        Eye {
            position: [0.0, 0.0, 2.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 90.0,
            shift,
            ortho_height: None,
        }
    }

    fn image() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(400.0, 200.0))
    }

    fn near(a: Pos2, b: Pos2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn projection_lands_on_hand_computed_points() {
        let lens = Lens::new(&eye([0.0, 0.0]), image());
        assert!(near(
            lens.project(DVec3::ZERO).unwrap(),
            Pos2::new(300.0, 150.0)
        ));
        assert!(near(
            lens.project(DVec3::new(0.0, 2.0, 0.0)).unwrap(),
            Pos2::new(300.0, 50.0)
        ));
        assert!(near(
            lens.project(DVec3::new(2.0, 0.0, 0.0)).unwrap(),
            Pos2::new(400.0, 150.0)
        ));
        assert!(near(
            lens.project(DVec3::new(1.0, -1.0, 1.0)).unwrap(),
            Pos2::new(400.0, 250.0)
        ));
        assert_eq!(lens.project(DVec3::new(0.0, 0.0, 3.0)), None);
    }

    #[test]
    fn lens_shift_moves_the_picture_as_the_engine_does() {
        let lens = Lens::new(&eye([0.1, -0.5]), image());
        let at = lens.project(DVec3::ZERO).unwrap();
        assert!(
            near(at, Pos2::new(300.0 - 0.1 * 200.0, 150.0 - 0.5 * 100.0)),
            "{at:?}"
        );
        let up = lens.project(DVec3::new(0.0, 1.0, 0.0)).unwrap();
        assert!(near(up, Pos2::new(280.0, 50.0)), "{up:?}");
    }

    #[test]
    fn an_orthographic_lens_projects_flat() {
        let mut flat = eye([0.25, 0.0]);
        flat.ortho_height = Some(4.0);
        let lens = Lens::new(&flat, image());
        let centre = lens.project(DVec3::new(0.0, 0.0, -5.0)).unwrap();
        assert!(near(centre, Pos2::new(300.0 - 0.25 * 200.0, 150.0)));
        let corner = lens.project(DVec3::new(4.0, 2.0, 1.0)).unwrap();
        assert!(near(corner, Pos2::new(500.0 - 50.0, 50.0)), "{corner:?}");
        assert!((lens.metres_per_point(DVec3::ZERO) - 0.02).abs() < 1e-12);
    }

    #[test]
    fn a_ray_through_a_projected_point_passes_through_it() {
        for shift in [[0.0, 0.0], [0.1, -0.6395]] {
            for ortho in [None, Some(3.0)] {
                let mut view = eye(shift);
                view.position = [0.4, 0.7, 1.9];
                view.target = [0.1, 0.05, -0.2];
                view.ortho_height = ortho;
                let lens = Lens::new(&view, image());
                let point = DVec3::new(0.2, -0.1, 0.3);
                let ray = lens.ray(lens.project(point).unwrap());
                let along = (point - ray.origin).dot(ray.direction);
                assert!(
                    (ray.at(along) - point).length() < 1e-4,
                    "{shift:?} {ortho:?}"
                );
            }
        }
    }

    #[test]
    fn metres_per_point_is_the_view_height_over_the_image() {
        let lens = Lens::new(&eye([0.0, 0.0]), image());
        assert!((lens.metres_per_point(DVec3::ZERO) - 4.0 / 200.0).abs() < 1e-12);
    }

    #[test]
    fn a_uv_maps_to_the_stage_pixel() {
        assert_eq!(uv(image(), Pos2::new(300.0, 150.0)), Some([0.5, 0.5]));
        assert_eq!(uv(image(), Pos2::new(90.0, 150.0)), None);
        assert_eq!(pixel([0.5, 0.5], [800, 400]), [400, 200]);
        assert_eq!(pixel([1.0, 1.0], [800, 400]), [799, 399]);
    }

    #[test]
    fn a_cast_finds_the_nearest_object_on_the_table() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/table/table.toml");
        let loaded = manfred_recipe::load(&path).unwrap();
        let recipe = loaded.recipe;
        let built = manfred_build::build(&recipe, path.parent().unwrap()).unwrap();
        let camera = recipe.cameras.iter().find(|c| c.name == "main").unwrap();
        let lens = Lens::new(&Eye::from_camera(camera, 1.6), image());
        let index = |name: &str| recipe.objects.iter().position(|o| o.name == name);
        let ball = recipe.objects[index("ball").unwrap()].at;
        let at = lens.project(DVec3::from_array(ball)).unwrap();
        assert_eq!(
            cast(&built.parts, &Words::default(), &lens.ray(at), at),
            index("ball")
        );
        let sky = lens.ray(Pos2::new(101.0, 51.0));
        assert_ne!(
            cast(
                &built.parts,
                &Words::default(),
                &sky,
                Pos2::new(101.0, 51.0)
            ),
            index("ball")
        );
        let away = Ray {
            origin: DVec3::new(0.0, 5.0, 0.0),
            direction: DVec3::Y,
        };
        assert_eq!(
            cast(&built.parts, &Words::default(), &away, Pos2::ZERO),
            None
        );
    }

    const PANES: &str = "manfred = 1\nname = \"panes\"\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 0.25, 2.0]\ntarget = [0.0, 0.15, 0.0]\nfov_y_deg = 35.0\n\n[[object]]\nname = \"back\"\nkind = \"box\"\nmaterial = \"orange\"\nsize = [0.4, 0.4, 0.2]\nat = [-0.3, 0.2, 0.0]\n\n[[object]]\nname = \"pane\"\nkind = \"box\"\nmaterial = \"glass\"\nsize = [0.5, 0.5, 0.02]\nat = [-0.3, 0.2, 0.3]\n\n[[object]]\nname = \"side\"\nkind = \"box\"\nmaterial = \"cream\"\nsize = [0.3, 0.3, 0.2]\nat = [0.35, 0.15, 0.0]\n\n[[object]]\nname = \"label\"\nkind = \"text\"\non = \"side\"\nspans = [{ text = \"side\", size = 0.08 }]\n\n[[object]]\nname = \"title\"\nkind = \"text\"\nspace = \"overlay\"\nat = [8.0, 8.0, 0.0]\nspans = [{ text = \"panes\", size = 24.0 }]\n\n[materials.glass]\nfamily = \"glass\"\nbase = [0.9, 0.95, 1.0]\nroughness = 0.02\ntransmission = 1.0\nior = 1.5\nthickness = 0.004\n\n[materials.orange]\nfamily = \"lacquer\"\nbase = [0.73, 0.06, 0.0]\nroughness = 0.45\nclearcoat = 1.0\nclearcoat_roughness = 0.07\n\n[materials.cream]\nfamily = \"lacquer\"\nbase = [0.81, 0.58, 0.33]\nroughness = 0.5\nclearcoat = 1.0\nclearcoat_roughness = 0.1\n";

    fn toward(origin: DVec3, point: [f64; 3]) -> Ray {
        Ray {
            origin,
            direction: (DVec3::from_array(point) - origin).normalize(),
        }
    }

    #[test]
    fn a_cast_finds_glass_through_its_triangles_and_text_by_its_box() {
        let recipe = manfred_recipe::parse(PANES).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let index = |name: &str| recipe.objects.iter().position(|o| o.name == name);
        let laid = manfred_scene::Laid::of(&recipe, &built.parts).unwrap();
        let words = Words::laid(&laid.surface_boxes());
        assert_eq!(words.surface.len(), 1);
        let from = DVec3::new(0.0, 0.25, 2.0);
        let nowhere = Pos2::new(-1.0, -1.0);
        let pane = toward(from, [-0.3, 0.2, 0.0]);
        assert_eq!(cast(&built.parts, &words, &pane, nowhere), index("pane"));
        let behind = toward(DVec3::new(-0.3, 0.2, 0.2), [-0.3, 0.2, 0.0]);
        assert_eq!(cast(&built.parts, &words, &behind, nowhere), index("back"));
        let label = toward(from, [0.35, 0.15, 0.0]);
        assert_eq!(cast(&built.parts, &words, &label, nowhere), index("label"));
        assert_eq!(
            cast(&built.parts, &Words::default(), &label, nowhere),
            index("side")
        );
        let corner = toward(from, [0.22, 0.28, 0.0]);
        assert_eq!(cast(&built.parts, &words, &corner, nowhere), index("side"));
        let image = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(320.0, 200.0));
        let overlay = laid.overlay_boxes([640, 400]);
        assert_eq!(overlay.len(), 1);
        let words = words.with_overlay(image, [640, 400], &overlay);
        let rect = words.overlay[0].1;
        assert!(image.contains(rect.min) && rect.width() > 10.0, "{rect:?}");
        assert!((rect.min.x - 104.0).abs() < 2.0, "{rect:?}");
        assert_eq!(
            cast(&built.parts, &words, &pane, rect.center()),
            index("title")
        );
        assert_eq!(
            cast(&built.parts, &words, &pane, rect.max + Vec2::splat(1.0)),
            index("pane")
        );
    }
}
