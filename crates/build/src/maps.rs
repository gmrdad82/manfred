use crate::Error;
use manfred_glb::{Anisotropy, Image, Look, Looks, Map, Mime, Sampler, Sheen, UvTransform};
use manfred_recipe::Recipe;
use pfx_materials::Material;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const LARGEST_IMAGE: u32 = 16384;
const GUTTER: u32 = 16;
const DETAIL_SET: u32 = 1;

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
    ])))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

fn png_size(bytes: &[u8]) -> Option<[u32; 2]> {
    (bytes.get(12..16)? == b"IHDR").then_some([be32(bytes, 16)?, be32(bytes, 20)?])
}

fn jpeg_size(bytes: &[u8]) -> Option<[u32; 2]> {
    let mut at = 2;
    loop {
        while *bytes.get(at)? == 0xff && *bytes.get(at + 1)? == 0xff {
            at += 1;
        }
        if *bytes.get(at)? != 0xff {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            return Some([be16(bytes, at + 7)?, be16(bytes, at + 5)?]);
        }
        at += 2 + be16(bytes, at + 2)? as usize;
    }
}

pub fn image_fits(size: [u32; 2]) -> Result<(), String> {
    let cell = |side: u32| u64::from(side.div_ceil(GUTTER) * GUTTER) + 2 * u64::from(GUTTER);
    if size.contains(&0)
        || size
            .iter()
            .any(|&side| cell(side) > u64::from(LARGEST_IMAGE))
    {
        return Err(format!(
            "a {}x{} image does not fit the {LARGEST_IMAGE}x{LARGEST_IMAGE} card atlas with its gutters",
            size[0], size[1]
        ));
    }
    Ok(())
}

pub fn image(bytes: Vec<u8>) -> Result<Image, String> {
    let mime = Mime::of(&bytes).ok_or("not a PNG or JPEG file")?;
    let size = match mime {
        Mime::Png => png_size(&bytes),
        Mime::Jpeg => jpeg_size(&bytes),
    }
    .ok_or_else(|| format!("{} with no readable size in its header", mime.text()))?;
    image_fits(size)?;
    Ok(Image {
        bytes: bytes.into(),
        mime,
        size,
    })
}

fn sampler(texture: &gltf::Texture) -> Option<Sampler> {
    let sampler = texture.sampler();
    sampler.index()?;
    Some(Sampler {
        mag: sampler.mag_filter().map(|f| f.as_gl_enum()),
        min: sampler.min_filter().map(|f| f.as_gl_enum()),
        wrap: [sampler.wrap_s().as_gl_enum(), sampler.wrap_t().as_gl_enum()],
    })
}

fn source(
    texture: &gltf::Texture,
    buffers: &[gltf::buffer::Data],
    folder: &Path,
) -> Result<Option<Vec<u8>>, String> {
    Ok(match texture.source().source() {
        gltf::image::Source::View { view, .. } => {
            let Some(data) = buffers.get(view.buffer().index()) else {
                return Ok(None);
            };
            data.0
                .get(view.offset()..view.offset() + view.length())
                .map(<[u8]>::to_vec)
        }
        gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => {
            let file = folder.join(uri);
            Some(std::fs::read(&file).map_err(|e| format!("cannot read {uri}: {e}"))?)
        }
        gltf::image::Source::Uri { .. } => None,
    })
}

fn kept(
    texture: gltf::Texture,
    tex_coord: u32,
    scale: f32,
    transform: Option<UvTransform>,
    buffers: &[gltf::buffer::Data],
    folder: &Path,
) -> Result<Option<Map>, String> {
    let Some(bytes) = source(&texture, buffers, folder)? else {
        return Ok(None);
    };
    if Mime::of(&bytes).is_none() {
        return Ok(None);
    }
    Ok(Some(Map {
        image: image(bytes)?,
        tex_coord,
        scale,
        sampler: sampler(&texture),
        transform,
    }))
}

fn transform(info: &gltf::texture::Info) -> Option<UvTransform> {
    info.texture_transform().map(|t| UvTransform {
        offset: t.offset(),
        rotation: t.rotation(),
        scale: t.scale(),
        tex_coord: t.tex_coord(),
    })
}

pub(crate) fn look(
    material: &gltf::Material,
    buffers: &[gltf::buffer::Data],
    folder: &Path,
) -> Result<Look, String> {
    let pbr = material.pbr_metallic_roughness();
    let info = |info: Option<gltf::texture::Info>| -> Result<Option<Map>, String> {
        match info {
            Some(info) => kept(
                info.texture(),
                info.tex_coord(),
                1.0,
                transform(&info),
                buffers,
                folder,
            ),
            None => Ok(None),
        }
    };
    Ok(Look {
        base_color: Some(pbr.base_color_factor()),
        metallic: Some(pbr.metallic_factor()),
        roughness: Some(pbr.roughness_factor()),
        emission: Some(material.emissive_factor()).filter(|e| *e != [0.0; 3]),
        sheen: None,
        anisotropy: None,
        base: info(pbr.base_color_texture())?,
        metal_roughness: info(pbr.metallic_roughness_texture())?,
        normal: match material.normal_texture() {
            Some(t) => kept(t.texture(), t.tex_coord(), t.scale(), None, buffers, folder)?,
            None => None,
        },
        occlusion: match material.occlusion_texture() {
            Some(t) => kept(
                t.texture(),
                t.tex_coord(),
                t.strength(),
                None,
                buffers,
                folder,
            )?,
            None => None,
        },
        detail_normal: None,
    })
}

#[derive(Default)]
struct Pool {
    files: BTreeMap<PathBuf, Image>,
    images: Vec<Image>,
}

impl Pool {
    fn intern(&mut self, image: &Image) -> Image {
        match self.images.iter().find(|known| known.bytes == image.bytes) {
            Some(known) => known.clone(),
            None => {
                self.images.push(image.clone());
                image.clone()
            }
        }
    }

    fn file(&mut self, base: &Path, name: &str, key: &str, path: &str) -> Result<Image, Error> {
        let file = base.join(path);
        if let Some(found) = self.files.get(&file) {
            return Ok(found.clone());
        }
        let fail = |text: String| Error::new(format!("materials.{name}.{key}"), text);
        let bytes = std::fs::read(&file).map_err(|e| fail(format!("cannot read {path}: {e}")))?;
        if Mime::of(&bytes) != Some(Mime::Png) {
            return Err(fail(format!("{path} is not a PNG")));
        }
        let found = image(bytes).map_err(|e| fail(format!("{path}: {e}")))?;
        let found = self.intern(&found);
        self.files.insert(file, found.clone());
        Ok(found)
    }
}

fn set(map: &mut Option<Map>, scale: Option<f64>) {
    if let (Some(map), Some(scale)) = (map.as_mut(), scale) {
        map.scale = scale as f32;
    }
}

fn factors(look: &mut Look, material: &Material) {
    let [r, g, b] = material.base;
    look.base_color = Some([r, g, b, 1.0]);
    look.metallic = Some(material.metalness);
    look.roughness = Some(material.roughness);
    look.emission = (material.emission != [0.0; 3]).then_some(material.emission);
    look.sheen = (!material.fabric.off()).then_some(Sheen {
        color: material.fabric.color,
        roughness: material.fabric.roughness,
    });
    look.anisotropy = (!material.anisotropy.off()).then_some(Anisotropy {
        strength: material.anisotropy.strength,
        rotation: material.anisotropy.rotation,
    });
}

pub(crate) fn resolve(
    recipe: &Recipe,
    base: &Path,
    imported: Looks,
    materials: &BTreeMap<String, serde_json::Value>,
) -> Result<Looks, Error> {
    let mut pool = Pool::default();
    let mut looks = imported;
    for look in looks.values_mut() {
        for map in [
            &mut look.base,
            &mut look.normal,
            &mut look.metal_roughness,
            &mut look.occlusion,
            &mut look.detail_normal,
        ]
        .into_iter()
        .flatten()
        {
            map.image = pool.intern(&map.image);
        }
    }
    for (name, textures) in &recipe.textures {
        let look = looks.entry(name.clone()).or_default();
        let mut read = |key: &str, path: &Option<String>| -> Result<Option<Map>, Error> {
            path.as_deref()
                .map(|path| pool.file(base, name, key, path).map(Map::new))
                .transpose()
        };
        if let Some(map) = read("base_map", &textures.base_map)? {
            look.base = Some(map);
        }
        if let Some(map) = read("normal_map", &textures.normal_map)? {
            look.normal = Some(map);
        }
        if let Some(map) = read("roughness_map", &textures.roughness_map)? {
            look.metal_roughness = Some(map);
        }
        if let Some(map) = read("occlusion_map", &textures.occlusion_map)? {
            look.occlusion = Some(map);
        }
        if let Some(map) = read("detail_normal_map", &textures.detail_normal_map)? {
            look.detail_normal = Some(Map {
                tex_coord: DETAIL_SET,
                ..map
            });
        }
        set(&mut look.normal, textures.normal_scale);
        set(&mut look.occlusion, textures.occlusion_strength);
        set(&mut look.detail_normal, textures.detail_normal_scale);
    }
    for (name, value) in materials {
        let Ok(material) = serde_json::from_value::<Material>(value.clone()) else {
            continue;
        };
        let lobed = !material.fabric.off() || !material.anisotropy.off();
        if !lobed && !looks.get(name).is_some_and(Look::has_maps) {
            continue;
        }
        factors(looks.entry(name.clone()).or_default(), &material);
    }
    looks.retain(|_, look| look.carried());
    Ok(looks)
}
