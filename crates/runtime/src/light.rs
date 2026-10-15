use crate::raster::{Lens, Look};
use crate::scene::Scene;
use crate::vector::{cross, dot, sub, unit};
use pfx_bake::detail::atlas::{self, Atlas};
use pfx_bake::reflection::{ReflectionArtifact, read_reflection_artifact_from};
use pfx_bake::{Artifact, blend_anchors, read_artifact_from};
use pfx_geom::tree::Tree;
use pfx_live::canopy::{CanopyMode, Gobo};
use pfx_live::deform::WindFrame;
use pfx_live::effects::{CreatureInstance, CreatureMesh, CreatureVertex, Plume, VolumeKind};
use pfx_live::frame::{Instance, SceneWind};
use pfx_live::maps::select_baked_detail;
use pfx_live::probes::ProbeLighting;
use pfx_live::renderer::{Exposure, Renderer, Volume};
use pfx_live::shadow::{Quality, ReceiverBox};
use pfx_live::sky::SkySource;
use pfx_load::Sky;
use pfx_materials::Material;

pub use pfx_live::effects::ParticleInstance;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    pub caster_margin: f32,
    pub framed: f32,
    pub behind: f32,
    pub before: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wind {
    pub strength: f32,
    pub direction: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SteamLook {
    pub color: [f32; 3],
    pub rise: f32,
    pub clock: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoteLook {
    pub cell: f32,
    pub share: f32,
    pub spread: f32,
    pub glow: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bird {
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub period: f32,
    pub start: f32,
    pub span: f32,
    pub half_span: f32,
    pub distance: f32,
}

#[derive(Clone, Debug)]
pub struct Canopy {
    pub tree: Tree,
    pub sharpness: f32,
    pub bird: Option<Bird>,
}

#[derive(Clone, Debug)]
pub struct Lighting {
    pub hour: f32,
    pub framing: Framing,
    pub reflection_occlusion: f32,
    pub wind: Wind,
    pub steam: SteamLook,
    pub motes: MoteLook,
    pub canopy: Option<Canopy>,
}

fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

pub fn hash3(p: [i32; 3]) -> f32 {
    let h = (p[0] as u32).wrapping_mul(73_856_093)
        ^ (p[1] as u32).wrapping_mul(19_349_663)
        ^ (p[2] as u32).wrapping_mul(83_492_791);
    (pcg(h) >> 8) as f32 / 16_777_216.0
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

pub fn lhash(p: [f32; 2]) -> f32 {
    let q = [fract(p[0] * 123.34), fract(p[1] * 456.21)];
    fract((q[0] + 45.32) * (q[1] + 45.32) * 34.23 + q[0] * 13.7)
}

pub fn gobo(sun: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let s = unit(sun);
    let horizontal = [-s[2], 0.0, s[0]];
    let l = (horizontal[0] * horizontal[0] + horizontal[2] * horizontal[2])
        .sqrt()
        .max(1e-6);
    let u = [horizontal[0] / l, 0.0, horizontal[2] / l];
    (u, cross(u, s))
}

pub fn sky(pixels: &Sky, look: &Look) -> Sky {
    pixels.clone().rotated(-look.turn).exposed(look.sky)
}

pub fn receiver(lens: &Lens, size: (u32, u32), framing: &Framing) -> ReceiverBox {
    let offset = sub(lens.pivot, lens.eye);
    let depth = dot(offset, offset).sqrt();
    let aspect = size.0 as f32 / size.1.max(1) as f32;
    let half = [lens.tan_y * depth * aspect, lens.tan_y * depth];
    let centre = [0, 1, 2].map(|k| {
        lens.pivot[k]
            + lens.right[k] * lens.shift[0] * half[0]
            + lens.up[k] * lens.shift[1] * half[1]
    });
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for along in [-framing.behind, framing.before] {
                for k in 0..3 {
                    let v = centre[k]
                        + lens.right[k] * sx * half[0] * framing.framed
                        + lens.up[k] * sy * half[1] * framing.framed
                        - lens.forward[k] * along;
                    min[k] = min[k].min(v);
                    max[k] = max[k].max(v);
                }
            }
        }
    }
    ReceiverBox { min, max }
}

pub fn quality(
    base: Quality,
    radius_deg: f32,
    lens: &Lens,
    size: (u32, u32),
    framing: &Framing,
) -> Quality {
    Quality {
        receiver: Some(receiver(lens, size, framing)),
        caster_margin: framing.caster_margin,
        sun_radius_deg: radius_deg,
        ..base
    }
}

const MOTE_CELLS: i64 = 1 << 20;
const MOTE_REACH: i64 = 1 << 30;

const PLUME: Plume = Plume {
    source: [0.0; 3],
    radius: 0.0,
    spread: [0.7, 4.0],
    drift: [[-0.9, -0.12], [0.0, 0.02]],
    sway: [[17.0, 1.3, 0.012, 0.4, 6.0], [13.0, 0.9, 0.01, 0.4, 5.0]],
    fade: [0.02, 0.07, 0.2],
    depth_per_width: 9.0,
    grain: [70.0, 42.0],
    lift: 0.05,
    warp: [2.2, 0.2],
    churn: 0.7,
    threshold: [0.42, 0.9],
};

const GOBO: Gobo = Gobo {
    gust: [0.21, 0.6, 0.37, 1.3, 0.4],
    sway: [[0.9, 9.0, 0.005, 0.011], [0.7, 7.0, 0.004, 0.004]],
    branch: [-0.55, 0.15, 0.8, 0.6],
    clusters: [3.4, 5.0, 2.0, 0.3, 0.62],
    leaves: [17.0, 0.36, 0.56, 1.3, 1.1, 0.08],
    weight: 0.88,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steam {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub density: f32,
    pub anisotropy: f32,
    pub color: [f32; 3],
    pub clock: f32,
}

impl Steam {
    pub fn of(scene: &Scene, look: &SteamLook) -> Option<Steam> {
        let m = scene.motion.as_ref()?;
        let (cup, r) = (m.cup, m.radius);
        Some(Steam {
            lo: [cup[0] - r * 5.0, cup[1], cup[2] - r * 3.0],
            hi: [cup[0] + r * 3.0, cup[1] + look.rise, cup[2] + r * 3.0],
            density: m.density,
            anisotropy: m.anisotropy,
            color: look.color,
            clock: look.clock,
        })
    }

    pub fn volume(&self) -> Volume {
        let width = (self.hi[2] - self.lo[2]).max(0.000_001);
        Volume {
            kind: VolumeKind::Plume(Plume {
                source: [
                    self.hi[0] - width * 0.5,
                    self.lo[1],
                    (self.lo[2] + self.hi[2]) * 0.5,
                ],
                radius: width / 6.0,
                ..PLUME
            }),
            lo: self.lo,
            hi: self.hi,
            color: self.color,
            density: self.density,
            anisotropy: self.anisotropy,
            time_scale: self.clock,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Motes {
    pub cells: Vec<([i32; 3], [f32; 3])>,
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub size: f32,
    pub glow: f32,
    pub cell: f32,
}

impl Motes {
    pub fn of(scene: &Scene, look: &MoteLook) -> Option<Motes> {
        let m = scene.motion.as_ref()?;
        if m.dust <= 0.0 || m.mote <= 0.0 {
            return None;
        }
        let first = m.lo.map(|v| (v / look.cell).floor() as i64 - 2);
        let last = m.hi.map(|v| (v / look.cell).floor() as i64 + 2);
        let total = (0..3).try_fold(1i64, |total, k| {
            total.checked_mul((last[k] - first[k] + 1).max(0))
        })?;
        let bound = |v: &i64| v.abs() < MOTE_REACH;
        if total > MOTE_CELLS || !first.iter().chain(&last).all(bound) {
            return None;
        }
        let first = first.map(|v| v as i32);
        let last = last.map(|v| v as i32);
        let mut cells = Vec::new();
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    let c = [x, y, z];
                    if hash3([x + 17, y + 3, z + 5]) > look.share {
                        continue;
                    }
                    let jitter = [
                        hash3(c),
                        hash3([x + 5, y + 1, z + 7]),
                        hash3([x + 2, y + 9, z + 4]),
                    ];
                    cells.push((c, jitter));
                }
            }
        }
        let size = m.mote * look.spread;
        let glow =
            look.glow * m.dust * std::f32::consts::PI.sqrt() * m.mote.powi(3) / (size * size);
        Some(Motes {
            cells,
            lo: m.lo,
            hi: m.hi,
            size,
            glow,
            cell: look.cell,
        })
    }

    pub fn at(&self, time: f32) -> Vec<ParticleInstance> {
        let t = time * 0.5;
        let mut out = Vec::with_capacity(self.cells.len());
        for (c, j) in &self.cells {
            let wander = [
                (t * 0.3 + j[0] * 20.0).sin() * 0.35,
                ((t * 0.22 + j[1] * 17.0).sin() - t * 0.08) * 0.35,
                (t * 0.27 + j[2] * 13.0).cos() * 0.35,
            ];
            let offset = [0, 1, 2].map(|k| j[k] + wander[k]);
            if offset.iter().any(|&o| !(-0.5..1.5).contains(&o)) {
                continue;
            }
            let centre = [0, 1, 2].map(|k| (c[k] as f32 + offset[k]) * self.cell);
            if (0..3).any(|k| centre[k] < self.lo[k] || centre[k] > self.hi[k]) {
                continue;
            }
            out.push(ParticleInstance {
                position_size: [centre[0], centre[1], centre[2], self.size],
                velocity_alpha: [0.0, 0.0, 0.0, 1.0],
                color_rotation: [self.glow, self.glow, self.glow, 0.0],
                detail: [1.0, 0.0, j[0], 1.0],
            });
        }
        out
    }
}

impl Bird {
    pub fn at(&self, time: f32) -> Option<([f32; 2], f32, f32)> {
        let k = (time / self.period).floor();
        let local = time - k * self.period - self.start;
        if !(0.0..=self.span).contains(&local) {
            return None;
        }
        let s = local / self.span;
        let dir = if lhash([k, 1.7]) > 0.5 { 1.0 } else { -1.0 };
        let lift = (lhash([k, 3.1]) - 0.5) * 0.35;
        let x = (-0.75 + 1.5 * s) * dir;
        let y = lift + (s * std::f32::consts::PI).sin() * 0.06 - s * 0.05;
        Some(([x, y], dir, local))
    }

    pub fn instance(&self, time: f32, target: [f32; 3], sun: [f32; 3]) -> Option<CreatureInstance> {
        let (q, dir, local) = self.at(time)?;
        let s = unit(sun);
        let (u, v) = gobo(s);
        let p = [0, 1, 2].map(|k| target[k] + u[k] * q[0] + v[k] * q[1] + s[k] * self.distance);
        Some(CreatureInstance {
            position_scale: [p[0], p[1], p[2], self.half_span],
            right: [v[0], v[1], v[2], 0.0],
            up: [s[0], s[1], s[2], 0.0],
            forward: [u[0] * dir, u[1] * dir, u[2] * dir, 0.0],
            motion: [local * 12.0, 0.0, 0.0, 0.0],
        })
    }
}

pub fn wind(wind: &Wind, time: f32) -> SceneWind {
    let frame = |time: f32| WindFrame {
        time,
        strength: wind.strength,
        direction: wind.direction,
    };
    SceneWind {
        current: frame(time),
        previous: frame((time - 1.0 / 60.0).max(0.0)),
    }
}

pub fn anchors(hour: f32, anchors: &[f32]) -> Option<(f32, f32)> {
    let blend = blend_anchors(anchors, hour).ok()?;
    let (lower, upper) = (anchors[blend.lower], anchors[blend.upper]);
    Some(if blend.upper_weight <= 1e-6 {
        (lower, lower)
    } else if blend.upper_weight >= 1.0 - 1e-6 {
        (upper, upper)
    } else {
        (lower, upper)
    })
}

pub fn detail_line(hour: f32, pair: Option<(f32, f32)>) -> String {
    let clock = |h: f32| {
        let minutes = (h.rem_euclid(24.0) * 60.0).round() as u32 % (24 * 60);
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    };
    match pair {
        Some((a, b)) if a == b => format!("{} on the {} probes", clock(hour), clock(a)),
        Some((a, b)) => format!(
            "{} between the {} and {} probes",
            clock(hour),
            clock(a),
            clock(b)
        ),
        None => format!("{} without probes", clock(hour)),
    }
}

pub fn embedded<'a>(
    files: &'a [(&str, &[u8])],
    folder: &'a str,
) -> impl Fn(&str) -> Option<Vec<u8>> + 'a {
    move |name| {
        files
            .iter()
            .find(|(path, _)| {
                path.strip_prefix(folder)
                    .and_then(|rest| rest.strip_prefix('/'))
                    == Some(name)
            })
            .map(|(_, bytes)| bytes.to_vec())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub probes: String,
    pub locals: Vec<String>,
    pub reflection: String,
    pub detail: Option<String>,
}

pub struct Bake {
    pub probes: Artifact,
    pub locals: Vec<Artifact>,
    pub anchors: Vec<f32>,
    pub reflection: ReflectionArtifact,
    pub detail: Option<Atlas>,
}

impl Bake {
    pub fn read(files: &dyn Fn(&str) -> Option<Vec<u8>>, layout: &Layout) -> Result<Bake, String> {
        let under = |folder: &str| {
            let folder = folder.to_string();
            move |name: &str| files(&format!("{folder}/{name}"))
        };
        let probes = read_artifact_from(&under(&layout.probes))
            .map_err(|e| format!("{}: {e}", layout.probes))?;
        let locals = layout
            .locals
            .iter()
            .map(|name| read_artifact_from(&under(name)).map_err(|e| format!("{name}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let reflection = read_reflection_artifact_from(&under(&layout.reflection))
            .map_err(|e| format!("{}: {e}", layout.reflection))?;
        let detail = match &layout.detail {
            Some(name) => Some(
                atlas::read_from(&under(name))
                    .map(|(_, atlas)| atlas)
                    .map_err(|e| format!("{name}: {e}"))?,
            ),
            None => None,
        };
        Ok(Bake {
            anchors: probes.manifest.anchors.clone(),
            probes,
            locals,
            reflection,
            detail,
        })
    }

    pub fn from_files(files: &[(&str, &[u8])], layout: &Layout) -> Result<Bake, String> {
        Bake::read(
            &|path| {
                files
                    .iter()
                    .find(|(name, _)| *name == path)
                    .map(|(_, bytes)| bytes.to_vec())
            },
            layout,
        )
    }

    pub fn from_dir(dir: &std::path::Path, layout: &Layout) -> Result<Bake, String> {
        Bake::read(&|path| std::fs::read(dir.join(path)).ok(), layout)
    }

    pub fn with_detail(mut self, detail: Option<Atlas>) -> Bake {
        self.detail = detail;
        self
    }
}

pub fn layers(atlas: &Atlas, parts: &[u32]) -> Vec<Option<u32>> {
    parts
        .iter()
        .map(|part| {
            atlas
                .parts
                .iter()
                .position(|placed| placed.index == *part)
                .map(|layer| layer as u32)
        })
        .collect()
}

pub fn detailed(
    layers: &[Option<u32>],
    materials: &mut Vec<Material>,
    instances: &mut [Instance],
) -> Result<usize, String> {
    let mut bound = 0;
    for (instance, layer) in instances.iter_mut().zip(layers) {
        let Some(layer) = layer else {
            continue;
        };
        let mut material = materials
            .get(instance.material as usize)
            .copied()
            .ok_or("an instance names a missing material")?;
        select_baked_detail(&mut material, *layer)?;
        instance.material = materials.len() as u32;
        materials.push(material);
        bound += 1;
    }
    Ok(bound)
}

pub fn strays(scene: &Scene, atlas: &Atlas) -> Vec<String> {
    let parts: Vec<u32> = scene.pieces.iter().map(|piece| piece.part).collect();
    let layers = layers(atlas, &parts);
    atlas
        .parts
        .iter()
        .enumerate()
        .filter_map(|(k, placed)| {
            let pieces: Vec<&crate::scene::Piece> = scene
                .pieces
                .iter()
                .zip(&layers)
                .filter(|(_, layer)| **layer == Some(k as u32))
                .map(|(piece, _)| piece)
                .collect();
            let fits = match pieces.as_slice() {
                [piece] => {
                    scene
                        .objects
                        .get(piece.object as usize)
                        .is_some_and(|name| *name == placed.node)
                        && scene.group_of(piece.object).is_none()
                }
                _ => false,
            };
            (!fits).then(|| {
                format!(
                    "{} (part {}) binds {} pieces",
                    placed.node,
                    placed.index,
                    pieces.len()
                )
            })
        })
        .collect()
}

pub struct Light {
    pub hour: f32,
    pub sky: Sky,
    pub target: [f32; 3],
    pub sun: [f32; 3],
    pub radius_deg: f32,
    pub canopy: bool,
    pub steam: Option<Steam>,
    pub motes: Option<Motes>,
    pub anchors: Vec<f32>,
    pub lighting: Lighting,
    bird: Option<CreatureMesh>,
}

impl Light {
    pub fn new(scene: &Scene, look: &Look, pixels: &Sky, lighting: Lighting) -> Light {
        Light {
            hour: lighting.hour,
            sky: sky(pixels, look),
            target: scene.camera.target,
            sun: unit(scene.sun.toward),
            radius_deg: scene.sun.radius_deg,
            canopy: lighting.canopy.is_some(),
            steam: Steam::of(scene, &lighting.steam),
            motes: Motes::of(scene, &lighting.motes),
            anchors: Vec::new(),
            lighting,
            bird: None,
        }
    }

    pub fn install(
        &mut self,
        renderer: &mut Renderer,
        look: &Look,
        lens: &Lens,
    ) -> Result<(), String> {
        renderer.set_sky(SkySource::Hdr(self.sky.clone()), self.hour)?;
        self.resized(renderer, lens);
        if let Some(canopy) = &self.lighting.canopy {
            let origin = [0, 1, 2].map(|k| self.target[k] + self.sun[k]);
            renderer.set_tree(&canopy.tree, origin, 0.0, 0, 0.0, [1.0; 3])?;
            renderer.set_canopy_gobo(GOBO)?;
            renderer.set_canopy_strength(GOBO.weight)?;
            renderer.set_canopy_mode(CanopyMode::Procedural)?;
            renderer.set_canopy_sharpness(canopy.sharpness)?;
            if let Some(bird) = &canopy.bird {
                let vertices: Vec<CreatureVertex> = bird
                    .vertices
                    .iter()
                    .map(|p| CreatureVertex {
                        position: [p[0], p[1], p[2], 0.0],
                    })
                    .collect();
                self.bird = Some(CreatureMesh::new(
                    &renderer.gpu().device,
                    &vertices,
                    &bird.indices,
                ));
            }
        }
        renderer.set_finish(look.chain.clone());
        renderer.set_exposure(Exposure::Fixed(1.0))
    }

    pub fn resized(&self, renderer: &mut Renderer, lens: &Lens) {
        let size = renderer.size();
        let base = renderer.shadows().quality();
        renderer.set_shadow_quality(quality(
            base,
            self.radius_deg,
            lens,
            size,
            &self.lighting.framing,
        ));
    }

    pub fn set_hour(&mut self, renderer: &mut Renderer, hour: f32) -> Result<(), String> {
        if !hour.is_finite() {
            return Err("the hour must be finite".into());
        }
        self.hour = hour;
        if renderer.sky_settling() {
            return self.request(renderer);
        }
        renderer.set_hour(hour)
    }

    fn request(&self, renderer: &mut Renderer) -> Result<(), String> {
        renderer
            .request_sky(SkySource::Hdr(self.sky.clone()), self.hour)
            .map(|_| ())
    }

    pub fn bake(
        &mut self,
        renderer: &mut Renderer,
        bake: Bake,
        parts: &[u32],
        materials: &mut Vec<Material>,
        instances: &mut [Instance],
    ) -> Result<usize, String> {
        let gpu = renderer.gpu();
        let mut probes = ProbeLighting::from_artifacts(
            &gpu.device,
            &gpu.queue,
            &bake.probes,
            &bake.locals,
            self.hour,
            [-1.0; 3],
        )?;
        probes.set_reflection_occlusion(&gpu.queue, self.lighting.reflection_occlusion)?;
        renderer.set_probes(probes)?;
        let mut bound = 0;
        if let Some(atlas) = &bake.detail {
            renderer.set_detail(atlas)?;
            bound = detailed(&layers(atlas, parts), materials, instances)?;
        }
        renderer
            .frame()
            .set_local_reflections(Some(vec![bake.reflection]), self.hour)?;
        self.anchors = bake.anchors;
        self.request(renderer)?;
        Ok(bound)
    }

    pub fn wind(&self, time: f32) -> SceneWind {
        wind(&self.lighting.wind, time)
    }

    pub fn volume(&self) -> Option<Volume> {
        self.steam.map(|s| s.volume())
    }

    pub fn particles(&self, time: f32) -> Vec<ParticleInstance> {
        self.motes.as_ref().map(|m| m.at(time)).unwrap_or_default()
    }

    pub fn birds(&self, time: f32) -> Vec<CreatureInstance> {
        let Some(bird) = self.lighting.canopy.as_ref().and_then(|c| c.bird.as_ref()) else {
            return Vec::new();
        };
        if self.bird.is_none() {
            return Vec::new();
        }
        bird.instance(time, self.target, self.sun)
            .into_iter()
            .collect()
    }

    pub fn bird_mesh(&self) -> Option<&CreatureMesh> {
        self.bird.as_ref()
    }

    pub fn detail(&self) -> String {
        detail_line(self.hour, anchors(self.hour, &self.anchors))
    }
}
