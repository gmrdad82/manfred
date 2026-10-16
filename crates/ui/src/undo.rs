use manfred_recipe::toml_edit::DocumentMut;
use manfred_recipe::{Patch, edit};

pub const DEPTH: usize = 256;

#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub undo: Vec<Patch>,
    pub redo: Vec<Patch>,
    pub join: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

fn fits(doc: &mut DocumentMut, patches: &[Patch]) -> bool {
    let mut trial = doc.clone();
    if patches.iter().all(|p| edit(&mut trial, p).is_ok()) {
        *doc = trial;
        true
    } else {
        false
    }
}

fn keep(entries: &mut Vec<Entry>, doc: &DocumentMut, side: fn(&Entry) -> &[Patch]) -> usize {
    let mut scratch = doc.clone();
    let before = entries.len();
    let mut kept: Vec<Entry> = entries
        .drain(..)
        .rev()
        .filter(|entry| fits(&mut scratch, side(entry)))
        .collect();
    kept.reverse();
    *entries = kept;
    before - entries.len()
}

impl History {
    pub fn done(&mut self, entry: Entry) {
        self.redo.clear();
        self.undo.push(entry);
        if self.undo.len() > DEPTH {
            self.undo.remove(0);
        }
    }

    fn stack(&mut self, undo: bool) -> &mut Vec<Entry> {
        if undo { &mut self.undo } else { &mut self.redo }
    }

    pub fn join(&mut self, entry: Entry) {
        let Some(top) = self
            .undo
            .last_mut()
            .filter(|top| top.join.is_some() && top.join == entry.join)
        else {
            return self.done(entry);
        };
        self.redo.clear();
        top.redo
            .retain(|kept| entry.redo.iter().all(|fresh| fresh.path != kept.path));
        top.redo.extend(entry.redo);
        top.label = entry.label;
    }

    pub fn top_join(&self) -> Option<u64> {
        self.undo.last().and_then(|top| top.join)
    }

    pub fn pop(&mut self, undo: bool) -> Option<Entry> {
        self.stack(undo).pop()
    }

    pub fn push(&mut self, undo: bool, entry: Entry) {
        self.stack(undo).push(entry);
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    #[cfg(test)]
    pub fn labels(&self) -> (Vec<&str>, Vec<&str>) {
        (
            self.undo.iter().map(|entry| entry.label.as_str()).collect(),
            self.redo.iter().map(|entry| entry.label.as_str()).collect(),
        )
    }

    pub fn prune(&mut self, doc: &DocumentMut) -> usize {
        keep(&mut self.undo, doc, |entry| &entry.undo)
            + keep(&mut self.redo, doc, |entry| &entry.redo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::{Key, number};

    fn at(name: &str, axis: usize) -> Vec<Key> {
        vec![
            Key::Table("object".into()),
            Key::Object(name.into()),
            Key::Table("at".into()),
            Key::Index(axis),
        ]
    }

    fn entry(label: &str, name: &str) -> Entry {
        let patch = Patch {
            path: at(name, 0),
            value: Some(number(0.0)),
        };
        Entry {
            label: label.into(),
            undo: vec![patch.clone()],
            redo: vec![patch],
            join: None,
        }
    }

    fn doc(names: &[&str]) -> DocumentMut {
        names
            .iter()
            .map(|name| format!("[[object]]\nname = \"{name}\"\nat = [1.0, 2.0, 3.0]\n"))
            .collect::<String>()
            .parse()
            .unwrap()
    }

    #[test]
    fn a_new_step_clears_the_redo_side() {
        let mut history = History::default();
        history.done(entry("one", "a"));
        history.done(entry("two", "a"));
        let two = history.pop(true).unwrap();
        history.push(false, two);
        assert_eq!(history.labels(), (vec!["one"], vec!["two"]));
        history.done(entry("three", "a"));
        assert_eq!(history.labels(), (vec!["one", "three"], vec![]));
    }

    #[test]
    fn pruning_drops_only_the_steps_whose_objects_are_gone() {
        let mut history = History::default();
        history.done(entry("a1", "a"));
        history.done(entry("b1", "b"));
        history.done(entry("a2", "a"));
        history.push(false, entry("c1", "c"));
        history.push(false, entry("b2", "b"));
        assert_eq!(history.prune(&doc(&["a", "c"])), 2);
        assert_eq!(history.labels(), (vec!["a1", "a2"], vec!["c1"]));
        assert_eq!(history.prune(&doc(&["a", "c"])), 0);
    }

    #[test]
    fn the_stack_keeps_its_last_steps() {
        let mut history = History::default();
        for step in 0..DEPTH + 3 {
            history.done(entry(&step.to_string(), "a"));
        }
        assert_eq!(history.undo_len(), DEPTH);
        assert_eq!(history.labels().0[0], "3");
    }
}
