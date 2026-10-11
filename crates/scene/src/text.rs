use crate::Error;
use manfred_build::{Part, local_matrix, world};
use manfred_glb::DMat4;
use manfred_recipe::{Kind, Object, Recipe, TextSpace as Space};
use pfx_gpu::Gpu;
use pfx_live::frame::{Matrix, multiply};
use pfx_live::renderer::{Text, TextQuadItem};
use pfx_live::text::{GpuAtlas, RichQuad, TextSpace, rich_quads};
use pfx_text::{Anchor, Atlas, Face, Representation, Span, TextEngine};
use std::collections::BTreeMap;
use std::path::Path;

use fonts::{BOLD, REGULAR};

pub mod fonts {
    pub static REGULAR: &[u8] =
        include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");
    pub static BOLD: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFont-Bold.ttf");
}

pub const UNITS_PER_METRE: f32 = 8192.0;
pub const LINE: f32 = 1.25;
pub const LIFT: f64 = 0.0005;
const CLIP: [f32; 4] = [-1e9, -1e9, 1e9, 1e9];
const REGULAR_WEIGHT: u16 = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    World { model: DMat4, lit: bool },
    Overlay { screen: [f32; 2] },
}

#[derive(Clone, Debug)]
pub struct Laying {
    pub name: String,
    pub object: usize,
    pub atlas: Atlas,
    pub quads: Vec<RichQuad>,
    pub place: Place,
    pub on: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Laid {
    pub items: Vec<Laying>,
}

struct Fonts {
    engine: TextEngine,
    mono: String,
    files: BTreeMap<String, String>,
}

impl Fonts {
    fn new() -> Result<Fonts, Error> {
        let fail = |e| Error(format!("the embedded mono font: {e:?}"));
        let mut engine = TextEngine::new(REGULAR).map_err(fail)?;
        engine.register_font(BOLD).map_err(fail)?;
        let mono = family_name(REGULAR)
            .ok_or_else(|| Error("the embedded mono font has no family name".into()))?;
        Ok(Fonts {
            engine,
            mono,
            files: BTreeMap::new(),
        })
    }

    fn family(&mut self, base: &Path, family: Option<&str>) -> Result<String, String> {
        let path = match family {
            None => return Ok(self.mono.clone()),
            Some(name) if name == manfred_recipe::MONO => return Ok(self.mono.clone()),
            Some(path) => path,
        };
        if let Some(name) = self.files.get(path) {
            return Ok(name.clone());
        }
        let file = base.join(path);
        let bytes =
            std::fs::read(&file).map_err(|e| format!("cannot read the font {path}: {e}"))?;
        let name = family_name(&bytes)
            .ok_or_else(|| format!("{path} is not a font with a family name"))?;
        self.engine
            .register_font(&bytes)
            .map_err(|e| format!("{path}: {e:?}"))?;
        self.files.insert(path.to_string(), name.clone());
        Ok(name)
    }
}

pub fn family_name(bytes: &[u8]) -> Option<String> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let of = |id: u16| {
        let mut found: Vec<(String, bool)> = face
            .names()
            .into_iter()
            .filter(|name| name.name_id == id && name.is_unicode())
            .filter_map(|name| {
                let english = name.language() == ttf_parser::Language::English_UnitedStates;
                name.to_string().map(|text| (text, english))
            })
            .collect();
        found.sort_by_key(|(_, english)| !english);
        found.into_iter().next().map(|(text, _)| text)
    };
    of(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| of(ttf_parser::name_id::FAMILY))
}

fn lay_one(
    fonts: &mut Fonts,
    base: &Path,
    recipe: &Recipe,
    parts: &[Part],
    index: usize,
) -> Result<Option<Laying>, Error> {
    let object = &recipe.objects[index];
    let Kind::Text {
        spans,
        wrap,
        anchor,
        space,
        on,
    } = &object.kind
    else {
        return Ok(None);
    };
    let path = format!("object[{}]", object.name);
    let overlay = *space == Space::Overlay;
    let scale = if overlay { 1.0 } else { UNITS_PER_METRE };
    let mut faces = Vec::with_capacity(spans.len());
    for (i, span) in spans.iter().enumerate() {
        let family = fonts
            .family(base, span.family.as_deref())
            .map_err(|text| Error(format!("{path}.spans[{i}].family: {text}")))?;
        let size = span.size as f32;
        faces.push(Span {
            text: span.text.clone(),
            face: Face {
                family,
                size,
                line: size * LINE,
                weight: span.weight.map_or(REGULAR_WEIGHT, |w| w.min(1000) as u16),
                italic: span.italic,
                spacing: 0.0,
            },
            color: [
                span.color[0] as f32,
                span.color[1] as f32,
                span.color[2] as f32,
                1.0,
            ],
        });
    }
    let name = anchor
        .as_deref()
        .unwrap_or(manfred_recipe::default_anchor(*space));
    let point = manfred_recipe::anchor_point(name)
        .ok_or_else(|| Error(format!("{path}.anchor: no anchor named \"{name}\"")))?
        .map(|v| v as f32);
    let wrap = wrap.map(|w| w as f32);
    let align = match (wrap, point[0]) {
        (None, _) => Anchor::Start,
        (Some(_), x) if x < 0.25 => Anchor::Start,
        (Some(_), x) if x > 0.75 => Anchor::End,
        (Some(_), _) => Anchor::Center,
    };
    let fail = |e| Error(format!("{path}.spans: the text engine refused them: {e:?}"));
    let block = fonts
        .engine
        .layout_spans(&faces, wrap, scale, align)
        .map_err(fail)?;
    let size = [wrap.unwrap_or(block.width), block.height];
    let mut origin = [-point[0] * size[0], -point[1] * size[1]];
    if overlay {
        origin[0] += object.at[0] as f32;
        origin[1] += object.at[1] as f32;
    }
    let representation = if overlay {
        Representation::Coverage
    } else {
        Representation::Msdf
    };
    let rich = fonts
        .engine
        .render_spans(
            &faces,
            wrap,
            scale,
            align,
            representation,
            origin,
            1.0,
            CLIP,
            [0.0; 3],
        )
        .map_err(fail)?;
    let quads = rich_quads(&rich);
    if quads.is_empty() {
        return Ok(None);
    }
    let mut target = None;
    let place = if overlay {
        Place::Overlay { screen: point }
    } else {
        let world = |at| world(recipe, at).map_err(|e| Error(e.to_string()));
        let frame = match on {
            Some(on) => {
                let found = recipe
                    .objects
                    .iter()
                    .position(|o| o.name == *on)
                    .ok_or_else(|| Error(format!("{path}.on: no object named `{on}`")))?;
                target = Some(found);
                let placed = world(found)?;
                placed
                    * face(&recipe.objects[found], parts, found, placed)
                    * DMat4::from_translation([0.0, 0.0, LIFT].into())
                    * local_matrix(object)
            }
            None => world(index)?,
        };
        Place::World {
            model: frame * DMat4::from_scale([1.0, -1.0, 1.0].into()),
            lit: *space == Space::Lit,
        }
    };
    Ok(Some(Laying {
        name: object.name.clone(),
        object: index,
        atlas: rich.atlas,
        quads,
        place,
        on: target,
    }))
}

pub fn face(target: &Object, parts: &[Part], index: usize, world: DMat4) -> DMat4 {
    let back = world.inverse();
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for part in parts.iter().filter(|part| part.object == index) {
        let node = &part.node;
        for p in &node.mesh.positions {
            let p = [p[0], p[1], p[2]].map(f64::from).into();
            let local = if node.keep_transform {
                p
            } else {
                back.transform_point3(p)
            };
            for (axis, v) in local.to_array().into_iter().enumerate() {
                low[axis] = low[axis].min(v);
                high[axis] = high[axis].max(v);
            }
        }
    }
    if low[0] > high[0] {
        return DMat4::IDENTITY;
    }
    let mid = |axis: usize| (low[axis] + high[axis]) * 0.5;
    match target.kind {
        Kind::Plane { .. } | Kind::Disc { .. } => DMat4::from_cols(
            [1.0, 0.0, 0.0, 0.0].into(),
            [0.0, 0.0, -1.0, 0.0].into(),
            [0.0, 1.0, 0.0, 0.0].into(),
            [mid(0), high[1], mid(2), 1.0].into(),
        ),
        _ => DMat4::from_translation([mid(0), mid(1), high[2]].into()),
    }
}

impl Laying {
    pub fn world(&self) -> Option<(Matrix, bool)> {
        match self.place {
            Place::World { model, lit } => Some((narrow(model), lit)),
            Place::Overlay { .. } => None,
        }
    }

    pub fn overlaid(&self, size: [u32; 2]) -> Option<Vec<RichQuad>> {
        match self.place {
            Place::Overlay { screen } => Some(placed(&self.quads, screen, size)),
            Place::World { .. } => None,
        }
    }
}

impl Laid {
    pub fn of(recipe: &Recipe, parts: &[Part]) -> Result<Laid, Error> {
        let texts: Vec<usize> = recipe
            .objects
            .iter()
            .enumerate()
            .filter(|(_, object)| !object.hidden && matches!(object.kind, Kind::Text { .. }))
            .map(|(index, _)| index)
            .collect();
        if texts.is_empty() {
            return Ok(Laid::default());
        }
        let mut fonts = Fonts::new()?;
        let base = recipe.base.as_deref().unwrap_or(Path::new(""));
        let mut items = Vec::new();
        for index in texts {
            if let Some(laying) = lay_one(&mut fonts, base, recipe, parts, index)? {
                items.push(laying);
            }
        }
        Ok(Laid { items })
    }

    pub fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for item in &self.items {
            let Place::World { model, .. } = item.place else {
                continue;
            };
            for quad in &item.quads {
                let [x, y, w, h] = quad.rect.map(f64::from);
                for corner in [[x, y], [x + w, y], [x, y + h], [x + w, y + h]] {
                    let p = model.transform_point3([corner[0], corner[1], 0.0].into());
                    for (axis, v) in p.to_array().into_iter().enumerate() {
                        low[axis] = low[axis].min(v);
                        high[axis] = high[axis].max(v);
                    }
                }
            }
        }
        (low[0] <= high[0]).then_some([low, high])
    }

    pub fn surface_boxes(&self) -> Vec<(usize, [[f64; 3]; 4])> {
        self.items
            .iter()
            .filter_map(|item| {
                let Place::World { model, .. } = item.place else {
                    return None;
                };
                let [left, top, right, bottom] = extent(&item.quads)?.map(f64::from);
                let corners = [[left, top], [right, top], [right, bottom], [left, bottom]]
                    .map(|[x, y]| model.transform_point3([x, y, 0.0].into()).to_array());
                Some((item.object, corners))
            })
            .collect()
    }

    pub fn overlay_boxes(&self, size: [u32; 2]) -> Vec<(usize, [f32; 4])> {
        let [width, height] = size.map(|side| side as f32);
        self.items
            .iter()
            .filter_map(|item| {
                let Place::Overlay { screen } = item.place else {
                    return None;
                };
                let [left, top, right, bottom] = extent(&item.quads)?;
                let shift = [screen[0] * width, screen[1] * height];
                let clipped = [
                    (left + shift[0]).clamp(0.0, width),
                    (top + shift[1]).clamp(0.0, height),
                    (right + shift[0]).clamp(0.0, width),
                    (bottom + shift[1]).clamp(0.0, height),
                ];
                (clipped[0] < clipped[2] && clipped[1] < clipped[3])
                    .then_some((item.object, clipped))
            })
            .collect()
    }
}

fn extent(quads: &[RichQuad]) -> Option<[f32; 4]> {
    let mut out = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for quad in quads {
        let [x, y, w, h] = quad.rect;
        out = [
            out[0].min(x),
            out[1].min(y),
            out[2].max(x + w),
            out[3].max(y + h),
        ];
    }
    (out[0] < out[2] && out[1] < out[3]).then_some(out)
}

pub fn widen(bounds: [[f64; 3]; 2], more: Option<[[f64; 3]; 2]>) -> [[f64; 3]; 2] {
    let Some([low, high]) = more else {
        return bounds;
    };
    [
        std::array::from_fn(|axis| bounds[0][axis].min(low[axis])),
        std::array::from_fn(|axis| bounds[1][axis].max(high[axis])),
    ]
}

struct Shown {
    object: usize,
    atlas: GpuAtlas,
    quads: Vec<RichQuad>,
    placed: Vec<RichQuad>,
    place: Place,
}

#[derive(Default)]
pub struct Drawn {
    items: Vec<Shown>,
    size: Option<[u32; 2]>,
}

impl Drawn {
    pub fn new(gpu: &Gpu, laid: Laid) -> Result<Drawn, Error> {
        let mut items = Vec::with_capacity(laid.items.len());
        for laying in laid.items {
            let atlas = GpuAtlas::new(&gpu.device, &gpu.queue, &laying.atlas)
                .map_err(|e| Error(format!("object[{}]: the text atlas: {e:?}", laying.name)))?;
            items.push(Shown {
                object: laying.object,
                atlas,
                placed: Vec::new(),
                quads: laying.quads,
                place: laying.place,
            });
        }
        Ok(Drawn { items, size: None })
    }

    pub fn text(&mut self, view_projection: Matrix, size: [u32; 2]) -> Text<'_> {
        if self.size != Some(size) {
            for item in &mut self.items {
                if let Place::Overlay { screen } = item.place {
                    item.placed = placed(&item.quads, screen, size);
                }
            }
            self.size = Some(size);
        }
        let mut text = Text::default();
        for item in &self.items {
            let id = item.object as u32 + 1;
            match item.place {
                Place::World { model, lit } => {
                    let model = narrow(model);
                    let model_view_projection = multiply(view_projection, model);
                    text.surface_quads.push(TextQuadItem {
                        atlas: &item.atlas,
                        quads: &item.quads,
                        space: if lit {
                            TextSpace::LitSurface {
                                model_view_projection,
                                model,
                            }
                        } else {
                            TextSpace::Surface {
                                model_view_projection,
                            }
                        },
                        icons: false,
                        id,
                    });
                }
                Place::Overlay { .. } => text.overlay_quads.push(TextQuadItem {
                    atlas: &item.atlas,
                    quads: &item.placed,
                    space: TextSpace::Overlay {
                        width: size[0],
                        height: size[1],
                    },
                    icons: false,
                    id,
                }),
            }
        }
        text
    }
}

fn placed(quads: &[RichQuad], screen: [f32; 2], size: [u32; 2]) -> Vec<RichQuad> {
    let shift = [screen[0] * size[0] as f32, screen[1] * size[1] as f32];
    quads.iter().map(|q| moved(q, shift)).collect()
}

fn moved(quad: &RichQuad, shift: [f32; 2]) -> RichQuad {
    let mut quad = *quad;
    quad.rect[0] += shift[0];
    quad.rect[1] += shift[1];
    quad.turn[0] += shift[0];
    quad.turn[1] += shift[1];
    quad
}

fn narrow(matrix: DMat4) -> Matrix {
    matrix.to_cols_array_2d().map(|col| col.map(|v| v as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::parse;

    const SCENE: &str = "manfred = 1\nname = \"words\"\n\n[[camera]]\nname = \"main\"\nposition = [0.0, 0.2, 1.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 35.0\n\n[[object]]\nname = \"slab\"\nkind = \"box\"\nsize = [0.4, 0.2, 0.1]\nat = [0.0, 0.1, 0.0]\nmaterial = \"orange\"\n\n[materials.orange]\nfamily = \"lacquer\"\nbase = [0.73, 0.06, 0.0]\nroughness = 0.45\nclearcoat = 1.0\nclearcoat_roughness = 0.07\n";

    fn laid(text: &str) -> (Recipe, Laid) {
        let recipe = parse(text).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let laid = Laid::of(&recipe, &built.parts).unwrap();
        (recipe, laid)
    }

    fn lines(quads: &[RichQuad], apart: f32) -> usize {
        let mut tops: Vec<f32> = quads.iter().map(|q| q.rect[1] + q.rect[3]).collect();
        tops.sort_by(f32::total_cmp);
        tops.dedup_by(|a, b| (*a - *b).abs() < apart);
        tops.len()
    }

    #[test]
    fn the_mono_family_is_named_from_the_font() {
        assert_eq!(
            family_name(REGULAR).as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(family_name(BOLD), family_name(REGULAR));
        assert_eq!(family_name(b"not a font"), None);
    }

    #[test]
    fn a_two_span_text_wraps_inside_its_width_in_both_colours() {
        let text = format!(
            "{SCENE}\n[[object]]\nname = \"label\"\nkind = \"text\"\nwrap = 0.12\nanchor = \"top_left\"\nspans = [{{ text = \"manfred draws \", size = 0.02, color = [1.0, 0.0, 0.0] }}, {{ text = \"the words of a recipe\", size = 0.02, weight = 700, color = [0.0, 0.0, 1.0] }}]\n"
        );
        let (_, laid) = laid(&text);
        assert_eq!(laid.items.len(), 1);
        let item = &laid.items[0];
        assert_eq!(item.name, "label");
        let glyphs = "manfreddrawsthewordsofarecipe".len();
        assert_eq!(item.quads.len(), glyphs);
        let red = item.quads.iter().filter(|q| q.color[0] == 1.0).count();
        let blue = item.quads.iter().filter(|q| q.color[2] == 1.0).count();
        assert_eq!(
            (red, blue),
            ("manfreddraws".len(), "thewordsofarecipe".len())
        );
        let wrap = 0.12;
        let count = lines(&item.quads, 0.01);
        assert!(count >= 3, "{count} lines");
        for quad in &item.quads {
            assert!(
                quad.rect[0] >= -0.003 && quad.rect[0] + quad.rect[2] <= wrap + 0.003,
                "{:?}",
                quad.rect
            );
            assert!(quad.rect[1] >= -0.005);
        }
        let Place::World { model, lit } = item.place else {
            panic!("surface text is placed in the world")
        };
        assert!(!lit);
        let corner = model.transform_point3([0.0, 0.0, 0.0].into());
        assert!(corner.length() < 1e-9);
        let down = model.transform_point3([0.0, 1.0, 0.0].into());
        assert!((down.y + 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_same_text_lays_out_the_same_bytes() {
        let text = format!(
            "{SCENE}\n[[object]]\nname = \"label\"\nkind = \"text\"\non = \"slab\"\nspans = [{{ text = \"label\", size = 0.03 }}]\n"
        );
        let (_, first) = laid(&text);
        let (_, second) = laid(&text);
        assert_eq!(first.items[0].quads, second.items[0].quads);
        assert_eq!(first.items[0].atlas.levels, second.items[0].atlas.levels);
    }

    #[test]
    fn text_on_an_object_sits_centred_on_its_front_face() {
        let text = format!(
            "{SCENE}\n[[object]]\nname = \"label\"\nkind = \"text\"\non = \"slab\"\nspans = [{{ text = \"label\", size = 0.03 }}]\n"
        );
        let (_, laid) = laid(&text);
        let item = &laid.items[0];
        let Place::World { model, .. } = item.place else {
            panic!("surface text is placed in the world")
        };
        let centre = model.transform_point3([0.0, 0.0, 0.0].into());
        assert!((centre.z - (0.05 + LIFT)).abs() < 1e-6, "{centre}");
        assert!(
            centre.x.abs() < 1e-6 && (centre.y - 0.1).abs() < 1e-6,
            "{centre}"
        );
        let [low, high] = laid.bounds().unwrap();
        assert!(low[0] < -0.01 && high[0] > 0.01);
        assert!((low[2] - (0.05 + LIFT)).abs() < 1e-6);
    }

    #[test]
    fn text_on_a_plane_lies_on_its_top() {
        let text = "manfred = 1\nname = \"floor\"\n\n[[object]]\nname = \"floor\"\nkind = \"plane\"\nsize = [1.0, 1.0]\nat = [0.0, 0.2, 0.0]\nmaterial = \"wood\"\n\n[materials.wood]\nfamily = \"wood\"\nbase = [0.11, 0.042, 0.016]\nroughness = 0.42\n\n[[object]]\nname = \"word\"\nkind = \"text\"\non = \"floor\"\nspans = [{ text = \"floor\", size = 0.05 }]\n";
        let (_, laid) = laid(text);
        let Place::World { model, .. } = laid.items[0].place else {
            panic!("surface text is placed in the world")
        };
        let up = model.transform_vector3([0.0, 0.0, 1.0].into());
        assert!((up.y - 1.0).abs() < 1e-9);
        let down_the_page = model.transform_vector3([0.0, 1.0, 0.0].into());
        assert!((down_the_page.z - 1.0).abs() < 1e-9);
        let p = model.transform_point3([0.0, 0.0, 0.0].into());
        assert!((p.y - (0.2 + LIFT)).abs() < 1e-6, "{p}");
    }

    #[test]
    fn overlay_text_is_in_pixels_at_its_screen_anchor() {
        let text = format!(
            "{SCENE}\n[[object]]\nname = \"title\"\nkind = \"text\"\nspace = \"overlay\"\nanchor = \"bottom_right\"\nat = [-12.0, -8.0, 0.0]\nspans = [{{ text = \"manfred\", size = 24.0 }}]\n"
        );
        let (_, laid) = laid(&text);
        let item = &laid.items[0];
        assert_eq!(item.place, Place::Overlay { screen: [1.0, 1.0] });
        let left = item
            .quads
            .iter()
            .map(|q| q.rect[0])
            .fold(f32::INFINITY, f32::min);
        assert!((-130.0..-90.0).contains(&left), "{left}");
        for quad in &item.quads {
            assert!(quad.rect[0] + quad.rect[2] <= -12.0 + 2.0);
            assert!(quad.rect[1] + quad.rect[3] <= -8.0 + 2.0);
        }
        assert_eq!(laid.bounds(), None);
    }

    #[test]
    fn a_laying_keeps_its_object_index_in_every_space() {
        let text = format!(
            "{SCENE}\n[[object]]\nname = \"gone\"\nkind = \"text\"\nhidden = true\nspans = [{{ text = \"gone\", size = 0.03 }}]\n\n[[object]]\nname = \"badge\"\nkind = \"text\"\non = \"slab\"\nspans = [{{ text = \"label\", size = 0.03 }}]\n\n[[object]]\nname = \"poster\"\nkind = \"text\"\nspace = \"lit\"\nspans = [{{ text = \"lit\", size = 0.03 }}]\n\n[[object]]\nname = \"title\"\nkind = \"text\"\nspace = \"overlay\"\nat = [10.0, 20.0, 0.0]\nspans = [{{ text = \"manfred\", size = 20.0 }}]\n"
        );
        let (recipe, laid) = laid(&text);
        let found: Vec<(usize, &str)> = laid
            .items
            .iter()
            .map(|item| (item.object, item.name.as_str()))
            .collect();
        assert_eq!(found, [(2, "badge"), (3, "poster"), (4, "title")]);
        for item in &laid.items {
            assert_eq!(recipe.objects[item.object].name, item.name);
        }
        let surface = laid.surface_boxes();
        assert_eq!(
            surface
                .iter()
                .map(|(object, _)| *object)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        let [a, b, c, d] = surface[0].1;
        for corner in [a, b, c, d] {
            assert!((corner[2] - (0.05 + LIFT)).abs() < 1e-6, "{corner:?}");
        }
        assert!(b[0] > a[0] && (b[1] - a[1]).abs() < 1e-9);
        assert!(d[1] < a[1]);
        let overlay = laid.overlay_boxes([640, 400]);
        assert_eq!(overlay.len(), 1);
        let (object, [left, top, right, bottom]) = overlay[0];
        assert_eq!(object, 4);
        assert!(
            (9.0..12.0).contains(&left) && (18.0..40.0).contains(&top),
            "{:?}",
            overlay[0].1
        );
        assert!(
            right > left + 30.0 && bottom > top + 10.0,
            "{:?}",
            overlay[0].1
        );
        assert!(laid.overlay_boxes([4, 4]).is_empty());
    }

    #[test]
    fn hidden_and_missing_fonts_are_handled() {
        let hidden = format!(
            "{SCENE}\n[[object]]\nname = \"label\"\nkind = \"text\"\nhidden = true\nspans = [{{ text = \"gone\", size = 0.03 }}]\n"
        );
        assert!(laid(&hidden).1.items.is_empty());
        let missing = format!(
            "{SCENE}\n[[object]]\nname = \"label\"\nkind = \"text\"\nspans = [{{ text = \"gone\", size = 0.03, family = \"fonts/none.ttf\" }}]\n"
        );
        let recipe = parse(&missing).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let error = Laid::of(&recipe, &built.parts).err().unwrap();
        assert!(
            error
                .0
                .starts_with("object[label].spans[0].family: cannot read the font fonts/none.ttf"),
            "{error}"
        );
    }

    const BOLD_FILE: &str = "fonts/JetBrainsMonoNerdFont-Bold.ttf";
    const NOTICE_FILE: &str = "fonts/SOURCE";

    fn put(path: &Path, bytes: &[u8]) {
        let part = path.with_extension(format!("part{}", std::process::id()));
        std::fs::write(&part, bytes).unwrap();
        std::fs::rename(&part, path).unwrap();
    }

    fn repo() -> std::path::PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/text-unit");
        std::fs::create_dir_all(base.join("fonts")).unwrap();
        put(&base.join(BOLD_FILE), BOLD);
        put(&base.join(NOTICE_FILE), b"not a font\n");
        base
    }

    fn lay_with_base(text: &str) -> Result<Laid, Error> {
        let mut recipe = parse(text).unwrap();
        recipe.base = Some(repo());
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        Laid::of(&recipe, &built.parts)
    }

    #[test]
    fn a_span_names_its_font_by_a_path_under_the_recipe_base() {
        let text = format!(
            "{SCENE}\n[[object]]\nname = \"label\"\nkind = \"text\"\nspans = [{{ text = \"by file \", size = 0.03, family = \"{BOLD_FILE}\" }}, {{ text = \"twice\", size = 0.03, family = \"{BOLD_FILE}\" }}]\n"
        );
        let laid = lay_with_base(&text).unwrap();
        assert_eq!(laid.items.len(), 1);
        assert_eq!(laid.items[0].quads.len(), "byfiletwice".len());

        let mut fonts = Fonts::new().unwrap();
        let first = fonts.family(&repo(), Some(BOLD_FILE)).unwrap();
        assert_eq!(Some(first.as_str()), family_name(BOLD).as_deref());
        assert_eq!(fonts.files.len(), 1);
        let second = fonts.family(&repo(), Some(BOLD_FILE)).unwrap();
        assert_eq!(second, first);
        assert_eq!(fonts.files.len(), 1);
        let unseen = fonts
            .family(Path::new("/nowhere"), Some(BOLD_FILE))
            .unwrap();
        assert_eq!(unseen, first);
    }

    #[test]
    fn a_span_pointing_at_a_file_that_is_not_a_font_is_refused() {
        let text = format!(
            "{SCENE}\n[[object]]\nname = \"label\"\nkind = \"text\"\nspans = [{{ text = \"nope\", size = 0.03, family = \"{NOTICE_FILE}\" }}]\n"
        );
        let error = lay_with_base(&text).err().unwrap();
        assert_eq!(
            error.0,
            format!(
                "object[label].spans[0].family: {NOTICE_FILE} is not a font with a family name"
            )
        );
        let mut fonts = Fonts::new().unwrap();
        assert!(fonts.family(&repo(), Some(NOTICE_FILE)).is_err());
        assert!(fonts.files.is_empty());
    }

    const TABLE: &str = include_str!("../../../tests/golden/table/table.toml");
    const STILL: [u32; 2] = [640, 400];
    const SURFACE: &str = "\n[[object]]\nname = \"badge\"\nkind = \"text\"\non = \"crate\"\nspans = [{ text = \"manfred \", size = 0.016, weight = 700, color = [1.0, 0.95, 0.85] }, { text = \"surface\", size = 0.012, color = [0.1, 0.1, 0.12] }]\n";
    const LIT: &str = "\n[[object]]\nname = \"poster\"\nkind = \"text\"\non = \"wall\"\nspace = \"lit\"\nat = [0.12, -0.36, 0.0]\nspans = [{ text = \"lit, on the wall\", size = 0.03, color = [0.05, 0.05, 0.08] }]\n";
    const OVERLAY: &str = "\n[[object]]\nname = \"title\"\nkind = \"text\"\nspace = \"overlay\"\nat = [16.0, 12.0, 0.0]\nspans = [{ text = \"overlay \", size = 22.0, color = [1.0, 0.75, 0.3] }, { text = \"in pixels\", size = 16.0, italic = true }]\n";

    fn still(text: &str) -> Vec<u8> {
        let recipe = parse(text).unwrap();
        let built = manfred_build::build(&recipe, Path::new("")).unwrap();
        let gpu = crate::headless().unwrap();
        crate::shoot(&gpu, &recipe, &built.parts, "main", STILL).unwrap()
    }

    fn changed(a: &[u8], b: &[u8]) -> usize {
        a.chunks_exact(4)
            .zip(b.chunks_exact(4))
            .filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > 24))
            .count()
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_text_recipe_draws_in_each_space() {
        let bare = still(TABLE);
        pfx::adapter::turn();
        for (space, text) in [("surface", SURFACE), ("lit", LIT), ("overlay", OVERLAY)] {
            let drawn = still(&format!("{TABLE}{text}"));
            let count = changed(&bare, &drawn);
            println!("{space}: {count} pixels changed");
            assert!(count > 150, "{space} text changed only {count} pixels");
            pfx::adapter::turn();
        }
        let all = still(&format!("{TABLE}{SURFACE}{LIT}{OVERLAY}"));
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/text-spaces.png");
        crate::write_png(&out, STILL, &all).unwrap();
        pfx::adapter::turn();
    }

    #[test]
    #[ignore = "GPU: pgpu run --class clip --as manfred -- cargo test -- --ignored"]
    fn a_still_with_text_is_the_same_bytes_twice() {
        let text = format!("{TABLE}{SURFACE}{LIT}{OVERLAY}");
        let first = still(&text);
        pfx::adapter::turn();
        let second = still(&text);
        assert!(
            first == second,
            "{} pixels differ",
            changed(&first, &second)
        );
        pfx::adapter::turn();
    }
}
