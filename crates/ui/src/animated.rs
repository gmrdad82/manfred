use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use glam::{DMat4, DVec3};
use manfred_scene::Eye;
use pfx_core::anim::{Pose, Rig};
use pfx_live::frame::Instance;
use pfx_live::glass;
use pfx_live::lights::{LocalLight, local_light};
use pfx_live::particles::{SprayInstance, Sprays};
use pfx_live::scene::Staged;
use pfx_load::scene::{Light, Matrix, Projection, Scene, multiply};
use pfx_materials::Family;

use crate::timeline::Span;

pub const CAMERA_PREFIX: &str = "camera.";

#[derive(Clone, Debug)]
struct Played {
    place: usize,
    rig: Arc<Rig>,
    clip: usize,
    start: f32,
    speed: f32,
}

impl Played {
    fn palette(&self, time: f32) -> Vec<Matrix> {
        let skeleton = self.rig.skeleton();
        let mut pose = Pose::rest(skeleton);
        self.rig.clips()[self.clip].sample(self.start + time * self.speed, &mut pose);
        pose.palette(skeleton)
    }
}

fn played(scene: &Scene) -> Vec<Played> {
    scene
        .objects
        .iter()
        .enumerate()
        .filter_map(|(place, object)| {
            let animation = scene.animations.get(&object.name)?;
            let rig = animation.rig.clone()?;
            let clip = rig.clip(animation.clip.as_deref()?)?;
            Some(Played {
                place,
                rig,
                clip,
                start: animation.start,
                speed: animation.speed,
            })
        })
        .collect()
}

fn glb_json(path: &Path) -> Option<serde_json::Value> {
    let bytes = std::fs::read(path).ok()?;
    let word = |at: usize| -> Option<usize> {
        let four: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_le_bytes(four) as usize)
    };
    if bytes.get(..4)? != b"glTF" || word(16)? != 0x4E4F_534A {
        return None;
    }
    let length = word(12)?;
    serde_json::from_slice(bytes.get(20..20 + length)?).ok()
}

fn aspect(scene: &Scene, object: usize) -> Option<f32> {
    let mesh = scene.meshes.get(&scene.objects.get(object)?.mesh)?;
    let json = glb_json(&scene.path.parent()?.join(&mesh.file)).or_else(|| glb_json(&mesh.file))?;
    json["cameras"]
        .as_array()?
        .iter()
        .find_map(|camera| camera["perspective"]["aspectRatio"].as_f64())
        .filter(|aspect| aspect.is_finite() && *aspect > 0.0)
        .map(|aspect| aspect as f32)
}

pub fn span(scene: &Scene) -> Option<Span> {
    let played = played(scene);
    let first = played.first()?;
    let clip = &first.rig.clips()[first.clip];
    let keys = clip
        .channels()
        .iter()
        .map(|channel| channel.times.len())
        .max()?;
    let frames = u32::try_from(keys.checked_sub(1)?).ok()?;
    let seconds = clip.duration();
    if frames == 0 || seconds <= 0.0 {
        return None;
    }
    Some(Span {
        fps: (frames as f32 / seconds).round().max(1.0) as u32,
        frames,
        aspect: aspect(scene, first.place).unwrap_or(crate::viewport::ASPECT),
    })
}

pub struct Animated {
    played: Vec<Played>,
    objects: Vec<Matrix>,
    attached: Vec<Light>,
    fov: Option<f32>,
    sprays: Sprays,
    spray_time: Option<f32>,
    spray_parents: Vec<Matrix>,
    spray_next: Vec<Matrix>,
    previous: Vec<Matrix>,
    pub instances: Vec<Instance>,
    pub glass: Vec<glass::Surface>,
    pub spray_instances: Vec<SprayInstance>,
    pub lights: Option<Vec<LocalLight>>,
}

impl Animated {
    pub fn of(scene: &Scene) -> Option<Animated> {
        let played = played(scene);
        if played.is_empty() || scene.plates.is_some() {
            return None;
        }
        let attached = if scene.lights.values().any(|light| light.attach.is_some()) {
            scene.lights.values().copied().collect()
        } else {
            Vec::new()
        };
        let fov = match scene.camera_or_default().projection {
            Projection::Perspective { fov } => Some(fov),
            Projection::Orthographic { .. } => None,
        };
        Some(Animated {
            played,
            objects: scene.objects.iter().map(|object| object.model).collect(),
            attached,
            fov,
            sprays: Sprays::of(scene),
            spray_time: None,
            spray_parents: Vec::new(),
            spray_next: Vec::new(),
            previous: Vec::new(),
            instances: Vec::new(),
            glass: Vec::new(),
            spray_instances: Vec::new(),
            lights: None,
        })
    }

    fn palettes(&self, time: f32) -> BTreeMap<usize, Vec<Matrix>> {
        self.played
            .iter()
            .map(|played| (played.place, played.palette(time)))
            .collect()
    }

    pub fn pose(&mut self, staged: &Staged, time: f32) {
        let palettes = self.palettes(time);
        let draws = staged.draws();
        let models = draws.models_posed(&self.objects, &palettes);
        self.instances.clear();
        self.instances.extend_from_slice(staged.instances());
        self.glass.clear();
        let staged_glass = staged.glass();
        let (mut opaque, mut glass_at) = (0, 0);
        for (index, draw) in draws.items.iter().enumerate() {
            let model = models[index];
            let previous = self.previous.get(index).copied().unwrap_or(model);
            let family = draws
                .materials
                .get(draw.material as usize)
                .map(|material| material.family);
            if family == Some(Family::Glass) {
                if staged
                    .overrides()
                    .get(&draw.object)
                    .is_some_and(|change| change.hidden)
                {
                    continue;
                }
                if let Some(surface) = staged_glass.get(glass_at) {
                    self.glass.push(glass::Surface { model, ..*surface });
                }
                glass_at += 1;
                continue;
            }
            if let Some(instance) = self.instances.get_mut(opaque) {
                instance.model = model;
                instance.previous_model = previous;
            }
            opaque += 1;
        }
        self.previous = models;
        self.lights = (!self.attached.is_empty()).then(|| {
            self.attached
                .iter()
                .map(|light| local_light(&light.placed_with(&self.objects, &palettes)))
                .collect()
        });
        self.spray(&palettes, time);
    }

    fn spray(&mut self, palettes: &BTreeMap<usize, Vec<Matrix>>, time: f32) {
        if self.sprays.is_empty() {
            self.spray_instances.clear();
            return;
        }
        self.sprays
            .parents(Some(&self.objects), palettes, &mut self.spray_next);
        let from = match self.spray_time {
            Some(last) if last <= time => last,
            _ => {
                self.sprays.reset();
                self.spray_parents.clone_from(&self.spray_next);
                time
            }
        };
        self.sprays
            .seek_between([from, time], &self.spray_parents, &self.spray_next);
        self.sprays.pack(&mut self.spray_instances);
        std::mem::swap(&mut self.spray_parents, &mut self.spray_next);
        self.spray_time = Some(time);
    }

    pub fn chase(&self, time: f32) -> Option<Eye> {
        let fov = self.fov?;
        let world = self.played.iter().find_map(|played| {
            let joint = played
                .rig
                .skeleton()
                .joints()
                .iter()
                .position(|joint| joint.name.starts_with(CAMERA_PREFIX))?;
            let palette = played.palette(time);
            Some(multiply(self.objects[played.place], *palette.get(joint)?))
        })?;
        let world = DMat4::from_cols_array_2d(&world.map(|column| column.map(f64::from)));
        let position = world.transform_point3(DVec3::ZERO);
        let forward = world.transform_vector3(DVec3::NEG_Z).normalize_or_zero();
        let up = world.transform_vector3(DVec3::Y).normalize_or_zero();
        if forward == DVec3::ZERO || up == DVec3::ZERO {
            return None;
        }
        Some(Eye {
            position: position.to_array(),
            target: (position + forward).to_array(),
            up: up.to_array(),
            fov_y_deg: f64::from(fov),
            shift: [0.0, 0.0],
            ortho_height: None,
        })
    }
}
