use pfx_gpu::{Gpu, wgpu};
use pfx_live::frame::InstanceSurface;
use pfx_live::maps::ContentFormat;
use pfx_live::renderer::{Picked, Renderer};
use pfx_materials::{Content, ContentLayer, Material};

pub trait Painter {
    fn layer(&self, material: &Material, filled: &[bool]) -> ContentLayer;

    fn surface(&self, content: Content, base: InstanceSurface) -> InstanceSurface;

    fn warm(&self, material: &Material) -> Vec<ContentLayer>;

    fn paint(&mut self, job: Paint<'_>) -> Result<(), String>;
}

pub struct Paint<'a> {
    pub gpu: &'a Gpu,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub slot: usize,
    pub source: &'a wgpu::Texture,
    pub target: &'a wgpu::TextureView,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoContent;

impl Painter for NoContent {
    fn layer(&self, _: &Material, _: &[bool]) -> ContentLayer {
        ContentLayer::default()
    }

    fn surface(&self, _: Content, base: InstanceSurface) -> InstanceSurface {
        base
    }

    fn warm(&self, _: &Material) -> Vec<ContentLayer> {
        Vec::new()
    }

    fn paint(&mut self, _: Paint<'_>) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Paint,
    Copy,
}

#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub texture: &'a wgpu::Texture,
    pub revision: u64,
    pub format: ContentFormat,
    pub fill: Fill,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slots {
    seen: Vec<Option<u64>>,
    filled: Vec<bool>,
    pub uploads: u64,
}

impl Slots {
    pub fn filled(&self) -> &[bool] {
        &self.filled
    }

    pub fn dirty(&self, sources: &[Source<'_>]) -> Vec<bool> {
        sources
            .iter()
            .enumerate()
            .map(|(k, source)| self.seen.get(k).copied().flatten() != Some(source.revision))
            .collect()
    }

    pub fn feed<P: Painter>(
        &mut self,
        renderer: &Renderer,
        painter: &mut P,
        sources: &[Source<'_>],
    ) -> Result<(), String> {
        let dirty = self.dirty(sources);
        if !dirty.iter().any(|&d| d) {
            return Ok(());
        }
        let gpu = renderer.gpu();
        let frame = renderer.frame();
        for (slot, source) in sources.iter().enumerate() {
            if dirty[slot] {
                let texture = source.texture;
                frame.set_content(slot, texture.width(), texture.height(), source.format)?;
            }
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("runtime live content"),
            });
        for (slot, source) in sources.iter().enumerate() {
            if !dirty[slot] || source.fill != Fill::Paint {
                continue;
            }
            {
                let slots = frame.content();
                let target = slots
                    .get(slot)
                    .and_then(|content| content.level(0))
                    .ok_or_else(|| format!("content slot {slot} has no first level"))?;
                painter.paint(Paint {
                    gpu,
                    encoder: &mut encoder,
                    slot,
                    source: source.texture,
                    target,
                })?;
            }
            frame.regenerate_content(&mut encoder, slot)?;
        }
        for (slot, source) in sources.iter().enumerate() {
            if dirty[slot] && source.fill == Fill::Copy {
                frame.copy_content(&mut encoder, slot, source.texture)?;
            }
        }
        gpu.queue.submit([encoder.finish()]);
        self.seen = sources.iter().map(|s| Some(s.revision)).collect();
        if self.filled.len() < sources.len() {
            self.filled.resize(sources.len(), false);
        }
        for (filled, dirty) in self.filled.iter_mut().zip(&dirty) {
            *filled |= *dirty;
        }
        self.uploads += 1;
        Ok(())
    }

    pub fn materials<P: Painter>(&self, painter: &P, materials: &[Material]) -> Vec<Material> {
        materials
            .iter()
            .map(|material| Material {
                content_layer: painter.layer(material, &self.filled),
                ..*material
            })
            .collect()
    }

    pub fn surfaces<P: Painter>(
        &self,
        painter: &P,
        contents: &[Content],
        surfaces: &[InstanceSurface],
    ) -> Vec<InstanceSurface> {
        contents
            .iter()
            .zip(surfaces)
            .map(|(&content, &base)| painter.surface(content, base))
            .collect()
    }
}

pub fn dresses<P: Painter>(
    painter: &P,
    current: Vec<Material>,
    materials: &[Material],
) -> Vec<Vec<Material>> {
    let warm: Vec<Vec<ContentLayer>> = materials.iter().map(|m| painter.warm(m)).collect();
    let count = warm.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = vec![current];
    for k in 0..count {
        let dressed: Vec<Material> = materials
            .iter()
            .zip(&warm)
            .map(|(material, layers)| Material {
                content_layer: layers.get(k).copied().unwrap_or_default(),
                ..*material
            })
            .collect();
        if !out.contains(&dressed) {
            out.push(dressed);
        }
    }
    out
}

pub fn pixel(cursor: (f32, f32), size: (u32, u32)) -> Option<(u32, u32)> {
    let (x, y) = (cursor.0.floor(), cursor.1.floor());
    (x >= 0.0 && y >= 0.0 && (x as u32) < size.0 && (y as u32) < size.1)
        .then_some((x as u32, y as u32))
}

pub fn id_of(object: u32) -> u32 {
    object + 1
}

pub fn object_of(id: u32) -> Option<u32> {
    id.checked_sub(1)
}

pub trait Picker {
    type Hit;

    fn objects(&self) -> usize;

    fn pick(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<Self::Hit>;

    fn pick_object(&self, origin: [f32; 3], dir: [f32; 3], object: u32) -> Option<Self::Hit>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Ids,
    Missed,
    Stale,
}

pub fn resolve<P: Picker>(
    picked: Option<Picked>,
    at: Option<(u32, u32)>,
    picker: &P,
    origin: [f32; 3],
    dir: [f32; 3],
) -> (Option<P::Hit>, Route) {
    let current = picked.filter(|p| at == Some((p.x, p.y)));
    let Some(picked) = current else {
        return (picker.pick(origin, dir), Route::Stale);
    };
    let Some(object) = object_of(picked.id).filter(|&o| (o as usize) < picker.objects()) else {
        return (None, Route::Ids);
    };
    match picker.pick_object(origin, dir, object) {
        Some(hit) => (Some(hit), Route::Ids),
        None => (picker.pick(origin, dir), Route::Missed),
    }
}
