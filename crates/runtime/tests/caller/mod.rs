use manfred_runtime::{
    Bird, Canopy, Framing, Lighting, Look, MoteLook, Paint, Painter, Scene, SteamLook, Wind,
};
use pfx_geom::tree::{Tree, TreeSpec};
use pfx_live::frame::InstanceSurface;
use pfx_live::renderer::Finish;
use pfx_materials::{Content, ContentLayer, ContentLook, Material};
use pfx_post::{Bloom, Chain, Pass, Saturation, Tone, Vignette, Warmth};

pub const PLATE: (u32, u32) = (1600, 1000);
pub const NEAR: f32 = 0.4;
pub const FAR: f32 = 6.0;
pub const KEEP_REFLECTION: [&str; 2] = ["card", "film"];
pub const PRINT_SLOT: usize = 0;
pub const SCREEN_SLOT: usize = 1;
const PHOTO_CROP: [f32; 4] = [0.1, 0.2, 0.5, 0.25];
pub const LOOK: ContentLook = ContentLook {
    ink_roughness: 0.547_722_6,
    ink_emboss_coated: 0.22,
    ink_emboss_bare: -0.12,
    photo_gain: 1.08,
    screen_gain: 1.6,
    photo_window: [0.0, 0.0, 1.0, 1.0],
};

const BIRD_VERTICES: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.5],
    [0.0, 0.0, -0.5],
    [-1.0, 0.0, -0.3],
    [1.0, 0.0, -0.3],
];
const BIRD_INDICES: [u32; 6] = [0, 2, 1, 0, 1, 3];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dress {
    pub gain: f32,
}

pub fn slot(content: Content) -> Option<usize> {
    match content {
        Content::None => None,
        Content::Ink | Content::Decal | Content::Scroll | Content::Print | Content::Field => {
            Some(PRINT_SLOT)
        }
        Content::Photo | Content::Screen => Some(SCREEN_SLOT),
    }
}

pub fn layer(material: &Material, dress: &Dress, filled: &[bool]) -> ContentLayer {
    let Some(slot) = slot(material.content).filter(|&s| filled.get(s).copied().unwrap_or(false))
    else {
        return ContentLayer::default();
    };
    if material.content == Content::Screen && dress.gain <= 0.0 {
        return ContentLayer::default();
    }
    let layer = ContentLayer::from_kind(material.content, material.clearcoat, slot as i32, &LOOK);
    match material.content {
        Content::Screen => ContentLayer {
            strength: dress.gain,
            ..layer
        },
        _ => layer,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlotPainter {
    pub dress: Dress,
}

impl Painter for SlotPainter {
    fn layer(&self, material: &Material, filled: &[bool]) -> ContentLayer {
        layer(material, &self.dress, filled)
    }

    fn surface(&self, content: Content, base: InstanceSurface) -> InstanceSurface {
        match content {
            Content::Photo => {
                let [x, y, w, h] = PHOTO_CROP;
                InstanceSurface {
                    uv_offset: [x, y],
                    uv_scale: [w, h],
                    ..base
                }
            }
            _ => base,
        }
    }

    fn warm(&self, material: &Material) -> Vec<ContentLayer> {
        vec![
            layer(material, &Dress::default(), &[false; 2]),
            layer(material, &Dress { gain: 1.0 }, &[true; 2]),
        ]
    }

    fn paint(&mut self, _: Paint<'_>) -> Result<(), String> {
        Err("the test caller paints no slot".into())
    }
}

pub fn tree() -> Tree {
    Tree::new(
        7,
        TreeSpec {
            height: 0.5,
            trunk_radius: 0.01,
            crown_width: 0.4,
            crown_height: 0.3,
            blossom_density: 0.0,
            leaf_density: 1.0,
            ..TreeSpec::plum()
        },
        0.001,
    )
}

pub fn lighting() -> Lighting {
    Lighting {
        hour: 16.0,
        framing: Framing {
            caster_margin: 0.3,
            framed: 1.3,
            behind: 0.2,
            before: 0.35,
        },
        reflection_occlusion: 0.5,
        wind: Wind {
            strength: 0.3,
            direction: [1.0, 0.0, 0.2],
        },
        steam: SteamLook {
            color: [0.9, 0.9, 0.92],
            rise: 0.2,
            clock: 0.5,
        },
        motes: MoteLook {
            cell: 0.012,
            share: 0.05,
            spread: 2.0,
            glow: 180.0,
        },
        canopy: Some(Canopy {
            tree: tree(),
            sharpness: 0.3,
            bird: Some(Bird {
                vertices: BIRD_VERTICES.to_vec(),
                indices: BIRD_INDICES.to_vec(),
                period: 31.0,
                start: 6.0,
                span: 3.4,
                half_span: 0.07,
                distance: 1.0,
            }),
        }),
    }
}

pub fn look(scene: &Scene) -> Look {
    let chain = Chain {
        passes: vec![
            Pass::Exposure(scene.exposure),
            Pass::Bloom(Bloom::pyramid(
                scene.grade.bloom,
                0.9,
                0.7,
                0.6,
                0.15,
                12,
                50.0,
                2,
            )),
            Pass::Vignette(Vignette::power(scene.grade.vignette, 2.4, 0.8, 1.28)),
            Pass::Saturation(Saturation::rec709(scene.grade.saturation)),
            Pass::Warmth(Warmth::linear(scene.grade.warmth)),
            Pass::Tone(Tone::aces()),
        ],
        ..Chain::new()
    };
    Look::of(scene, NEAR, FAR, chain, Finish::Standard)
}
