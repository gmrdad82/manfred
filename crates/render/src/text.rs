use crate::staging::Staging;
use manfred_scene::{Error, IDENTITY, Laid};
use pfx_live::frame::Matrix;
use pfx_live::text::RichQuad;
use pfx_materials::{ContentLayer, Material};
use pfx_text::Atlas;
use pfx_trace::stage::{ContentKind, Mesh, Placement};
use pfx_trace::text::{self, Carrier, Fit, Glyph, Laid as Laidout};
use std::ops::Deref;

#[derive(Clone, Debug, PartialEq)]
pub enum Way {
    Carrier {
        lettering: Ink,
        carrier: Box<Carrier>,
        model: Matrix,
        lit: bool,
    },
    Fit {
        piece: usize,
        lettering: Ink,
        fit: Fit,
    },
    Over {
        piece: usize,
        lettering: Ink,
        fit: Fit,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Letter {
    pub object: usize,
    pub name: String,
    pub way: Way,
}

impl Letter {
    fn ink(&self) -> &text::Lettering {
        match &self.way {
            Way::Carrier { lettering, .. }
            | Way::Fit { lettering, .. }
            | Way::Over { lettering, .. } => lettering,
        }
    }

    pub fn bytes(&self) -> usize {
        self.ink()
            .shown()
            .records()
            .map(|text| text.records.len() * 16)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lettering {
    pub letters: Vec<Letter>,
}

impl Lettering {
    pub fn of(staging: &Staging) -> Result<Lettering, Error> {
        let meshes = staging.meshes();
        let mut taken = vec![false; staging.pieces.len()];
        let mut letters = Vec::new();
        for item in &staging.text.items {
            let Some((model, lit)) = item.world() else {
                continue;
            };
            let glyphs = glyphs(&item.quads);
            if !glyphs.iter().any(|g| g.rect[2] > 0.0 && g.rect[3] > 0.0) {
                continue;
            }
            let fail = |e: String| Error(format!("object[{}]: the traced text: {e}", item.name));
            let targets: Vec<usize> = match item.on {
                Some(on) if lit => (0..staging.pieces.len())
                    .filter(|&piece| staging.pieces[piece].object == on && !taken[piece])
                    .collect(),
                _ => Vec::new(),
            };
            let way = way(
                staging,
                &meshes,
                &targets,
                (&item.atlas, &glyphs),
                (model, lit),
            )
            .map_err(fail)?;
            if let Way::Fit { piece, .. } | Way::Over { piece, .. } = way {
                taken[piece] = true;
            }
            letters.push(Letter {
                object: item.object,
                name: item.name.clone(),
                way,
            });
        }
        Ok(Lettering { letters })
    }

    pub fn stage<'a>(
        &'a self,
        meshes: &mut Vec<Mesh<'a>>,
        placements: &mut Vec<Placement<'a>>,
        materials: &mut Vec<Material>,
    ) {
        for letter in &self.letters {
            match &letter.way {
                Way::Carrier {
                    lettering,
                    carrier,
                    model,
                    lit,
                } => {
                    let mesh = meshes.len() as u32;
                    meshes.push(carrier.mesh());
                    let material = interned(materials, text::material(*lit));
                    placements.push(Placement {
                        casts_shadow: false,
                        content: Some(lettering.content()),
                        ..Placement::new(mesh, *model, material)
                    });
                }
                Way::Fit {
                    piece,
                    lettering,
                    fit,
                } => {
                    let placement = &mut placements[*piece];
                    let inked = Material {
                        content_layer: ContentLayer::default(),
                        ..materials[placement.material as usize]
                    };
                    placement.material = interned(materials, inked);
                    placement.content = Some(fit.content(lettering));
                }
                Way::Over {
                    piece,
                    lettering,
                    fit,
                } => {
                    let placement = &mut placements[*piece];
                    if let Some(picture) = placement.content {
                        placement.content = Some(fit.over(&picture, lettering).unwrap_or(picture));
                    }
                }
            }
        }
    }
}

fn way(
    staging: &Staging,
    meshes: &[Mesh<'_>],
    targets: &[usize],
    (atlas, glyphs): (&Atlas, &[Glyph]),
    (model, lit): (Matrix, bool),
) -> Result<Way, String> {
    let lettering = Ink(text::lettering(atlas, glyphs)?);
    let fits = |piece: usize| text::fit(&meshes[piece], IDENTITY, model).ok();
    for &piece in targets {
        let Some(card) = staging.cards[piece] else {
            continue;
        };
        let Some(fit) = fits(piece) else {
            continue;
        };
        let picture = staging.content(&card);
        if card.face.is_none() && fit.elsewhere(&meshes[piece], &picture).is_empty() {
            fit.over(&picture, &lettering)?;
            return Ok(Way::Over {
                piece,
                lettering,
                fit,
            });
        }
    }
    for &piece in targets {
        if staging.cards[piece].is_some() {
            continue;
        }
        let Some(fit) = fits(piece) else {
            continue;
        };
        let clear = {
            let content = fit.content(&lettering);
            fit.elsewhere(&meshes[piece], &content).is_empty()
        };
        if clear {
            return Ok(Way::Fit {
                piece,
                lettering,
                fit,
            });
        }
    }
    let carrier = Box::new(lettering.carrier());
    Ok(Way::Carrier {
        lettering,
        carrier,
        model,
        lit,
    })
}

#[derive(Clone, Debug)]
pub struct Ink(text::Lettering);

impl PartialEq for Ink {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = (&self.0, &other.0);
        a.rect == b.rect
            && a.density == b.density
            && a.glyphs == b.glyphs
            && a.atlas.channels == b.atlas.channels
            && a.atlas.levels == b.atlas.levels
    }
}

impl Laidout for Ink {
    fn rect(&self) -> [f32; 4] {
        self.0.rect()
    }

    fn kind(&self) -> ContentKind<'_> {
        self.0.kind()
    }
}

impl Deref for Ink {
    type Target = text::Lettering;

    fn deref(&self) -> &text::Lettering {
        &self.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Overlay {
    pub laid: Laid,
}

impl Overlay {
    pub fn of(laid: &Laid) -> Overlay {
        Overlay {
            laid: Laid {
                items: laid
                    .items
                    .iter()
                    .filter(|item| item.world().is_none())
                    .cloned()
                    .collect(),
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.laid.items.is_empty()
    }

    pub fn paint(&self, pixels: &mut [[f32; 4]], size: [u32; 2]) -> Result<(), String> {
        for item in &self.laid.items {
            if let Some(quads) = item.overlaid(size) {
                text::overlay(pixels, size[0], size[1], &item.atlas, &glyphs(&quads))
                    .map_err(|e| format!("object[{}]: the overlay text: {e}", item.name))?;
            }
        }
        Ok(())
    }
}

pub fn glyphs(quads: &[RichQuad]) -> Vec<Glyph> {
    quads
        .iter()
        .map(|q| Glyph {
            rect: q.rect,
            uv: q.uv,
            color: [q.color[0], q.color[1], q.color[2], q.color[3] * q.alpha],
            clip: q.clip,
            turn: q.turn,
        })
        .collect()
}

fn interned(materials: &mut Vec<Material>, material: Material) -> u32 {
    let at = materials
        .iter()
        .position(|m| *m == material)
        .unwrap_or_else(|| {
            materials.push(material);
            materials.len() - 1
        });
    at as u32
}
