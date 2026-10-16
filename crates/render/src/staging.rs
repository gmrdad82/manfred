use crate::text::Lettering;
use manfred_build::Part;
use manfred_recipe::{Recipe, Shadow};
use manfred_scene::{Card, Cards, Error, Eye, Laid, Lighting, Palette, Picture, Piece};
use pfx_live::frame::{Matrix, multiply};
use pfx_live::sky::SkySource;
use pfx_materials::{Blend, Material};
use pfx_post::Chain;
use pfx_trace::detail::Lens;
use pfx_trace::lights::{LightShape, LocalLight};
use pfx_trace::sky::Environment;
use pfx_trace::stage::{Image, Mesh, Placement, PlacementContent, Stage, Staged};
use pfx_trace::{Camera, Projection, Sun};

pub use manfred_scene::IDENTITY;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Lens,
    Ortho,
    Equirect,
}

#[derive(Clone, Copy, Debug)]
pub struct Eyepiece {
    pub camera: Camera,
    pub projection: Projection,
    pub lens: Lens,
    pub eye: Eye,
}

pub struct Staging {
    pub pieces: Vec<Piece>,
    pub slots: Vec<u32>,
    pub names: Vec<String>,
    pub materials: Vec<Material>,
    pub cards: Vec<Option<Card>>,
    pub mapped: Vec<Option<usize>>,
    pub images: Vec<Picture>,
    pub clips: Vec<[[f32; 4]; 2]>,
    pub sky: Environment,
    pub sun: Sun,
    pub sun_radius_deg: f32,
    pub transmissive_shadows: bool,
    pub clamp_indirect: f32,
    pub filter_glossy: f32,
    pub lights: Vec<LocalLight>,
    pub finish: Chain,
    pub text: Laid,
    pub posed: Vec<Matrix>,
}

impl Staging {
    pub fn of(recipe: &Recipe, parts: &[Part]) -> Result<Staging, Error> {
        let pieces = manfred_scene::pieces(parts);
        let found = Cards::of(recipe, &pieces)?;
        let mut images: Vec<Picture> = found
            .pictures
            .iter()
            .map(|picture| Picture {
                width: picture.width,
                height: picture.height,
                texels: manfred_scene::premultiplied(picture),
            })
            .collect();
        let mut mapped: Vec<Option<usize>> = vec![None; pieces.len()];
        let mut seen: Vec<(&[u8], usize)> = Vec::new();
        for ((at, piece), card) in pieces.iter().enumerate().zip(&found.cards) {
            let Some(map) = piece
                .look
                .as_ref()
                .and_then(|look| look.base.as_ref())
                .filter(|map| card.is_none() && map.tex_coord == 0)
            else {
                continue;
            };
            let index = match seen.iter().find(|(bytes, _)| **bytes == *map.image.bytes) {
                Some(&(_, index)) => index,
                None => {
                    let picture = manfred_scene::decode(&map.image.bytes).map_err(|e| {
                        Error(format!("materials.{}: the base map: {e}", piece.material))
                    })?;
                    images.push(Picture {
                        width: picture.width,
                        height: picture.height,
                        texels: manfred_scene::premultiplied(&picture),
                    });
                    seen.push((&map.image.bytes, images.len() - 1));
                    images.len() - 1
                }
            };
            mapped[at] = Some(index);
        }
        let layers = found.cards.iter().zip(&mapped).map(|(card, map)| {
            if card.is_some() {
                Some(Blend::Over)
            } else {
                map.map(|_| Blend::Multiply)
            }
        });
        let Palette {
            names,
            materials,
            slots,
        } = manfred_scene::palette_with(recipe, &pieces, layers)?;
        let clips = pieces
            .iter()
            .map(|piece| manfred_scene::clips(recipe, piece.object))
            .collect();
        let lighting = Lighting::of(recipe)?;
        let shine = lighting.shine(None);
        let sky = match lighting.at(None).sky {
            SkySource::Analytic(sky) => Environment::Analytic(sky),
            SkySource::Hdr(sky) => Environment::Hdr(sky),
        };
        let lights = lighting
            .lights
            .iter()
            .map(|light| LocalLight {
                position: light.position,
                colour: light.colour,
                intensity: light.intensity,
                radius: light.radius,
                range: light.range,
                shadow: light.shadow,
                shape: LightShape::Point,
            })
            .collect();
        let count = pieces.len();
        Ok(Staging {
            pieces,
            slots,
            names,
            materials,
            cards: found.cards,
            mapped,
            images,
            clips,
            sky,
            sun: Sun {
                direction: shine.sun.direction,
                color: shine.sun.colour,
                intensity: shine.sun.intensity,
            },
            sun_radius_deg: shine.radius_deg,
            transmissive_shadows: recipe.trace.transmissive_shadows,
            clamp_indirect: recipe.trace.clamp_indirect as f32,
            filter_glossy: recipe.trace.filter_glossy as f32,
            lights,
            finish: lighting.finish.clone(),
            text: Laid::of(recipe, parts)?,
            posed: vec![IDENTITY; count],
        })
    }

    pub fn meshes(&self) -> Vec<Mesh<'_>> {
        self.pieces
            .iter()
            .map(|piece| {
                let geometry = &piece.geometry;
                Mesh {
                    positions: &geometry.positions,
                    normals: &geometry.normals,
                    tangents: &geometry.tangents,
                    uvs: &geometry.uvs,
                    uvs1: None,
                    alpha: None,
                    indices: &geometry.indices,
                }
            })
            .collect()
    }

    pub fn content(&self, card: &Card) -> PlacementContent<'_> {
        PlacementContent {
            cutout: card.cuts,
            ..PlacementContent::new(self.image(card.picture))
        }
    }

    pub fn image(&self, picture: usize) -> Image<'_> {
        let picture = &self.images[picture];
        Image {
            width: picture.width,
            height: picture.height,
            texels: picture.texels.as_chunks::<4>().0,
            srgb: true,
        }
    }

    pub fn placements(&self, eye: &Eye) -> Vec<Placement<'_>> {
        self.slots
            .iter()
            .zip(&self.pieces)
            .zip(&self.cards)
            .zip(&self.clips)
            .zip(&self.mapped)
            .enumerate()
            .map(|(mesh, ((((&material, piece), card), clip), mapped))| {
                let faced = card
                    .and_then(|card| card.face)
                    .map_or(IDENTITY, |face| manfred_scene::facing(&face, eye));
                let posed = self.posed.get(mesh).copied().unwrap_or(IDENTITY);
                let model = if posed == IDENTITY {
                    faced
                } else {
                    multiply(posed, faced)
                };
                let mut placed = Placement {
                    casts_shadow: piece.shadow != Shadow::None,
                    content: card.as_ref().map(|card| self.content(card)).or_else(|| {
                        mapped.map(|picture| PlacementContent::new(self.image(picture)))
                    }),
                    clip: *clip,
                    shadow_only: piece.shadow == Shadow::Only,
                    two_sided: piece.two_sided,
                    ..Placement::new(mesh as u32, model, material)
                };
                if let Some(card) = card {
                    placed.alpha_cutoff = card.cutoff;
                }
                placed
            })
            .collect()
    }

    pub fn stage(&self, eyepiece: Eyepiece) -> Result<Staged<Environment>, Error> {
        if self.pieces.is_empty() {
            return Err(Error(
                "the recipe has nothing to trace: no object with a mesh".into(),
            ));
        }
        let lettering = Lettering::of(self)?;
        let mut meshes = self.meshes();
        let mut placements = self.placements(&eyepiece.eye);
        let mut materials = self.materials.clone();
        lettering.stage(&mut meshes, &mut placements, &mut materials);
        let mut staged = Stage {
            meshes: &meshes,
            instances: &placements,
            materials: &materials,
            sky: self.sky.clone(),
            sun: self.sun,
            camera: eyepiece.camera,
            projection: eyepiece.projection,
            lens: eyepiece.lens,
        }
        .build()
        .map_err(|e| Error(format!("the tracer's stage: {e}")))?;
        staged.detail.lights = self.lights.clone();
        staged.detail.sun_radius_deg = self.sun_radius_deg;
        staged.detail.transmissive_shadows = self.transmissive_shadows;
        staged.detail.clamp_indirect = self.clamp_indirect;
        staged.detail.filter_glossy = self.filter_glossy;
        Ok(staged)
    }
}

pub fn eyepiece(
    camera: &manfred_recipe::Camera,
    view: View,
    size: [u32; 2],
) -> Result<Eyepiece, Error> {
    if size[0] == 0 || size[1] == 0 {
        return Err(Error(format!("the size {}x{} is empty", size[0], size[1])));
    }
    let aspect = f64::from(size[0]) / f64::from(size[1]);
    let eye = Eye::from_camera(camera, aspect);
    let [right, up, forward] = eye.basis();
    let narrow = |v: [f64; 3], scale: f64| v.map(|x| (x * scale) as f32);
    let basis = |across: f64, high: f64| Camera {
        origin: narrow(eye.position, 1.0),
        forward: narrow(forward, 1.0),
        right: narrow(right, across),
        up: narrow(up, high),
    };
    let shifted = Lens {
        shift: eye.shift.map(|v| v as f32),
        ..Lens::default()
    };
    let ortho = match view {
        View::Ortho => Some(camera.ortho_height.ok_or_else(|| {
            Error(format!(
                "camera.{}: --ortho uses the camera's ortho_height, and it has none",
                camera.name
            ))
        })?),
        View::Lens => eye.ortho_height,
        View::Equirect => None,
    };
    let eye = Eye {
        ortho_height: ortho,
        ..eye
    };
    let (camera, projection, lens) = match (view, ortho) {
        (View::Equirect, _) => (
            basis(1.0, 1.0),
            Projection::Equirectangular,
            Lens::default(),
        ),
        (_, Some(height)) if !(height.is_finite() && height > 0.0) => {
            return Err(Error(format!(
                "camera.{}: ortho_height is {height}, expected a positive number",
                camera.name
            )));
        }
        (_, Some(height)) => (
            basis(1.0, 1.0),
            Projection::Orthographic {
                width: (height * aspect) as f32,
                height: height as f32,
            },
            shifted,
        ),
        (_, None) => {
            let tan_y = (eye.fov_y_deg.to_radians() * 0.5).tan();
            if !(tan_y.is_finite() && tan_y > 0.0) {
                return Err(Error(format!(
                    "camera.{}: a field of view of {} degrees cannot be traced",
                    camera.name, eye.fov_y_deg
                )));
            }
            let focused = eye.lens(camera, aspect).map_or(shifted, |lens| Lens {
                aperture: lens.focal_m * lens.aperture / lens.fstop,
                focus_distance: lens.focus_m,
                ..shifted
            });
            (
                basis(tan_y * aspect, tan_y),
                Projection::Perspective,
                focused,
            )
        }
    };
    Ok(Eyepiece {
        camera,
        projection,
        lens,
        eye,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::parse;
    use std::path::Path;

    const SCENE: &str = "manfred = 1\nname = \"t\"\n\n[sun]\nmodel = \"authored\"\ntoward = [0.0, 1.0, 0.0]\nradius_deg = 1.5\ncolor = [1.0, 0.5, 0.25]\nirradiance = 3.0\n\n[[light]]\nname = \"lamp\"\nposition = [0.0, 2.0, 0.0]\ncolor = [1.0, 1.0, 1.0]\nintensity = 4.0\nradius = 0.05\nrange = 6.0\nshadow = true\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 0.0, 4.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 90.0\nshift = [0.25, -0.5]\n\n[[camera]]\nname = \"plan\"\nkind = \"orthographic\"\nposition = [0.0, 5.0, 0.0]\ntarget = [0.0, 0.0, 0.0]\nup = [0.0, 0.0, -1.0]\northo_height = 2.0\n\n[materials.red]\nbase = [0.8, 0.1, 0.1]\nroughness = 0.5\n\n[materials.steel]\nbase = [0.9, 0.9, 0.9]\nmetalness = 1.0\nroughness = 0.1\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nmaterial = \"red\"\nsize = [1.0, 1.0, 1.0]\n\n[[object]]\nname = \"b\"\nkind = \"box\"\nmaterial = \"steel\"\nsize = [0.5, 0.5, 0.5]\nat = [2.0, 0.0, 0.0]\n\n[[object]]\nname = \"c\"\nkind = \"box\"\nmaterial = \"red\"\nsize = [1.0, 1.0, 1.0]\nat = [-2.0, 0.0, 0.0]\nshadow = \"only\"\n";

    fn staging(text: &str) -> (Recipe, Staging) {
        let recipe = parse(text).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let staging = Staging::of(&recipe, &built.parts).unwrap();
        (recipe, staging)
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    #[test]
    fn three_boxes_stage_as_three_placed_meshes_with_interned_materials() {
        let (recipe, staging) = staging(SCENE);
        assert_eq!(staging.names, ["red", "steel"]);
        assert_eq!(staging.slots, [0, 1, 0]);
        assert_eq!(staging.materials[0].base, [0.8, 0.1, 0.1]);
        let eyepiece = eyepiece(&recipe.cameras[0], View::Lens, [64, 32]).unwrap();
        let mut expected: Vec<_> = [0u32, 1, 0]
            .iter()
            .enumerate()
            .map(|(mesh, &material)| Placement {
                two_sided: false,
                ..Placement::new(mesh as u32, IDENTITY, material)
            })
            .collect();
        expected[2].shadow_only = true;
        let placements = staging.placements(&eyepiece.eye);
        assert_eq!(placements, expected);
        assert!(placements.iter().all(|placed| placed.clip == [[0.0; 4]; 2]));
        assert!(
            placements.iter().all(|placed| {
                placed.alpha_cutoff == Placement::new(0, IDENTITY, 0).alpha_cutoff
            })
        );
        let staged = staging.stage(eyepiece).unwrap();
        assert!(staged.detail.content_slots.is_empty());
        assert!(
            staged
                .detail
                .instances
                .iter()
                .all(|i| i.surface.casts_shadow && !i.surface.cutout)
        );
        assert!(staged.scene.triangles.is_empty());
        assert_eq!(staged.scene.materials.len(), 2);
        let instances = &staged.detail.instances;
        assert_eq!(instances.len(), 3);
        assert_eq!(
            instances
                .iter()
                .map(|i| i.triangles.len())
                .collect::<Vec<_>>(),
            [12, 12, 12]
        );
        assert_eq!(
            instances
                .iter()
                .map(|i| i.triangles[0].material)
                .collect::<Vec<_>>(),
            [0, 1, 0]
        );
        assert_eq!(
            instances.iter().map(|i| i.shadow_only).collect::<Vec<_>>(),
            [false, false, true]
        );
        assert!(instances.iter().all(|i| i.transform == IDENTITY));
        let corners = |at: usize| {
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for triangle in &instances[at].triangles {
                for vertex in triangle.vertices {
                    for k in 0..3 {
                        lo[k] = lo[k].min(vertex[k]);
                        hi[k] = hi[k].max(vertex[k]);
                    }
                }
            }
            [lo, hi]
        };
        assert_eq!(corners(0), [[-0.5; 3], [0.5; 3]]);
        assert_eq!(corners(1), [[1.75, -0.25, -0.25], [2.25, 0.25, 0.25]]);
        for triangle in &instances[0].triangles {
            let [a, b, c] = triangle.vertices;
            let centre: [f32; 3] = std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0);
            let normal = triangle.normals[0];
            let outward: f32 = (0..3).map(|k| centre[k] * normal[k]).sum();
            assert!(outward > 0.0, "{triangle:?}");
        }
    }

    #[test]
    fn the_recipes_trace_key_reaches_the_tracers_detail() {
        let (recipe, plain) = staging(SCENE);
        let eye = eyepiece(&recipe.cameras[0], View::Lens, [8, 8]).unwrap();
        assert!(!plain.transmissive_shadows);
        assert!(!plain.stage(eye).unwrap().detail.transmissive_shadows);
        let (recipe, traced) = staging(&format!("{SCENE}\n[trace]\ntransmissive_shadows = true\n"));
        let eye = eyepiece(&recipe.cameras[0], View::Lens, [8, 8]).unwrap();
        assert!(traced.transmissive_shadows);
        assert!(traced.stage(eye).unwrap().detail.transmissive_shadows);
    }

    #[test]
    fn the_firefly_settings_reach_the_tracers_detail_and_are_off_by_default() {
        let (recipe, plain) = staging(SCENE);
        let eye = eyepiece(&recipe.cameras[0], View::Lens, [8, 8]).unwrap();
        let detail = plain.stage(eye).unwrap().detail;
        assert_eq!((detail.clamp_indirect, detail.filter_glossy), (0.0, 0.0));
        let (recipe, traced) = staging(&format!(
            "{SCENE}\n[trace]\nclamp_indirect = 4\nfilter_glossy = 0.25\n"
        ));
        let eye = eyepiece(&recipe.cameras[0], View::Lens, [8, 8]).unwrap();
        let detail = traced.stage(eye).unwrap().detail;
        assert_eq!((detail.clamp_indirect, detail.filter_glossy), (4.0, 0.25));
    }

    #[test]
    fn the_sun_sky_and_lights_are_the_live_scenes() {
        let (_, staging) = staging(SCENE);
        assert!(close(staging.sun.direction, [0.0, 1.0, 0.0]));
        assert_eq!(staging.sun.color, [1.0, 0.5, 0.25]);
        assert_eq!(staging.sun.intensity, 3.0);
        assert_eq!(staging.sun_radius_deg, 1.5);
        assert!(matches!(staging.sky, Environment::Analytic(_)));
        assert_eq!(
            staging.lights,
            [LocalLight {
                position: [0.0, 2.0, 0.0],
                colour: [1.0; 3],
                intensity: 4.0,
                radius: 0.05,
                range: 6.0,
                shadow: true,
                shape: LightShape::Point,
            }]
        );
        let recipe = parse(SCENE).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let eyepiece = eyepiece(&recipe.cameras[0], View::Lens, [8, 8]).unwrap();
        let staged = staging.stage(eyepiece).unwrap();
        assert_eq!(staged.detail.lights, staging.lights);
        assert_eq!(staged.detail.sun_radius_deg, 1.5);
        assert_eq!(built.parts.len(), 3);
    }

    #[test]
    fn an_unknown_material_and_an_empty_scene_are_refused() {
        let recipe = parse(&SCENE.replace("\"steel\"", "\"no_such\"")).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let error = Staging::of(&recipe, &built.parts).err().unwrap();
        assert!(error.0.contains("no_such"), "{error}");
        let cut = SCENE.find("[[object]]").unwrap();
        let (recipe, empty) = staging(&SCENE[..cut]);
        let eyepiece = eyepiece(&recipe.cameras[0], View::Lens, [8, 8]).unwrap();
        assert!(empty.stage(eyepiece).is_err());
    }

    #[test]
    fn a_perspective_camera_carries_its_tangents_and_shift() {
        let recipe = parse(SCENE).unwrap();
        let eyepiece = eyepiece(&recipe.cameras[0], View::Lens, [200, 100]).unwrap();
        assert_eq!(eyepiece.projection, Projection::Perspective);
        assert!(close(eyepiece.camera.origin, [0.0, 0.0, 4.0]));
        assert!(close(eyepiece.camera.forward, [0.0, 0.0, -1.0]));
        assert!(close(eyepiece.camera.right, [2.0, 0.0, 0.0]));
        assert!(close(eyepiece.camera.up, [0.0, 1.0, 0.0]));
        assert_eq!(eyepiece.lens.shift, [0.25, -0.5]);
    }

    #[test]
    fn a_closed_aperture_traces_a_pinhole() {
        let recipe = parse(SCENE).unwrap();
        assert_eq!(recipe.cameras[0].fstop, 0.0);
        let eyepiece = eyepiece(&recipe.cameras[0], View::Lens, [200, 100]).unwrap();
        assert_eq!(
            eyepiece.lens,
            Lens {
                shift: [0.25, -0.5],
                ..Lens::default()
            }
        );
    }

    #[test]
    fn an_open_aperture_focuses_on_the_target_or_focus_m() {
        let open = SCENE.replace(
            "shift = [0.25, -0.5]\n",
            "shift = [0.25, -0.5]\nfstop = 2.0\n",
        );
        let lens = |text: &str, view: View| {
            let recipe = parse(text).unwrap();
            eyepiece(&recipe.cameras[0], view, [200, 100]).unwrap().lens
        };
        let diameter = 0.036 / (2.0 * 2.0) / 2.0;
        let target = lens(&open, View::Lens);
        assert_eq!(target.shift, [0.25, -0.5]);
        assert!((target.aperture - diameter).abs() < 1e-7, "{target:?}");
        assert!((target.focus_distance - 4.0).abs() < 1e-6, "{target:?}");
        let near = open.replace("fstop = 2.0\n", "fstop = 2.0\nfocus_m = 2.5\n");
        let focused = lens(&near, View::Lens);
        assert!((focused.aperture - diameter).abs() < 1e-7, "{focused:?}");
        assert!((focused.focus_distance - 2.5).abs() < 1e-6, "{focused:?}");
        assert_eq!(lens(&near, View::Equirect), Lens::default());
    }

    #[test]
    fn ortho_uses_the_cameras_height_and_equirect_its_basis() {
        let recipe = parse(SCENE).unwrap();
        let plan = eyepiece(&recipe.cameras[1], View::Lens, [300, 100]).unwrap();
        assert_eq!(
            plan.projection,
            Projection::Orthographic {
                width: 6.0,
                height: 2.0
            }
        );
        assert!(close(plan.camera.forward, [0.0, -1.0, 0.0]));
        assert!(close(plan.camera.up, [0.0, 0.0, -1.0]));
        assert!(close(plan.camera.right, [1.0, 0.0, 0.0]));
        assert_eq!(
            eyepiece(&recipe.cameras[1], View::Ortho, [300, 100])
                .unwrap()
                .projection,
            plan.projection
        );
        let error = eyepiece(&recipe.cameras[0], View::Ortho, [300, 100])
            .err()
            .unwrap();
        assert!(error.0.contains("ortho_height"), "{error}");
        let round = eyepiece(&recipe.cameras[0], View::Equirect, [64, 32]).unwrap();
        assert_eq!(round.projection, Projection::Equirectangular);
        assert_eq!(round.lens, Lens::default());
        assert!(close(round.camera.origin, [0.0, 0.0, 4.0]));
        assert!(close(round.camera.forward, [0.0, 0.0, -1.0]));
        assert!(close(round.camera.right, [1.0, 0.0, 0.0]));
        assert!(close(round.camera.up, [0.0, 1.0, 0.0]));
        assert!(eyepiece(&recipe.cameras[0], View::Lens, [0, 8]).is_err());
    }
}
