use crate::memory::{Memory, digest};
use crate::parts::Piece;
use crate::vector::{add, cross, dot, scale, sub};
use crate::{Error, Eye};
use manfred_recipe::{Kind, Recipe};
use pfx_geom::mesh::Mesh as Geometry;
use pfx_live::content_atlas::{
    AtlasError, AtlasImage, ContentAtlas, DEFAULT_BLEED_LEVEL, cell_size,
};
use pfx_live::frame::{Frame, InstanceSurface, Matrix};
use pfx_live::maps::ContentFormat;
use pfx_load::{ColorSpace, Image, Pixels};
use pfx_materials::{Blend, ContentLayer, Material};
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const SLOT: usize = 0;
pub const FORMAT: ContentFormat = ContentFormat::Srgb8;
pub const SMALLEST: u32 = 256;
pub const LARGEST: u32 = 16384;
pub const PLACEHOLDER: u32 = 64;
const SQUARE: u32 = 8;
const BORDER: u32 = 2;
const GREYS: [u8; 3] = [0x5a, 0x80, 0x9a];
const PARALLEL: f64 = 1e-9;
const NO_PLANES: [[f32; 4]; 2] = [[0.0; 4]; 2];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub centre: [f64; 3],
    pub up: [f64; 3],
    pub normal: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dress {
    pub surface: InstanceSurface,
    pub card: Option<Card>,
}

impl Dress {
    pub fn face(&self) -> Option<Face> {
        self.card.and_then(|card| card.face)
    }
}

pub struct Atlas {
    pub key: u64,
    pub size: [u32; 2],
    pub bytes: u64,
    packed: Option<ContentAtlas>,
    pictures: Vec<Arc<Picture>>,
    memory: Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub key: u64,
    pub size: [u32; 2],
    pub bytes: u64,
}

impl Atlas {
    #[cfg(test)]
    pub fn packed(atlas: ContentAtlas, key: u64) -> Atlas {
        Atlas {
            key,
            size: atlas.size(),
            bytes: atlas.vram_bytes(),
            packed: Some(atlas),
            pictures: Vec::new(),
            memory: Memory::default(),
        }
    }

    #[cfg(test)]
    pub fn atlas(&self) -> Option<&ContentAtlas> {
        self.packed.as_ref()
    }

    pub fn ensure(&mut self) -> Result<&mut ContentAtlas, Error> {
        match &mut self.packed {
            Some(atlas) => Ok(atlas),
            empty => {
                self.memory.packed();
                Ok(empty.insert(pack(&self.pictures)?.0))
            }
        }
    }

    pub fn shown(&self) -> Shown {
        Shown {
            key: self.key,
            size: self.size,
            bytes: self.bytes,
        }
    }

    pub fn show(&mut self, frame: &Frame) -> Result<(), Error> {
        let atlas = self.ensure()?;
        atlas
            .allocate(frame, SLOT)
            .and_then(|()| atlas.upload(frame, SLOT))
            .map(|_| ())
            .map_err(|e| Error(format!("card images: {e}")))
    }
}

pub struct Dressed {
    pub pieces: Vec<Piece>,
    pub dress: Vec<Dress>,
    pub atlas: Option<Atlas>,
}

struct Quad {
    corner: [f64; 3],
    across: [f64; 3],
    down: [f64; 3],
    normal: [f64; 3],
}

#[derive(Clone)]
pub(crate) struct Layout {
    pub key: u64,
    pub size: [u32; 2],
    pub bytes: u64,
    pub surfaces: Vec<Option<InstanceSurface>>,
}

pub fn dress(recipe: &Recipe, pieces: Vec<Piece>, memory: &Memory) -> Result<Dressed, Error> {
    let (pictures, hashes, shown) = found(recipe, &pieces, memory)?;
    let (atlas, surfaces) = if pictures.is_empty() {
        memory.forget_layout();
        (None, Vec::new())
    } else {
        let key = key(&pictures, &hashes);
        let (packed, layout) = match memory.layout(key) {
            Some(layout) => (None, layout),
            None => {
                memory.packed();
                let (atlas, ids) = pack(&pictures)?;
                let layout = Layout {
                    key,
                    size: atlas.size(),
                    bytes: atlas.vram_bytes(),
                    surfaces: ids.iter().map(|&id| atlas.surface(id)).collect(),
                };
                memory.keep_layout(&layout);
                (Some(atlas), layout)
            }
        };
        let atlas = Atlas {
            key,
            size: layout.size,
            bytes: layout.bytes,
            packed,
            pictures: pictures.clone(),
            memory: memory.clone(),
        };
        (Some(atlas), layout.surfaces)
    };
    let least = least(&pictures);
    let mut dress = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        let mut worn = Dress {
            surface: InstanceSurface {
                clip: clips(recipe, piece.object),
                ..InstanceSurface::default()
            },
            card: None,
        };
        if let (Some(card), Some(_)) = (card(recipe, piece, &shown, &least), &atlas) {
            if let Some(Some(placed)) = surfaces.get(card.picture) {
                worn.surface.uv_offset = placed.uv_offset;
                worn.surface.uv_scale = placed.uv_scale;
                worn.surface.crop = placed.crop;
            }
            worn.surface.cutout = card.cuts;
            worn.card = Some(card);
        }
        dress.push(worn);
    }
    Ok(Dressed {
        pieces,
        dress,
        atlas,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Card {
    pub picture: usize,
    pub cutoff: f32,
    pub cuts: bool,
    pub face: Option<Face>,
}

pub struct Cards {
    pub pictures: Vec<Arc<Picture>>,
    pub cards: Vec<Option<Card>>,
}

impl Cards {
    pub fn of(recipe: &Recipe, pieces: &[Piece]) -> Result<Cards, Error> {
        let (pictures, _, shown) = found(recipe, pieces, &Memory::default())?;
        let least = least(&pictures);
        let cards = pieces
            .iter()
            .map(|piece| card(recipe, piece, &shown, &least))
            .collect();
        Ok(Cards { pictures, cards })
    }
}

fn card(
    recipe: &Recipe,
    piece: &Piece,
    shown: &BTreeMap<usize, usize>,
    least: &[u8],
) -> Option<Card> {
    let Kind::Card {
        face_camera,
        alpha_cutoff,
        ..
    } = &recipe.objects.get(piece.object)?.kind
    else {
        return None;
    };
    let &picture = shown.get(&piece.object)?;
    let cutoff = alpha_cutoff.clamp(0.0, 1.0) as f32;
    Some(Card {
        picture,
        cutoff,
        cuts: cutoff > 0.0 && f32::from(least[picture]) / 255.0 < cutoff,
        face: quad(&piece.geometry)
            .as_ref()
            .filter(|_| *face_camera)
            .map(face),
    })
}

type Found = (Vec<Arc<Picture>>, Vec<u64>, BTreeMap<usize, usize>);

fn found(recipe: &Recipe, pieces: &[Piece], memory: &Memory) -> Result<Found, Error> {
    let base = recipe.base.clone().unwrap_or_default();
    let mut found: BTreeMap<Option<PathBuf>, usize> = BTreeMap::new();
    let mut pictures: Vec<Arc<Picture>> = Vec::new();
    let mut hashes: Vec<u64> = Vec::new();
    let mut shown: BTreeMap<usize, usize> = BTreeMap::new();
    for piece in pieces {
        let Some(object) = recipe.objects.get(piece.object) else {
            continue;
        };
        let Kind::Card { image, .. } = &object.kind else {
            continue;
        };
        if shown.contains_key(&piece.object) {
            continue;
        }
        let source = image.as_ref().map(|path| base.join(path));
        let at = match found.get(&source) {
            Some(&at) => at,
            None => {
                let (hash, picture) = match &source {
                    Some(file) => {
                        let bytes = std::fs::read(file).map_err(|e| {
                            Error(format!(
                                "object[{}].image: cannot read {}: {e}",
                                object.name,
                                file.display()
                            ))
                        })?;
                        memory.picture(file, &bytes, decode).map_err(|e| {
                            Error(format!(
                                "object[{}].image: {}: {e}",
                                object.name,
                                file.display()
                            ))
                        })?
                    }
                    None => {
                        let picture = placeholder();
                        (digest(&picture.texels), Arc::new(picture))
                    }
                };
                pictures.push(picture);
                hashes.push(hash);
                found.insert(source, pictures.len() - 1);
                pictures.len() - 1
            }
        };
        shown.insert(piece.object, at);
    }
    let used: Vec<&Path> = found.keys().filter_map(|file| file.as_deref()).collect();
    memory.retain(&used);
    Ok((pictures, hashes, shown))
}

pub fn layered(material: Material) -> Material {
    Material {
        content_layer: ContentLayer {
            slot: SLOT as i32,
            blend: Blend::Over,
            ..ContentLayer::default()
        },
        ..material
    }
}

pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        let image = Image::png(bytes)?;
        let texels = match (image.pixels, image.space) {
            (Pixels::Eight(texels), ColorSpace::Srgb) => texels,
            (Pixels::Eight(texels), ColorSpace::Linear) => texels
                .chunks_exact(4)
                .flat_map(|t| {
                    let [r, g, b] = [t[0], t[1], t[2]].map(|v| encode(f64::from(v) / 255.0));
                    [r, g, b, t[3]]
                })
                .collect(),
            (Pixels::Sixteen(samples), space) => samples
                .chunks_exact(4)
                .flat_map(|t| {
                    let [r, g, b, a] = [t[0], t[1], t[2], t[3]].map(|v| f64::from(v) / 65535.0);
                    let colour = |v: f64| match space {
                        ColorSpace::Srgb => (v * 255.0).round() as u8,
                        ColorSpace::Linear => encode(v),
                    };
                    [colour(r), colour(g), colour(b), (a * 255.0).round() as u8]
                })
                .collect(),
        };
        return Ok(Picture {
            width: image.width,
            height: image.height,
            texels,
        });
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let options = zune_jpeg::zune_core::options::DecoderOptions::default()
            .jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::RGBA)
            .set_use_unsafe(false);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(
            zune_jpeg::zune_core::bytestream::ZCursor::new(bytes),
            options,
        );
        let texels = decoder.decode().map_err(|e| format!("jpeg: {e}"))?;
        let info = decoder.info().ok_or("jpeg: no header")?;
        let (width, height) = (u32::from(info.width), u32::from(info.height));
        if width == 0 || height == 0 || texels.len() != width as usize * height as usize * 4 {
            return Err(format!(
                "jpeg: a {width}x{height} image decoded to {} bytes",
                texels.len()
            ));
        }
        return Ok(Picture {
            width,
            height,
            texels,
        });
    }
    Err("not a PNG or JPEG file".into())
}

pub fn placeholder() -> Picture {
    let side = PLACEHOLDER;
    let mut texels = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let edge = x < BORDER || y < BORDER || x >= side - BORDER || y >= side - BORDER;
            let grey = if edge {
                GREYS[0]
            } else if (x / SQUARE + y / SQUARE).is_multiple_of(2) {
                GREYS[1]
            } else {
                GREYS[2]
            };
            texels.extend([grey, grey, grey, 255]);
        }
    }
    Picture {
        width: side,
        height: side,
        texels,
    }
}

pub fn premultiplied(picture: &Picture) -> Vec<u8> {
    let linear: [f64; 256] = std::array::from_fn(|v| linear(v as u8));
    let mut rows: Vec<Option<[u8; 256]>> = vec![None; 256];
    picture
        .texels
        .chunks_exact(4)
        .flat_map(|t| {
            if t[3] == 255 {
                return [t[0], t[1], t[2], t[3]];
            }
            let row = rows[usize::from(t[3])].get_or_insert_with(|| {
                let a = f64::from(t[3]) / 255.0;
                std::array::from_fn(|v| encode(linear[v] * a))
            });
            let [r, g, b] = [t[0], t[1], t[2]].map(|v| row[usize::from(v)]);
            [r, g, b, t[3]]
        })
        .collect()
}

pub fn pack<P: Borrow<Picture>>(pictures: &[P]) -> Result<(ContentAtlas, Vec<usize>), Error> {
    let texels: Vec<Vec<u8>> = pictures
        .iter()
        .map(|picture| premultiplied(picture.borrow()))
        .collect();
    let images: Vec<AtlasImage> = pictures
        .iter()
        .zip(&texels)
        .map(|(picture, texels)| AtlasImage {
            width: picture.borrow().width,
            height: picture.borrow().height,
            texels,
        })
        .collect();
    let mut side = first_side(pictures);
    loop {
        let mut atlas = ContentAtlas::new(side, side, FORMAT, DEFAULT_BLEED_LEVEL)
            .map_err(|e| Error(format!("card images: {e}")))?;
        match atlas.add_all(&images) {
            Ok(ids) => return Ok((atlas, ids)),
            Err(AtlasError::Full { .. } | AtlasError::TooLarge { .. }) if side < LARGEST => {
                side *= 2;
            }
            Err(error) => return Err(Error(format!("card images: {error}"))),
        }
    }
}

fn first_side<P: Borrow<Picture>>(pictures: &[P]) -> u32 {
    let cells: Vec<[u32; 2]> = pictures
        .iter()
        .map(|p| cell_size([p.borrow().width, p.borrow().height], DEFAULT_BLEED_LEVEL))
        .collect();
    let area: u64 = cells
        .iter()
        .map(|c| u64::from(c[0]) * u64::from(c[1]))
        .sum();
    let widest = cells.iter().flatten().copied().max().unwrap_or(1);
    let mut side = SMALLEST;
    while side < LARGEST && (side < widest || u64::from(side) * u64::from(side) < area) {
        side *= 2;
    }
    side
}

fn key(pictures: &[Arc<Picture>], hashes: &[u64]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for (picture, hash) in pictures.iter().zip(hashes) {
        picture.width.hash(&mut hasher);
        picture.height.hash(&mut hasher);
        hash.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn clips(recipe: &Recipe, object: usize) -> [[f32; 4]; 2] {
    let Some(name) = recipe.objects.get(object).map(|o| o.name.as_str()) else {
        return NO_PLANES;
    };
    let plane = |p: Option<[f64; 4]>| p.map_or([0.0; 4], |p| p.map(|v| v as f32));
    recipe
        .movers
        .iter()
        .find(|mover| mover.owns(name))
        .map_or(NO_PLANES, |mover| [plane(mover.clip), plane(mover.clip2)])
}

pub fn facing(face: &Face, eye: &Eye) -> Matrix {
    let toward = if eye.ortho_height.is_some() {
        sub(eye.position, eye.target)
    } else {
        sub(eye.position, face.centre)
    };
    let flat = sub(toward, scale(face.up, dot(toward, face.up)));
    let length = dot(flat, flat).sqrt();
    if length < PARALLEL {
        return turn(face.centre, face.up, 0.0);
    }
    let wanted = scale(flat, 1.0 / length);
    let angle = dot(cross(face.normal, wanted), face.up).atan2(dot(face.normal, wanted));
    turn(face.centre, face.up, angle)
}

fn turn(centre: [f64; 3], axis: [f64; 3], angle: f64) -> Matrix {
    let (s, c) = angle.sin_cos();
    let [x, y, z] = axis;
    let rows = [
        [
            c + x * x * (1.0 - c),
            x * y * (1.0 - c) - z * s,
            x * z * (1.0 - c) + y * s,
        ],
        [
            y * x * (1.0 - c) + z * s,
            c + y * y * (1.0 - c),
            y * z * (1.0 - c) - x * s,
        ],
        [
            z * x * (1.0 - c) - y * s,
            z * y * (1.0 - c) + x * s,
            c + z * z * (1.0 - c),
        ],
    ];
    let moved: [f64; 3] = std::array::from_fn(|r| dot(rows[r], centre));
    let t = sub(centre, moved);
    let mut out = [[0.0f32; 4]; 4];
    for col in 0..3 {
        for row in 0..3 {
            out[col][row] = rows[row][col] as f32;
        }
    }
    out[3] = [t[0] as f32, t[1] as f32, t[2] as f32, 1.0];
    out
}

fn quad(geometry: &Geometry) -> Option<Quad> {
    let at = |score: fn([f32; 2]) -> f32| {
        geometry
            .uvs
            .iter()
            .enumerate()
            .max_by(|a, b| score(*a.1).total_cmp(&score(*b.1)).then(b.0.cmp(&a.0)))
            .map(|(index, _)| index)
    };
    let top_left = at(|uv| -(uv[0] + uv[1]))?;
    let top_right = at(|uv| uv[0] - uv[1])?;
    let bottom_left = at(|uv| uv[1] - uv[0])?;
    let point = |index: usize| geometry.positions[index].map(f64::from);
    let corner = point(top_left);
    let across = sub(point(top_right), corner);
    let down = sub(point(bottom_left), corner);
    let given = geometry
        .normals
        .get(top_left)
        .map(|n| n.map(f64::from))
        .unwrap_or([0.0; 3]);
    let normal = if dot(given, given) > PARALLEL {
        given
    } else {
        cross(down, across)
    };
    let length = dot(normal, normal).sqrt();
    if length < PARALLEL || dot(across, across) < PARALLEL || dot(down, down) < PARALLEL {
        return None;
    }
    Some(Quad {
        corner,
        across,
        down,
        normal: scale(normal, 1.0 / length),
    })
}

fn face(quad: &Quad) -> Face {
    let up = unit(scale(quad.down, -1.0));
    let normal = unit(sub(quad.normal, scale(up, dot(quad.normal, up))));
    Face {
        centre: add(quad.corner, scale(add(quad.across, quad.down), 0.5)),
        up,
        normal,
    }
}

fn least(pictures: &[Arc<Picture>]) -> Vec<u8> {
    pictures
        .iter()
        .map(|picture| {
            picture
                .texels
                .chunks_exact(4)
                .map(|t| t[3])
                .min()
                .unwrap_or(u8::MAX)
        })
        .collect()
}

pub fn linear(v: u8) -> f64 {
    let c = f64::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let c = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let length = dot(a, a).sqrt().max(PARALLEL);
    scale(a, 1.0 / length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::parse;
    use std::path::Path;

    fn solid(width: u32, height: u32, texel: [u8; 4]) -> Picture {
        Picture {
            width,
            height,
            texels: texel.repeat((width * height) as usize),
        }
    }

    fn disc(side: u32) -> Picture {
        let mut texels = Vec::new();
        let r = f64::from(side) * 0.5;
        for y in 0..side {
            for x in 0..side {
                let dx = f64::from(x) + 0.5 - r;
                let dy = f64::from(y) + 0.5 - r;
                let inside = dx * dx + dy * dy < r * r * 0.5;
                texels.extend(if inside {
                    [0, 255, 0, 255]
                } else {
                    [0, 0, 0, 0]
                });
            }
        }
        Picture {
            width: side,
            height: side,
            texels,
        }
    }

    fn png(picture: &Picture) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, picture.width, picture.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&picture.texels).unwrap();
        writer.finish().unwrap();
        out
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/cards-unit")
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn built(dir: &Path, text: &str) -> (Recipe, Vec<Piece>) {
        let file = dir.join("scene.toml");
        std::fs::write(&file, text).unwrap();
        let recipe = manfred_recipe::load(&file).unwrap().recipe;
        let parts = manfred_build::build(&recipe, dir).unwrap().parts;
        let pieces = crate::parts::pieces(&parts);
        (recipe, pieces)
    }

    const HEAD: &str = "manfred = 1\nname = \"cards\"\n";

    fn card(name: &str, source: &str, extra: &str) -> String {
        format!(
            "\n[[object]]\nname = \"{name}\"\nkind = \"card\"\nsize = [1.0, 0.5]\n{source}\n{extra}"
        )
    }

    #[test]
    fn three_images_pack_inside_the_atlas_without_overlap() {
        let pictures = [
            solid(300, 120, [255, 0, 0, 255]),
            solid(64, 64, [0, 255, 0, 255]),
            solid(17, 200, [0, 0, 255, 255]),
        ];
        let (atlas, ids) = pack(&pictures).unwrap();
        assert_eq!(atlas.size(), [512, 512]);
        let crops: Vec<[f32; 4]> = ids.iter().map(|&id| atlas.crop(id).unwrap()).collect();
        for (crop, picture) in crops.iter().zip(&pictures) {
            assert!(crop.iter().all(|v| (0.0..=1.0).contains(v)), "{crop:?}");
            assert!(crop[0] < crop[2] && crop[1] < crop[3], "{crop:?}");
            let width = (crop[2] - crop[0]) * 512.0;
            let height = (crop[3] - crop[1]) * 512.0;
            assert_eq!(
                [width, height],
                [picture.width, picture.height].map(|v| v as f32)
            );
        }
        for a in 0..3 {
            for b in a + 1..3 {
                let [p, q] = [crops[a], crops[b]];
                let apart = p[2] <= q[0] || q[2] <= p[0] || p[3] <= q[1] || q[3] <= p[1];
                assert!(apart, "{p:?} overlaps {q:?}");
            }
        }
        let cells: Vec<[u32; 4]> = ids
            .iter()
            .map(|&id| atlas.placement(id).unwrap().cell)
            .collect();
        for a in 0..3 {
            for b in a + 1..3 {
                let [p, q] = [cells[a], cells[b]];
                let apart = p[0] + p[2] <= q[0]
                    || q[0] + q[2] <= p[0]
                    || p[1] + p[3] <= q[1]
                    || q[1] + q[3] <= p[1];
                assert!(apart, "the gutters of {p:?} and {q:?} overlap");
            }
        }
        let again = pack(&pictures).unwrap();
        assert_eq!(again.1, ids);
        assert_eq!(again.0.crop(ids[2]), atlas.crop(ids[2]));
    }

    #[test]
    fn an_atlas_grows_to_hold_what_it_is_given() {
        let small = pack(&[solid(8, 8, [1, 2, 3, 255])]).unwrap().0;
        assert_eq!(small.size(), [SMALLEST, SMALLEST]);
        let many: Vec<Picture> = (0..9).map(|_| solid(512, 512, [9, 9, 9, 255])).collect();
        let big = pack(&many).unwrap().0;
        assert_eq!(big.size(), [2048, 2048]);
        assert!(pack(&[solid(LARGEST, 4, [0, 0, 0, 255])]).is_err());
    }

    #[test]
    fn premultiplying_darkens_by_alpha_in_linear_light() {
        let picture = Picture {
            width: 3,
            height: 1,
            texels: vec![200, 100, 50, 255, 255, 255, 255, 0, 255, 255, 255, 128],
        };
        let out = premultiplied(&picture);
        assert_eq!(&out[..4], &[200, 100, 50, 255]);
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
        assert_eq!(out[11], 128);
        assert_eq!(out[8], encode(128.0 / 255.0));
        assert!((150..200).contains(&out[8]), "{}", out[8]);
    }

    #[test]
    fn png_and_jpeg_cards_decode_to_srgb_texels() {
        let picture = Picture {
            width: 2,
            height: 1,
            texels: vec![10, 20, 30, 255, 40, 50, 60, 7],
        };
        assert_eq!(decode(&png(&picture)).unwrap(), picture);
        let jpeg = include_bytes!("../tests/fixtures/halves.jpg");
        let halves = decode(jpeg).unwrap();
        assert_eq!([halves.width, halves.height], [16, 8]);
        let texel = |x: usize, y: usize| &halves.texels[(y * 16 + x) * 4..][..4];
        assert!(
            texel(2, 4)[0] > 240 && texel(2, 4)[2] < 15,
            "{:?}",
            texel(2, 4)
        );
        assert!(
            texel(13, 4)[2] > 240 && texel(13, 4)[0] < 15,
            "{:?}",
            texel(13, 4)
        );
        assert_eq!(texel(13, 4)[3], 255);
        assert!(decode(b"GIF89a").is_err());
    }

    #[test]
    fn a_slot_card_gets_the_placeholder() {
        let dir = scratch("slot");
        std::fs::write(dir.join("red.png"), png(&solid(32, 16, [255, 0, 0, 255]))).unwrap();
        let text = format!(
            "{HEAD}{}{}{}\n[[object]]\nname = \"floor\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\n",
            card("photo", "image = \"red.png\"", ""),
            card("feed", "slot = \"page\"", ""),
            card("other", "slot = \"video\"", "at = [2.0, 0.0, 0.0]"),
        );
        let (recipe, pieces) = built(&dir, &text);
        let dressed = dress(&recipe, pieces, &Memory::default()).unwrap();
        let atlas = dressed.atlas.unwrap();
        assert_eq!(atlas.atlas().unwrap().len(), 2);
        assert_eq!(dressed.dress.len(), 4);
        let [photo, feed, other, floor] = [0, 1, 2, 3].map(|k| dressed.dress[k]);
        assert!(photo.card.is_some() && feed.card.is_some() && other.card.is_some());
        assert!(floor.card.is_none());
        assert_eq!(feed.surface, other.surface);
        assert_ne!(feed.surface.crop, photo.surface.crop);
        let expected = placeholder();
        let id = (0..2)
            .find(|&id| {
                let rect = atlas.atlas().unwrap().placement(id).unwrap().rect;
                [rect[2], rect[3]] == [expected.width, expected.height]
            })
            .unwrap();
        assert_eq!(feed.surface, atlas.atlas().unwrap().surface(id).unwrap());
        assert_eq!(floor.surface, InstanceSurface::default());
        let middle = &expected.texels[((32 * 64 + 32) * 4) as usize..][..4];
        assert_eq!(middle[0], middle[1]);
        assert_eq!(middle[1], middle[2]);
        assert_eq!(middle[3], 255);
        let greys: std::collections::BTreeSet<u8> =
            expected.texels.chunks_exact(4).map(|t| t[0]).collect();
        assert_eq!(greys.len(), 3);
    }

    #[test]
    fn a_card_image_is_decoded_once_across_rebuilds_and_again_when_its_bytes_change() {
        let dir = scratch("kept");
        let file = dir.join("red.png");
        std::fs::write(&file, png(&solid(32, 16, [255, 0, 0, 255]))).unwrap();
        let text = format!("{HEAD}{}", card("photo", "image = \"red.png\"", ""));
        let memory = Memory::default();
        let (recipe, pieces) = built(&dir, &text);
        let first = dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.decodes(), 1);
        let moved = text.replace("size = [1.0, 0.5]", "size = [2.0, 0.5]");
        let (recipe, pieces) = built(&dir, &moved);
        let second = dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.decodes(), 1);
        assert_eq!(first.atlas.unwrap().key, second.atlas.unwrap().key);
        std::fs::write(&file, png(&solid(32, 16, [0, 0, 255, 255]))).unwrap();
        let (recipe, pieces) = built(&dir, &moved);
        let third = dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.decodes(), 2);
        let (recipe, pieces) = built(&dir, &moved);
        let again = dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.decodes(), 2);
        assert_eq!(third.atlas.unwrap().key, again.atlas.unwrap().key);
    }

    #[test]
    fn unchanged_cards_are_not_packed_again_and_changed_ones_are() {
        let dir = scratch("packs");
        let file = dir.join("red.png");
        std::fs::write(&file, png(&solid(32, 16, [255, 0, 0, 255]))).unwrap();
        let text = format!("{HEAD}{}", card("photo", "image = \"red.png\"", ""));
        let memory = Memory::default();
        let (recipe, pieces) = built(&dir, &text);
        let first = dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.packs(), 1);
        let moved = text.replace("size = [1.0, 0.5]", "size = [2.0, 0.5]");
        let (recipe, pieces) = built(&dir, &moved);
        let second = dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.packs(), 1);
        let (first, mut second) = (first.atlas.unwrap(), second.atlas.unwrap());
        assert!(second.atlas().is_none());
        assert_eq!(first.key, second.key);
        assert_eq!(first.shown(), second.shown());
        second.ensure().unwrap();
        assert_eq!(memory.packs(), 2);
        std::fs::write(&file, png(&solid(32, 16, [0, 0, 255, 255]))).unwrap();
        let (recipe, pieces) = built(&dir, &moved);
        let third = dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.packs(), 3);
        assert_ne!(third.atlas.unwrap().key, first.key);
    }

    #[test]
    fn a_one_number_edit_on_a_few_cards_dresses_without_packing() {
        let dir = scratch("timed");
        let mut text = HEAD.to_string();
        for k in 0..4u32 {
            let texels = (0..1024u32 * 1024)
                .flat_map(|t| [(t % 251) as u8, (t / 1024) as u8, k as u8 * 60, 255])
                .collect();
            let picture = Picture {
                width: 1024,
                height: 1024,
                texels,
            };
            std::fs::write(dir.join(format!("{k}.png")), png(&picture)).unwrap();
            text.push_str(&card(
                &format!("c{k}"),
                &format!("image = \"{k}.png\""),
                &format!("at = [{k}.0, 0.0, 0.0]"),
            ));
        }
        let memory = Memory::default();
        let (recipe, pieces) = built(&dir, &text);
        let since = std::time::Instant::now();
        dress(&recipe, pieces, &memory).unwrap();
        let first = since.elapsed();
        let moved = text.replace("at = [1.0, 0.0, 0.0]", "at = [1.5, 0.0, 0.0]");
        let (recipe, pieces) = built(&dir, &moved);
        let since = std::time::Instant::now();
        dress(&recipe, pieces, &memory).unwrap();
        let edit = since.elapsed();
        println!(
            "four 1024x1024 cards: first dress {} ms, a one-number edit {} ms",
            first.as_millis(),
            edit.as_millis()
        );
    }

    #[test]
    fn a_card_image_the_recipe_drops_is_forgotten() {
        let dir = scratch("dropped");
        std::fs::write(dir.join("red.png"), png(&solid(8, 8, [255, 0, 0, 255]))).unwrap();
        let with = format!("{HEAD}{}", card("photo", "image = \"red.png\"", ""));
        let memory = Memory::default();
        let (recipe, pieces) = built(&dir, &with);
        dress(&recipe, pieces, &memory).unwrap();
        let (recipe, pieces) = built(&dir, HEAD);
        dress(&recipe, pieces, &memory).unwrap();
        let (recipe, pieces) = built(&dir, &with);
        dress(&recipe, pieces, &memory).unwrap();
        assert_eq!(memory.decodes(), 2);
    }

    #[test]
    fn a_missing_image_names_its_card() {
        let dir = scratch("missing");
        let text = format!("{HEAD}{}", card("photo", "image = \"gone.png\"", ""));
        let (recipe, pieces) = built(&dir, &text);
        let error = dress(&recipe, pieces, &Memory::default()).err().unwrap();
        assert!(
            error.0.starts_with("object[photo].image: cannot read"),
            "{error}"
        );
    }

    #[test]
    fn a_cutout_card_stays_one_quad_and_marks_cutout() {
        let dir = scratch("cutout");
        std::fs::write(dir.join("disc.png"), png(&disc(32))).unwrap();
        std::fs::write(dir.join("solid.png"), png(&solid(32, 32, [9, 9, 9, 255]))).unwrap();
        std::fs::write(dir.join("clear.png"), png(&solid(8, 8, [0, 0, 0, 0]))).unwrap();
        let text = format!(
            "{HEAD}{}{}{}{}",
            card("cut", "image = \"disc.png\"", "alpha_cutoff = 0.5"),
            card("open", "image = \"disc.png\"", "alpha_cutoff = 0.0"),
            card("whole", "image = \"solid.png\"", ""),
            card("clear", "image = \"clear.png\"", "alpha_cutoff = 0.5"),
        );
        let (recipe, pieces) = built(&dir, &text);
        let dressed = dress(&recipe, pieces, &Memory::default()).unwrap();
        assert_eq!(dressed.pieces.len(), 4);
        for piece in &dressed.pieces {
            assert_eq!(piece.geometry.indices.len(), 6);
        }
        assert!(dressed.dress[0].surface.cutout);
        assert!(!dressed.dress[1].surface.cutout);
        assert!(!dressed.dress[2].surface.cutout);
        assert!(dressed.dress[3].surface.cutout);
        assert_eq!(dressed.dress[0].card.unwrap().cutoff, 0.5);
        assert_eq!(dressed.dress[1].card.unwrap().cutoff, 0.0);
        assert!(dressed.dress[2].card.unwrap().cutoff > 0.0);
        assert!(!dressed.dress[2].card.unwrap().cuts);
    }

    #[test]
    fn a_face_camera_card_turns_about_its_centre_to_the_camera() {
        let dir = scratch("face");
        std::fs::write(dir.join("red.png"), png(&solid(4, 4, [255, 0, 0, 255]))).unwrap();
        let text = format!(
            "{HEAD}{}",
            card(
                "sprite",
                "image = \"red.png\"",
                "face_camera = true\nat = [1.0, 2.0, -1.0]\nrotate = [0.0, 30.0, 0.0]"
            ),
        );
        let (recipe, pieces) = built(&dir, &text);
        let dressed = dress(&recipe, pieces, &Memory::default()).unwrap();
        let face = dressed.dress[0].card.unwrap().face.unwrap();
        for k in 0..3 {
            assert!(
                (face.centre[k] - [1.0, 2.0, -1.0][k]).abs() < 1e-6,
                "{face:?}"
            );
            assert!((face.up[k] - [0.0, 1.0, 0.0][k]).abs() < 1e-6, "{face:?}");
        }
        let eye = |position: [f64; 3]| Eye {
            position,
            target: [1.0, 2.0, -1.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 40.0,
            shift: [0.0, 0.0],
            ortho_height: None,
        };
        let apply = |m: &Matrix, p: [f64; 3], w: f32| -> [f64; 3] {
            let v = [p[0] as f32, p[1] as f32, p[2] as f32, w];
            std::array::from_fn(|r| f64::from((0..4).map(|c| m[c][r] * v[c]).sum::<f32>()))
        };
        for position in [[6.0, 5.0, -1.0], [1.0, 0.0, -9.0], [-3.0, 2.5, 3.0]] {
            let model = facing(&face, &eye(position));
            let centre = apply(&model, face.centre, 1.0);
            let normal = apply(&model, face.normal, 0.0);
            let up = apply(&model, face.up, 0.0);
            let toward = [position[0] - 1.0, 0.0, position[2] + 1.0];
            let length = dot(toward, toward).sqrt();
            for k in 0..3 {
                assert!((centre[k] - face.centre[k]).abs() < 1e-5, "{centre:?}");
                assert!((up[k] - face.up[k]).abs() < 1e-5, "{up:?}");
                assert!(
                    (normal[k] - toward[k] / length).abs() < 1e-5,
                    "{normal:?} at {position:?}"
                );
            }
        }
        let ortho = Eye {
            ortho_height: Some(2.0),
            target: [0.0, 0.0, 0.0],
            ..eye([0.0, 0.0, 5.0])
        };
        let normal = apply(&facing(&face, &ortho), face.normal, 0.0);
        assert!((normal[2] - 1.0).abs() < 1e-5, "{normal:?}");
        let above = facing(&face, &eye([1.0, 9.0, -1.0]));
        assert_eq!(
            apply(&above, face.normal, 0.0).map(|v| (v * 1e5).round()),
            face.normal.map(|v| (v * 1e5).round())
        );
    }

    #[test]
    fn a_mover_part_gets_its_movers_planes_in_world_space() {
        let text = "manfred = 1\nname = \"m\"\n\n[plate]\nwidth = 200\nheight = 100\nmetres_per_pixel = 0.01\norigin = \"top_left\"\n\n[[object]]\nname = \"mv_strip\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n[[object]]\nname = \"mv_strip__card0\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n[[object]]\nname = \"mv_stripe\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n[[object]]\nname = \"loose\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n[[object]]\nname = \"knob\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\n\n[[mover]]\ngroup = \"strip\"\npivot = [0.0, 0.0, 0.0]\naxis = [0.0, 1.0, 0.0]\ntravel = 0.01\nclip_px = [0, 1, 0, 20]\nclip2_px = [0, -1, 0, 80]\n\n[[mover]]\ngroup = \"turn\"\npivot = [0.0, 0.0, 0.0]\naxis = [0.0, 1.0, 0.0]\ntravel = 0.01\nclip = [1.0, 0.0, 0.0, 0.25]\nparts = [\"knob\"]\n";
        let recipe = parse(text).unwrap();
        let strip = &recipe.movers[0];
        let planes = clips(&recipe, 0);
        assert_eq!(
            planes,
            [
                strip.clip.unwrap().map(|v| v as f32),
                strip.clip2.unwrap().map(|v| v as f32)
            ]
        );
        assert_eq!(planes[0][..3], [0.0, 1.0, 0.0]);
        assert_eq!(planes[1][..3], [0.0, -1.0, 0.0]);
        assert_eq!(clips(&recipe, 1), planes);
        assert_eq!(clips(&recipe, 2), NO_PLANES);
        assert_eq!(clips(&recipe, 3), NO_PLANES);
        assert_eq!(clips(&recipe, 4), [[1.0, 0.0, 0.0, 0.25], [0.0; 4]]);
        assert_eq!(clips(&recipe, 9), NO_PLANES);
        let surface = InstanceSurface {
            clip: planes,
            ..InstanceSurface::default()
        };
        let top = strip.clip.unwrap()[3];
        let bottom = -strip.clip2.unwrap()[3];
        assert!(top > bottom, "{top} {bottom}");
        assert!(surface.cuts([0.0, (top + 0.05) as f32, 0.0]));
        assert!(surface.cuts([0.0, (bottom - 0.05) as f32, 0.0]));
        assert!(!surface.cuts([0.0, ((top + bottom) * 0.5) as f32, 0.0]));
    }

    #[test]
    fn a_card_material_carries_the_atlas_layer() {
        let material = layered(Material::default());
        assert_eq!(material.content_layer.slot, SLOT as i32);
        assert_eq!(material.content_layer.blend, Blend::Over);
        assert_eq!(material.base, Material::default().base);
    }

    #[test]
    fn the_toyboxs_mover_pieces_carry_their_planes() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/toybox/toybox.toml");
        let recipe = manfred_recipe::load(&path).unwrap().recipe;
        let parts = manfred_build::build(&recipe, path.parent().unwrap())
            .unwrap()
            .parts;
        let pieces = crate::parts::pieces(&parts);
        let movers = pieces
            .iter()
            .filter(|piece| {
                let name = &recipe.objects[piece.object].name;
                recipe.movers.iter().any(|mover| mover.owns(name))
            })
            .count();
        let dressed = dress(&recipe, pieces, &Memory::default()).unwrap();
        let clipped = dressed
            .dress
            .iter()
            .filter(|worn| worn.surface.clipped())
            .count();
        println!(
            "the toybox: {} pieces, {movers} of movers, {clipped} clipped",
            dressed.dress.len()
        );
        assert!(dressed.atlas.is_none());
        assert_eq!((movers, clipped), (7, 3));
    }
}
