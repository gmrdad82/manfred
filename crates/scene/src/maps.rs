use crate::Error;
use crate::memory::digest;
use crate::parts::Piece;
use manfred_build::{Look, Map, Mime};
use pfx_load::scene::textures;
use pfx_load::{ColorSpace, Image, Pixels};
use pfx_materials::{Material, Textures, UvTransform};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

const KINDS: [(&str, ColorSpace); 3] = [
    ("base", ColorSpace::Srgb),
    ("normal", ColorSpace::Linear),
    ("roughness", ColorSpace::Linear),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    pub key: u64,
    names: Vec<String>,
    looks: Vec<Look>,
}

type Lists<'a> = [Vec<(&'a str, &'a Map)>; 3];

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

fn main(look: &Look) -> UvTransform {
    kinds(look)
        .into_iter()
        .flatten()
        .next()
        .and_then(|map| map.transform)
        .map_or_else(UvTransform::default, |t| UvTransform {
            set: 0,
            offset: t.offset,
            rotation: t.rotation,
            scale: t.scale,
        })
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
                map.map(|m| {
                    (
                        digest(&m.image.bytes),
                        m.scale.to_bits(),
                        m.transform.map(|t| {
                            [t.offset[0], t.offset[1], t.rotation, t.scale[0], t.scale[1]]
                                .map(f32::to_bits)
                        }),
                    )
                })
                .hash(&mut hasher);
            }
        }
        Some(Set {
            key: hasher.finish(),
            names,
            looks,
        })
    }

    fn plan(&self) -> (Vec<[i32; 3]>, Lists<'_>) {
        let mut lists: Lists<'_> = Default::default();
        let mut slots = Vec::with_capacity(self.looks.len());
        for (name, look) in self.names.iter().zip(&self.looks) {
            let mut slot = [-1; 3];
            for (kind, map) in kinds(look).into_iter().enumerate() {
                let Some(map) = map else {
                    continue;
                };
                let list = &mut lists[kind];
                let at = match list.iter().position(|(_, seen)| seen.image == map.image) {
                    Some(at) => at,
                    None => {
                        list.push((name.as_str(), map));
                        list.len() - 1
                    }
                };
                slot[kind] = at as i32;
            }
            slots.push(slot);
        }
        (slots, lists)
    }

    pub fn dress(&self, names: &[String], materials: &mut [Material]) {
        let (slots, _) = self.plan();
        for (name, material) in names.iter().zip(materials) {
            let Some(at) = self.names.iter().position(|known| known == name) else {
                continue;
            };
            let look = &self.looks[at];
            let [base, normal, orm] = slots[at];
            material.textures = Textures {
                base,
                normal,
                orm,
                normal_scale: kinds(look)[1].map_or(1.0, |m| m.scale),
                occlusion: 0.0,
                main: main(look),
                ..Textures::default()
            };
        }
    }

    pub fn images(&self) -> Result<textures::Textures, Error> {
        let (_, lists) = self.plan();
        let mut out: [Vec<Arc<Image>>; 3] = Default::default();
        for ((list, found), (kind, space)) in lists.iter().zip(&mut out).zip(KINDS) {
            for (name, map) in list {
                let image = decode(map, space)
                    .map_err(|e| Error(format!("materials.{name}: the {kind} map: {e}")))?;
                found.push(Arc::new(image));
            }
        }
        let [colour, linear, orm] = out;
        Ok(textures::Textures {
            colour,
            linear,
            orm,
        })
    }
}

fn decode(map: &Map, space: ColorSpace) -> Result<Image, String> {
    let image = match map.image.mime {
        Mime::Png => Image::png(&map.image.bytes)?,
        Mime::Jpeg => {
            let picture = crate::cards::decode(&map.image.bytes)?;
            Image {
                width: picture.width,
                height: picture.height,
                space,
                pixels: Pixels::Eight(picture.texels),
            }
        }
    };
    Ok(Image { space, ..image })
}
