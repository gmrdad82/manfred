use crate::Error;
use crate::memory::digest;
use crate::parts::Piece;
use manfred_build::{Look, Map, Mime};
use pfx_load::{ColorSpace, Image, Pixels};
use pfx_materials::{Maps, Material};
use std::hash::{DefaultHasher, Hash, Hasher};

pub const LARGEST_MAP: u32 = 4096;
const NEUTRAL: [[u8; 4]; 4] = [
    [255, 255, 255, 255],
    [128, 128, 255, 255],
    [255, 255, 255, 255],
    [255, 255, 255, 255],
];

#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    pub key: u64,
    names: Vec<String>,
    looks: Vec<Look>,
}

fn drawn(map: &Option<Map>) -> Option<&Map> {
    map.as_ref().filter(|map| {
        map.tex_coord == 0
            && map
                .transform
                .and_then(|t| t.tex_coord)
                .is_none_or(|set| set == 0)
    })
}

fn kinds(look: &Look) -> [Option<&Map>; 3] {
    [
        drawn(&look.base),
        drawn(&look.normal),
        drawn(&look.metal_roughness),
    ]
}

impl Set {
    pub fn of(pieces: &[Piece]) -> Option<Set> {
        let mut names: Vec<String> = Vec::new();
        let mut looks = Vec::new();
        for piece in pieces {
            let Some(look) = &piece.look else {
                continue;
            };
            if kinds(look).iter().all(Option::is_none) || names.contains(&piece.material) {
                continue;
            }
            names.push(piece.material.clone());
            looks.push(look.clone());
        }
        if names.is_empty() {
            return None;
        }
        let mut hasher = DefaultHasher::new();
        for (name, look) in names.iter().zip(&looks) {
            name.hash(&mut hasher);
            for map in kinds(look) {
                map.map(|m| (digest(&m.image.bytes), m.scale.to_bits()))
                    .hash(&mut hasher);
            }
        }
        Some(Set {
            key: hasher.finish(),
            names,
            looks,
        })
    }

    pub fn dress(&self, names: &[String], materials: &mut [Material]) {
        for (name, material) in names.iter().zip(materials) {
            let Some(layer) = self.names.iter().position(|known| known == name) else {
                continue;
            };
            let normal = drawn(&self.looks[layer].normal).map_or(1.0, |m| m.scale.clamp(0.0, 1.0));
            material.maps = Maps {
                layer: layer as f32,
                tile: 1.0,
                normal,
                albedo: 1.0,
            };
        }
    }

    pub fn images(&self) -> Result<[Vec<Image>; 4], Error> {
        let mut decoded: [Vec<Option<Image>>; 4] = Default::default();
        for (name, look) in self.names.iter().zip(&self.looks) {
            let [base, normal, metal_roughness] = kinds(look);
            let open = |map: Option<&Map>, kind: &str| {
                map.map(decode)
                    .transpose()
                    .map_err(|e| Error(format!("materials.{name}: the {kind} map: {e}")))
            };
            let base = open(base, "base")?;
            let normal = open(normal, "normal")?;
            let packed = open(metal_roughness, "roughness")?;
            decoded[0].push(base);
            decoded[1].push(normal);
            decoded[2].push(packed.as_ref().map(|image| channel(image, 1)));
            decoded[3].push(packed.as_ref().map(|image| channel(image, 2)));
        }
        Ok(std::array::from_fn(|kind| {
            layers(&decoded[kind], NEUTRAL[kind])
        }))
    }
}

fn decode(map: &Map) -> Result<Image, String> {
    let image = match map.image.mime {
        Mime::Png => Image::png(&map.image.bytes)?,
        Mime::Jpeg => {
            let picture = crate::cards::decode(&map.image.bytes)?;
            Image {
                width: picture.width,
                height: picture.height,
                space: ColorSpace::Srgb,
                pixels: Pixels::Eight(picture.texels),
            }
        }
    };
    let expected = image.width as usize * image.height as usize * 4;
    let pixels = match image.pixels {
        Pixels::Eight(texels) => texels,
        Pixels::Sixteen(samples) => samples.iter().map(|v| (v >> 8) as u8).collect(),
    };
    if image.width == 0 || image.height == 0 || pixels.len() != expected {
        return Err(format!(
            "a {}x{} image decoded to {} bytes",
            image.width,
            image.height,
            pixels.len()
        ));
    }
    Ok(Image {
        pixels: Pixels::Eight(pixels),
        ..image
    })
}

fn texels(image: &Image) -> &[u8] {
    match &image.pixels {
        Pixels::Eight(texels) => texels,
        Pixels::Sixteen(_) => &[],
    }
}

fn channel(image: &Image, at: usize) -> Image {
    let pixels = texels(image)
        .chunks_exact(4)
        .flat_map(|t| [t[at], t[at], t[at], 255])
        .collect();
    Image {
        width: image.width,
        height: image.height,
        space: ColorSpace::Linear,
        pixels: Pixels::Eight(pixels),
    }
}

fn layers(found: &[Option<Image>], neutral: [u8; 4]) -> Vec<Image> {
    let side = |pick: fn(&Image) -> u32| {
        found
            .iter()
            .flatten()
            .map(pick)
            .max()
            .unwrap_or(1)
            .min(LARGEST_MAP)
    };
    let (width, height) = (side(|i| i.width), side(|i| i.height));
    found
        .iter()
        .map(|image| match image {
            Some(image) if image.width == width && image.height == height => image.clone(),
            Some(image) => resized(image, width, height),
            None => Image {
                width,
                height,
                space: ColorSpace::Linear,
                pixels: Pixels::Eight(neutral.repeat(width as usize * height as usize)),
            },
        })
        .collect()
}

fn resized(image: &Image, width: u32, height: u32) -> Image {
    let source = texels(image);
    let (sw, sh) = (image.width as usize, image.height as usize);
    let at = |x: usize, y: usize, c: usize| f32::from(source[(y * sw + x) * 4 + c]);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        let fy = ((y as f32 + 0.5) * sh as f32 / height as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..width {
            let fx =
                ((x as f32 + 0.5) * sw as f32 / width as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
                let bottom = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
                pixels.push((top * (1.0 - ty) + bottom * ty).round() as u8);
            }
        }
    }
    Image {
        width,
        height,
        space: image.space,
        pixels: Pixels::Eight(pixels),
    }
}
