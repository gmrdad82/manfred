use std::path::{Path, PathBuf};

use manfred_recipe::{Key, Kind, Patch, Recipe, TextSpace};
use toml_edit::{Array, Item, Value};

use crate::follow::{Stamp, stamp};

pub const SIZE_DECIMALS: usize = 4;
pub const PIXEL_DECIMALS: usize = 1;
pub const COLOR_DECIMALS: usize = 3;

#[derive(Clone, Debug, PartialEq)]
struct Saved {
    text: String,
    size: f32,
    color: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub text: String,
    pub size: f32,
    pub color: [f32; 3],
    saved: Saved,
}

#[derive(Debug, Default)]
pub struct Spans {
    path: Option<PathBuf>,
    stamp: Option<Stamp>,
    recipe: Option<Recipe>,
    object: Option<String>,
    shown: Option<(String, usize)>,
    pub rows: Vec<Row>,
    pub overlay: bool,
    text: bool,
    read: usize,
}

pub fn span_path(object: &str, span: usize, key: &str) -> Vec<Key> {
    vec![
        Key::Table("object".into()),
        Key::Object(object.into()),
        Key::Table("spans".into()),
        Key::Index(span),
        Key::Table(key.into()),
    ]
}

pub fn rounded(value: f32, decimals: usize) -> f64 {
    format!("{value:.decimals$}")
        .parse()
        .unwrap_or(f64::from(value))
}

pub fn text_patch(object: &str, span: usize, text: &str) -> Patch {
    Patch {
        path: span_path(object, span, "text"),
        value: Some(toml_edit::value(text)),
    }
}

pub fn size_patch(object: &str, span: usize, size: f32, decimals: usize) -> Patch {
    Patch {
        path: span_path(object, span, "size"),
        value: Some(toml_edit::value(rounded(size, decimals))),
    }
}

pub fn color_patch(object: &str, span: usize, color: [f32; 3]) -> Patch {
    let mut array = Array::new();
    for channel in color {
        array.push(rounded(channel, COLOR_DECIMALS));
    }
    Patch {
        path: span_path(object, span, "color"),
        value: Some(Item::Value(Value::Array(array))),
    }
}

impl Spans {
    pub fn follow(&mut self, path: &str, object: Option<&str>) -> bool {
        let path = Path::new(path);
        let now = stamp(path);
        if self.path.as_deref() != Some(path) || now != self.stamp {
            self.path = Some(path.to_path_buf());
            self.stamp = now;
            self.recipe = now
                .and_then(|_| manfred_recipe::load(path).ok())
                .map(|loaded| loaded.recipe);
            self.read += 1;
        }
        let wanted = object.map(|name| (name.to_string(), self.read));
        if wanted != self.shown {
            self.shown = wanted;
            self.object = object.map(str::to_string);
            self.fill();
        }
        self.object.is_some() && !self.rows.is_empty()
    }

    fn fill(&mut self) {
        self.rows.clear();
        self.overlay = false;
        self.text = false;
        let Some(name) = &self.object else {
            return;
        };
        let Some(recipe) = &self.recipe else {
            return;
        };
        let Some(object) = recipe.objects.iter().find(|o| o.name == *name) else {
            return;
        };
        let Kind::Text { spans, space, .. } = &object.kind else {
            return;
        };
        self.text = true;
        self.overlay = *space == TextSpace::Overlay;
        self.rows = spans
            .iter()
            .map(|span| {
                let saved = Saved {
                    text: span.text.clone(),
                    size: span.size as f32,
                    color: span.color.map(|c| c as f32),
                };
                Row {
                    text: saved.text.clone(),
                    size: saved.size,
                    color: saved.color,
                    saved,
                }
            })
            .collect();
    }

    pub fn is_text(&self) -> bool {
        self.text
    }

    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    pub fn size_decimals(&self) -> usize {
        if self.overlay {
            PIXEL_DECIMALS
        } else {
            SIZE_DECIMALS
        }
    }

    pub fn cancel_text(&mut self, span: usize) {
        if let Some(row) = self.rows.get_mut(span) {
            row.text = row.saved.text.clone();
        }
    }

    pub fn finish_text(&mut self, span: usize) -> Option<(String, Vec<Patch>)> {
        let object = self.object.clone()?;
        let row = self.rows.get_mut(span)?;
        if row.text == row.saved.text {
            return None;
        }
        row.saved.text = row.text.clone();
        Some((
            format!("text of {object} span {}", span + 1),
            vec![text_patch(&object, span, &row.text)],
        ))
    }

    pub fn finish_size(&mut self, span: usize) -> Option<(String, Vec<Patch>)> {
        let decimals = self.size_decimals();
        let object = self.object.clone()?;
        let row = self.rows.get_mut(span)?;
        if !(row.size.is_finite() && row.size > 0.0) {
            row.size = row.saved.size;
            return None;
        }
        if rounded(row.size, decimals) == rounded(row.saved.size, decimals) {
            return None;
        }
        row.saved.size = row.size;
        Some((
            format!("size of {object} span {}", span + 1),
            vec![size_patch(&object, span, row.size, decimals)],
        ))
    }

    pub fn finish_color(&mut self, span: usize) -> Option<(String, Vec<Patch>)> {
        let object = self.object.clone()?;
        let row = self.rows.get_mut(span)?;
        for channel in &mut row.color {
            if !channel.is_finite() {
                *channel = 0.0;
            }
            *channel = channel.max(0.0);
        }
        let same = (0..3).all(|c| {
            rounded(row.color[c], COLOR_DECIMALS) == rounded(row.saved.color[c], COLOR_DECIMALS)
        });
        if same {
            return None;
        }
        row.saved.color = row.color;
        Some((
            format!("colour of {object} span {}", span + 1),
            vec![color_patch(&object, span, row.color)],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::{apply, load, save};

    const SCENE: &str = r#"manfred = 1
name = "words"

# the label on the slab
[[object]]
name = "slab"
kind = "box"
size = [0.4, 0.2, 0.1]
material = "orange"

[[object]]
name = "label"
kind = "text"
on = "slab"
spans = [
  { text = "manfred ", size = 0.02 },  # the tool
  { text = "draws", size = 0.02, color = [0.9, 0.5, 0.1] },
]
"#;

    fn scene(name: &str) -> PathBuf {
        let path = crate::scratch(name);
        std::fs::write(&path, SCENE).unwrap();
        path
    }

    fn applied(path: &Path, patches: &[Patch]) -> Vec<Patch> {
        let mut loaded = load(path).unwrap();
        let inverses = patches
            .iter()
            .map(|p| apply(&mut loaded, p).unwrap())
            .collect();
        save(&loaded).unwrap();
        inverses
    }

    #[test]
    fn a_text_object_shows_one_row_per_span_and_others_show_none() {
        let path = scene("spans-rows");
        let mut spans = Spans::default();
        let at = path.display().to_string();
        assert!(spans.follow(&at, Some("label")));
        assert_eq!(spans.rows.len(), 2);
        assert_eq!(spans.rows[0].text, "manfred ");
        assert_eq!(spans.rows[0].color, [1.0, 1.0, 1.0]);
        assert_eq!(spans.rows[1].color, [0.9, 0.5, 0.1]);
        assert!(!spans.overlay);
        assert!(!spans.follow(&at, Some("slab")));
        assert!(!spans.follow(&at, None));
        assert!(!spans.follow("tmp/ui-tests/none/scene.toml", Some("label")));
    }

    #[test]
    fn an_unchanged_span_makes_no_gesture() {
        let path = scene("spans-unchanged");
        let mut spans = Spans::default();
        spans.follow(&path.display().to_string(), Some("label"));
        assert!(spans.finish_text(0).is_none());
        assert!(spans.finish_size(1).is_none());
        spans.rows[1].color[0] = 0.9000001;
        assert!(spans.finish_color(1).is_none());
        spans.rows[0].text.push('x');
        spans.cancel_text(0);
        assert!(spans.finish_text(0).is_none());
    }

    #[test]
    fn a_span_edit_patches_its_key_and_its_undo_restores_the_file() {
        let path = scene("spans-patches");
        let at = path.display().to_string();
        let mut spans = Spans::default();
        spans.follow(&at, Some("label"));
        spans.rows[0].text = "manfred, headless, ".into();
        let (label, text) = spans.finish_text(0).unwrap();
        assert_eq!(label, "text of label span 1");
        assert_eq!(
            text[0].path,
            span_path("label", 0, "text"),
            "object[label].spans[0].text"
        );
        spans.rows[1].size = 0.035;
        let (_, size) = spans.finish_size(1).unwrap();
        spans.rows[0].color = [0.25, 0.5, 1.0];
        let (label, color) = spans.finish_color(0).unwrap();
        assert_eq!(label, "colour of label span 1");
        let patches: Vec<Patch> = [text, size, color].concat();
        let inverses = applied(&path, &patches);
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("# the label on the slab"));
        assert!(written.contains("# the tool"), "{written}");
        assert!(
            written.contains("text = \"manfred, headless, \""),
            "{written}"
        );
        assert!(written.contains("color = [0.25, 0.5, 1.0]"), "{written}");
        assert!(written.contains("size = 0.035"), "{written}");
        let recipe = load(&path).unwrap().recipe;
        let Kind::Text { spans: read, .. } = &recipe.objects[1].kind else {
            panic!("the label is text")
        };
        assert_eq!(read[0].text, "manfred, headless, ");
        assert_eq!(read[0].color, [0.25, 0.5, 1.0]);
        assert_eq!(read[1].size, 0.035);
        let undo: Vec<Patch> = inverses.into_iter().rev().collect();
        applied(&path, &undo);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCENE);
        assert!(spans.follow(&at, Some("label")));
        assert_eq!(spans.rows[0].text, "manfred ");
    }

    #[test]
    fn sizes_round_to_their_decimals() {
        assert_eq!(rounded(0.02, SIZE_DECIMALS), 0.02);
        assert_eq!(rounded(0.123_456, SIZE_DECIMALS), 0.1235);
        let patch = size_patch("label", 0, 0.1, SIZE_DECIMALS);
        assert_eq!(patch.value.unwrap().to_string(), "0.1");
        let patch = color_patch("label", 0, [0.9, 0.5, 0.1]);
        assert_eq!(patch.value.unwrap().to_string(), "[0.9, 0.5, 0.1]");
    }
}
