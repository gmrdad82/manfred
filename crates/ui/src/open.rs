use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use egui::{
    Align, Align2, Color32, Context, CornerRadius, Frame, Id, Key, Layout, Margin, RichText,
    Stroke, Ui, Vec2,
};

use crate::shell::{code, list_row, small, tail_text, text_field};
use crate::theme::*;
use crate::widgets::KeyCap;

pub const LIMIT: usize = 10;
const CARD: f32 = 520.0;
const LIST: f32 = 280.0;
const FILE: &str = "\u{f15b}";
const FOLDER: &str = "\u{f07b}";

pub fn field_id() -> Id {
    Id::new("manfred open path")
}

pub fn state_dir() -> PathBuf {
    state_dir_from(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

pub fn state_dir_from(xdg: Option<&OsStr>, home: Option<&OsStr>) -> PathBuf {
    match xdg {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("manfred"),
        _ => home
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local/state/manfred"),
    }
}

pub fn data_dir_from(xdg: Option<&OsStr>, home: Option<&OsStr>) -> PathBuf {
    match xdg {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("manfred"),
        _ => home
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local/share/manfred"),
    }
}

pub fn installed_showcase() -> PathBuf {
    data_dir_from(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .join("showcase")
}

pub fn showcase(fallback: Option<&Path>) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    showcase_with(exe.as_deref().and_then(Path::parent), fallback)
}

pub fn showcase_with(start: Option<&Path>, fallback: Option<&Path>) -> Option<PathBuf> {
    start
        .and_then(showcase_from)
        .or_else(|| fallback.filter(|dir| dir.is_dir()).map(Path::to_path_buf))
}

pub fn showcase_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    for _ in 0..8 {
        let candidate = dir.join("showcase");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

pub fn known(
    anchor: Option<&Path>,
    last: Option<&Path>,
    showcase: Option<&Path>,
) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut push = |title: &str, path: &Path| {
        if path.as_os_str().is_empty() || !path.is_dir() {
            return;
        }
        if out.iter().any(|(_, known)| known == path) {
            return;
        }
        let title = if showcase.is_some_and(|dir| dir == path) {
            "showcase".to_string()
        } else {
            title.to_string()
        };
        out.push((title, path.to_path_buf()));
    };
    if let Some(path) = anchor {
        let title = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "folder".to_string());
        push(&title, path);
    }
    if let Some(path) = showcase {
        push("showcase", path);
    }
    if let Some(path) = last {
        let title = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "folder".to_string());
        push(&title, path);
    }
    out
}

pub fn recipes(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_recipe(&path) {
            found.push(path);
        }
    }
    found.sort_by(|a, b| by_name(a, b));
    found
}

pub fn complete(typed: &str) -> Vec<PathBuf> {
    if typed.is_empty() {
        return Vec::new();
    }
    let (dir, prefix) = split_query(typed);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = visible_name(&path) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        if path.is_dir() || is_recipe(&path) {
            found.push(path);
        }
    }
    found.sort_by(|a, b| by_name(a, b));
    found
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Recipe(PathBuf),
    Dir(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub recent: Vec<PathBuf>,
    pub groups: Vec<(String, Vec<PathBuf>)>,
    pub matches: Vec<Row>,
}

impl Listed {
    fn len(&self) -> usize {
        if !self.matches.is_empty() {
            return self.matches.len();
        }
        self.recent.len()
            + self
                .groups
                .iter()
                .map(|(_, recipes)| recipes.len())
                .sum::<usize>()
    }

    fn get(&self, index: usize) -> Option<Row> {
        if !self.matches.is_empty() {
            return self.matches.get(index).cloned();
        }
        if index < self.recent.len() {
            return Some(Row::Recipe(self.recent[index].clone()));
        }
        let mut index = index - self.recent.len();
        for (_, recipes) in &self.groups {
            if index < recipes.len() {
                return Some(Row::Recipe(recipes[index].clone()));
            }
            index -= recipes.len();
        }
        None
    }
}

pub fn listed(query: &str, recent: &[PathBuf], folders: &[(String, PathBuf)]) -> Listed {
    let query = query.trim();
    if !query.is_empty() {
        return Listed {
            recent: Vec::new(),
            groups: Vec::new(),
            matches: complete(query)
                .into_iter()
                .map(|path| {
                    if path.is_dir() {
                        Row::Dir(path)
                    } else {
                        Row::Recipe(path)
                    }
                })
                .collect(),
        };
    }
    Listed {
        recent: recent.iter().take(LIMIT).cloned().collect(),
        groups: folders
            .iter()
            .map(|(title, folder)| (title.clone(), recipes(folder)))
            .collect(),
        matches: Vec::new(),
    }
}

fn sources(
    live: bool,
    path: &str,
    anchor: Option<&Path>,
    book: &Book,
    fallback: Option<&Path>,
    own: bool,
) -> (Vec<(String, PathBuf)>, Vec<PathBuf>) {
    let recent = book.list();
    let last = if live {
        Path::new(path).parent().map(Path::to_path_buf)
    } else {
        recent
            .first()
            .and_then(|item| item.parent().map(Path::to_path_buf))
    };
    (
        known(
            anchor,
            last.as_deref(),
            showcase(fallback).filter(|_| !own).as_deref(),
        ),
        recent,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Open(PathBuf),
    Browse(PathBuf),
}

pub fn choose(query: &str, selected: Option<&Row>) -> Option<Choice> {
    choose_with(query, selected, std::env::var_os("HOME").as_deref())
}

fn choose_with(query: &str, selected: Option<&Row>, home: Option<&OsStr>) -> Option<Choice> {
    let query = query.trim();
    let typed = expand_home(query, home);
    if !query.is_empty() && typed.is_file() {
        return Some(Choice::Open(typed));
    }
    if !query.is_empty() && typed.is_dir() {
        return Some(Choice::Browse(typed));
    }
    match selected {
        Some(Row::Recipe(path)) => Some(Choice::Open(path.clone())),
        Some(Row::Dir(path)) => Some(Choice::Browse(path.clone())),
        None => None,
    }
}

#[derive(Clone, Debug)]
pub struct Book {
    path: Option<PathBuf>,
}

impl Book {
    pub fn machine() -> Book {
        Book {
            path: Some(state_dir().join("recent")),
        }
    }

    pub fn empty() -> Book {
        Book { path: None }
    }

    #[cfg(test)]
    pub fn at(dir: &Path) -> Book {
        Book {
            path: Some(dir.join("recent")),
        }
    }

    pub fn list(&self) -> Vec<PathBuf> {
        let Some(path) = &self.path else {
            return Vec::new();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect()
    }

    pub fn remember(&self, recipe: &Path) -> std::io::Result<()> {
        let Some(book) = &self.path else {
            return Ok(());
        };
        let recipe = std::path::absolute(recipe).unwrap_or_else(|_| recipe.to_path_buf());
        let mut paths = self.list();
        paths.retain(|path| !same_path(path, &recipe));
        paths.insert(0, recipe);
        paths.truncate(LIMIT);
        let mut text = String::new();
        for path in &paths {
            text.push_str(&path.display().to_string());
            text.push('\n');
        }
        write_atomic(book, text.as_bytes())
    }

    fn view_path(&self) -> Option<PathBuf> {
        self.path.as_ref().map(|path| path.with_file_name("view"))
    }

    pub fn view(&self) -> crate::wire::Look {
        let mut look = crate::wire::Look::default();
        let Some(text) = self
            .view_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
        else {
            return look;
        };
        for line in text.lines() {
            match line
                .split_once('=')
                .map(|(key, value)| (key.trim(), value.trim()))
            {
                Some(("wireframe", value)) => {
                    look.amount = crate::wire::amount(value).unwrap_or(0.0);
                }
                Some(("xray", value)) => look.xray = value == "on",
                _ => {}
            }
        }
        look
    }

    pub fn keep_view(&self, look: crate::wire::Look) -> std::io::Result<()> {
        let Some(path) = self.view_path() else {
            return Ok(());
        };
        let switch = |on: bool| if on { "on" } else { "off" };
        let text = format!(
            "wireframe = {}\nxray = {}\n",
            switch(look.on()),
            switch(look.xray)
        );
        write_atomic(&path, text.as_bytes())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Open(PathBuf),
    Close,
}

#[derive(Clone, Debug)]
struct Held {
    query: String,
    anchor: Option<PathBuf>,
    listed: Listed,
}

#[derive(Clone, Debug, Default)]
pub struct Picker {
    query: String,
    seen: String,
    shown: bool,
    anchor: Option<PathBuf>,
    cursor: usize,
    error: Option<String>,
    focus: bool,
    held: Option<Held>,
    fallback: Option<PathBuf>,
    own: bool,
}

impl Picker {
    pub fn set_showcase_fallback(&mut self, folder: PathBuf) {
        self.fallback = Some(folder);
    }

    pub fn without_showcase(&mut self) {
        self.own = true;
    }

    pub fn on_folder(folder: &str) -> Picker {
        Picker {
            anchor: Some(PathBuf::from(folder)),
            shown: true,
            ..Picker::default()
        }
    }

    pub fn showing(&self, live: bool) -> bool {
        !live || self.shown
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn show(&mut self) {
        self.shown = true;
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.shown = false;
        self.query.clear();
        self.seen.clear();
        self.error = None;
        self.cursor = 0;
        self.focus = false;
        self.held = None;
    }

    pub fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn draw(&mut self, ctx: &Context, live: bool, path: &str, book: &Book) -> Option<Act> {
        if !self.showing(live) {
            self.held = None;
            return None;
        }
        egui::Area::new(Id::new("manfred open"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fade_in(false)
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(BG_BASE)
                    .stroke(Stroke::new(STROKE, LINE_STRONG))
                    .corner_radius(CornerRadius::same(RADIUS))
                    .inner_margin(Margin::same(PANEL_PADDING))
                    .show(ui, |ui| {
                        ui.set_min_width(CARD);
                        ui.set_max_width(CARD);
                        ui.spacing_mut().item_spacing = Vec2::new(ITEM_SPACING, ITEM_SPACING);
                        self.contents(ui, live, path, book)
                    })
                    .inner
            })
            .inner
    }

    fn hold(&mut self, live: bool, path: &str, book: &Book) {
        let same = self
            .held
            .as_ref()
            .is_some_and(|held| held.query == self.query && held.anchor == self.anchor);
        if same {
            return;
        }
        let (folders, recent) = if self.query.trim().is_empty() {
            sources(
                live,
                path,
                self.anchor.as_deref(),
                book,
                self.fallback.as_deref(),
                self.own,
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let listed = listed(&self.query, &recent, &folders);
        self.held = Some(Held {
            query: self.query.clone(),
            anchor: self.anchor.clone(),
            listed,
        });
    }

    fn contents(&mut self, ui: &mut Ui, live: bool, path: &str, book: &Book) -> Option<Act> {
        if self.focus {
            ui.memory_mut(|memory| memory.request_focus(field_id()));
            if !ui.is_sizing_pass() {
                self.focus = false;
            }
        }
        title(ui);
        let response = text_field(
            ui,
            &mut self.query,
            "path of a recipe or a scene",
            ui.available_width(),
            Some(field_id()),
        );
        if self.query != self.seen {
            self.cursor = 0;
            self.seen.clone_from(&self.query);
            self.error = None;
        }
        self.hold(live, path, book);
        let len = self
            .held
            .as_ref()
            .map(|held| held.listed.len())
            .unwrap_or(0);
        if len > 0 {
            self.cursor = self.cursor.min(len - 1);
        }
        let take = response.has_focus() || response.lost_focus() || self.shown;
        let enter = take && pressed(ui, Key::Enter);
        let escape = take && pressed(ui, Key::Escape);
        let up = take && pressed(ui, Key::ArrowUp);
        let down = take && pressed(ui, Key::ArrowDown);
        if up {
            self.cursor = self.cursor.saturating_sub(1);
        }
        if down && len > 0 {
            self.cursor = (self.cursor + 1).min(len - 1);
        }
        if let Some(error) = &self.error {
            ui.label(code(ui, error, DANGER));
        }
        let cursor = self.cursor;
        let empty = self.query.trim().is_empty();
        let mut clicked = None;
        if let Some(held) = self.held.as_ref() {
            let listed = &held.listed;
            egui::ScrollArea::vertical()
                .id_salt("manfred open list")
                .max_height(LIST)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if empty {
                        let mut index = 0;
                        heading(ui, "recent");
                        if listed.recent.is_empty() {
                            muted(ui, "none yet");
                        }
                        for path in &listed.recent {
                            if recipe_row(ui, index == cursor, path).clicked() {
                                clicked = Some(index);
                            }
                            index += 1;
                        }
                        for (title, recipes) in &listed.groups {
                            heading(ui, title);
                            if recipes.is_empty() {
                                muted(ui, "no recipes");
                                continue;
                            }
                            for path in recipes {
                                if recipe_row(ui, index == cursor, path).clicked() {
                                    clicked = Some(index);
                                }
                                index += 1;
                            }
                        }
                    } else {
                        heading(ui, "matches");
                        if listed.matches.is_empty() {
                            muted(ui, "nothing matches");
                        }
                        for (index, row) in listed.matches.iter().enumerate() {
                            let response = match row {
                                Row::Recipe(path) => recipe_row(ui, index == cursor, path),
                                Row::Dir(path) => dir_row(ui, index == cursor, path),
                            };
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        }
                    }
                });
        }
        let (enter_clicked, escape_clicked) = footer(ui, live);
        let enter = enter || enter_clicked;
        if escape || escape_clicked {
            if live && self.shown {
                return Some(Act::Close);
            }
            self.query.clear();
            self.seen.clear();
            self.error = None;
            self.cursor = 0;
            self.shown = false;
            self.held = None;
            return None;
        }
        if clicked.is_none() && !enter {
            return None;
        }
        let index = clicked.unwrap_or(self.cursor);
        let chosen = self.held.as_ref().and_then(|held| held.listed.get(index));
        match choose(&self.query, chosen.as_ref()) {
            Some(Choice::Open(path)) => Some(Act::Open(path)),
            Some(Choice::Browse(path)) => {
                self.anchor = Some(path.clone());
                let mut text = path.display().to_string();
                if !text.ends_with('/') {
                    text.push('/');
                }
                self.query = text;
                self.seen.clone_from(&self.query);
                self.cursor = 0;
                self.error = None;
                self.focus = true;
                None
            }
            None => {
                let query = self.query.trim();
                self.error = Some(if query.is_empty() {
                    "type a path".to_string()
                } else {
                    format!("no recipe at {query}")
                });
                None
            }
        }
    }
}

fn title(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("OPEN")
                .font(Theme::of(ui.ctx()).bold_font(SIZE_BODY))
                .color(TEXT_SECONDARY),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(small(ui, "open", TEXT_SECONDARY));
            ui.add(KeyCap::new("ctrl o"));
        });
    });
}

fn heading(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(small(ui, title.to_uppercase(), TEXT_SECONDARY));
}

fn muted(ui: &mut Ui, line: &str) {
    ui.label(code(ui, line, TEXT_MUTED));
}

fn footer(ui: &mut Ui, live: bool) -> (bool, bool) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let enter = ui.add(KeyCap::new("enter").button()).clicked();
        ui.label(small(ui, "open", TEXT_SECONDARY));
        ui.add_space(8.0);
        let escape = ui.add(KeyCap::new("esc").button()).clicked();
        ui.label(small(
            ui,
            if live { "close" } else { "clear" },
            TEXT_SECONDARY,
        ));
        ui.add_space(8.0);
        ui.add(KeyCap::new("↑↓"));
        ui.label(small(ui, "choose", TEXT_SECONDARY));
        (enter, escape)
    })
    .inner
}

fn recipe_row(ui: &mut Ui, selected: bool, path: &Path) -> egui::Response {
    let name = file_name(path);
    let parent = path
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    entry(ui, selected, (FILE, INFO), &name, &detail(path, &parent))
}

pub fn detail(path: &Path, parent: &str) -> String {
    if crate::watched::is_scene(path) {
        format!("scene  {}", tail_text(parent, 35))
    } else {
        tail_text(parent, 42)
    }
}

fn dir_row(ui: &mut Ui, selected: bool, path: &Path) -> egui::Response {
    entry(ui, selected, (FOLDER, TEXT_MUTED), &file_name(path), "")
}

fn entry(
    ui: &mut Ui,
    selected: bool,
    icon: (&str, Color32),
    label: &str,
    detail: &str,
) -> egui::Response {
    let response = list_row(ui, true, selected, 0, icon, label, detail);
    if selected {
        response.scroll_to_me(None);
    }
    response
}

fn pressed(ui: &Ui, key: Key) -> bool {
    ui.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: found,
                    pressed: true,
                    modifiers,
                    ..
                } if *found == key && modifiers.is_none()
            )
        })
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_recipe(path: &Path) -> bool {
    path.is_file()
        && path.extension().is_some_and(|ext| ext == "toml")
        && visible_name(path).is_some()
}

fn visible_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if name.starts_with('.') {
        None
    } else {
        Some(name)
    }
}

fn by_name(a: &Path, b: &Path) -> std::cmp::Ordering {
    a.file_name().cmp(&b.file_name()).then(a.cmp(b))
}

fn expand_home(typed: &str, home: Option<&OsStr>) -> PathBuf {
    match (typed.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) if !home.is_empty() => PathBuf::from(home).join(rest),
        _ => PathBuf::from(typed),
    }
}

fn split_query(typed: &str) -> (PathBuf, String) {
    split_query_with(typed, std::env::var_os("HOME").as_deref())
}

fn split_query_with(typed: &str, home: Option<&OsStr>) -> (PathBuf, String) {
    let expanded = expand_home(typed, home);
    if typed.ends_with('/') {
        return (expanded, String::new());
    }
    let path = expanded.as_path();
    let prefix = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    (dir, prefix)
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::path::absolute(a), std::path::absolute(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "recent".to_string());
    let temp = path.with_file_name(format!(".{name}.manfred-tmp"));
    std::fs::write(&temp, bytes)?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let folder = crate::scratch(name).parent().unwrap().to_path_buf();
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths.iter().map(|path| file_name(path)).collect()
    }

    #[test]
    fn the_picker_lists_a_folders_recipes_sorted_by_name() {
        let folder = dir("open-list");
        std::fs::write(folder.join("c.toml"), "c").unwrap();
        std::fs::write(folder.join("a.toml"), "a").unwrap();
        std::fs::write(folder.join("b.toml"), "b").unwrap();
        std::fs::write(folder.join("notes.txt"), "n").unwrap();
        std::fs::write(folder.join(".hidden.toml"), "h").unwrap();
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        std::fs::write(folder.join("sub/d.toml"), "d").unwrap();
        assert_eq!(names(&recipes(&folder)), ["a.toml", "b.toml", "c.toml"]);
        let listed = listed("", &[], &[("scenes".into(), folder.clone())]);
        assert!(listed.recent.is_empty());
        assert_eq!(listed.groups.len(), 1);
        assert_eq!(names(&listed.groups[0].1), ["a.toml", "b.toml", "c.toml"]);
        assert_eq!(listed.len(), 3);
        assert_eq!(
            listed.get(0).and_then(|row| match row {
                Row::Recipe(path) => Some(file_name(&path)),
                Row::Dir(_) => None,
            }),
            Some("a.toml".to_string())
        );
        assert_eq!(listed.get(3), None);
    }

    #[test]
    fn the_picker_lists_scene_files_beside_recipes_and_marks_them() {
        let folder = dir("open-list-scene");
        std::fs::write(folder.join("room.scene.toml"), "s").unwrap();
        std::fs::write(folder.join("desk.toml"), "d").unwrap();
        let found = recipes(&folder);
        assert_eq!(names(&found), ["desk.toml", "room.scene.toml"]);
        assert_eq!(detail(&found[1], "scenes"), "scene  scenes");
        assert_eq!(detail(&found[0], "scenes"), "scenes");
    }

    #[test]
    fn recent_recipes_are_kept_deduplicated_and_capped_at_ten() {
        let folder = dir("open-recent");
        let book = Book::at(&folder.join("state"));
        let mut made = Vec::new();
        for index in 0..12 {
            let path = folder.join(format!("r{index}.toml"));
            std::fs::write(&path, "manfred = 1\n").unwrap();
            book.remember(&path).unwrap();
            made.push(std::path::absolute(&path).unwrap());
        }
        let list = book.list();
        assert_eq!(list.len(), LIMIT);
        assert_eq!(list[0], made[11]);
        assert!(!list.contains(&made[0]));
        assert!(!list.contains(&made[1]));
        book.remember(&made[5]).unwrap();
        let list = book.list();
        assert_eq!(list.len(), LIMIT);
        assert_eq!(list[0], made[5]);
        assert_eq!(list.iter().filter(|path| *path == &made[5]).count(), 1);
        assert_eq!(list[1], made[11]);
        assert!(!folder.join("state").join(".recent.manfred-tmp").exists());
        let text = std::fs::read_to_string(book.path.unwrap()).unwrap();
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), LIMIT);
    }

    #[test]
    fn the_path_field_completes_files_and_folders() {
        let folder = dir("open-complete");
        std::fs::write(folder.join("toybox.toml"), "s").unwrap();
        std::fs::write(folder.join("table.toml"), "p").unwrap();
        std::fs::write(folder.join("readme.txt"), "r").unwrap();
        std::fs::create_dir_all(folder.join("more")).unwrap();
        let prefix = format!("{}/to", folder.display());
        assert_eq!(names(&complete(&prefix)), ["toybox.toml"]);
        let inside = format!("{}/", folder.display());
        assert_eq!(
            names(&complete(&inside)),
            ["more", "table.toml", "toybox.toml"]
        );
        let listed = listed(&prefix, &[], &[]);
        assert_eq!(
            listed.matches,
            vec![Row::Recipe(folder.join("toybox.toml"))]
        );
    }

    #[test]
    fn a_leading_tilde_slash_is_the_home_folder_in_choose_and_completion() {
        let home = dir("open-home");
        std::fs::create_dir_all(home.join("scenes")).unwrap();
        std::fs::write(home.join("scenes/toybox.toml"), "manfred = 1\n").unwrap();
        std::fs::write(home.join("scenes/table.toml"), "manfred = 1\n").unwrap();
        let root = home.clone();
        let home = Some(home.as_os_str());
        assert_eq!(
            choose_with("~/scenes/toybox.toml", None, home),
            Some(Choice::Open(root.join("scenes/toybox.toml")))
        );
        assert_eq!(
            choose_with("~/scenes/", None, home),
            Some(Choice::Browse(root.join("scenes/")))
        );
        assert_eq!(choose_with("~/scenes/missing.toml", None, home), None);
        let (folder, prefix) = split_query_with("~/scenes/to", home);
        assert_eq!(folder, root.join("scenes"));
        assert_eq!(prefix, "to");
        let (folder, prefix) = split_query_with("~/scenes/", home);
        assert_eq!(folder, root.join("scenes/"));
        assert_eq!(prefix, "");
        assert_eq!(
            split_query_with("~/scenes/to", None),
            (PathBuf::from("~/scenes"), "to".to_string())
        );
        assert_eq!(
            split_query_with("~x/scenes", home),
            (PathBuf::from("~x"), "scenes".to_string())
        );
    }

    #[test]
    fn enter_opens_a_typed_file_and_browses_a_typed_folder() {
        let folder = dir("open-choose");
        let recipe = folder.join("scene.toml");
        std::fs::write(&recipe, "manfred = 1\n").unwrap();
        assert_eq!(
            choose(&recipe.display().to_string(), None),
            Some(Choice::Open(recipe))
        );
        assert_eq!(
            choose(&folder.display().to_string(), None),
            Some(Choice::Browse(folder.clone()))
        );
        let selected = Row::Recipe(folder.join("other.toml"));
        assert_eq!(
            choose("", Some(&selected)),
            Some(Choice::Open(folder.join("other.toml")))
        );
        assert_eq!(choose("missing.toml", None), None);
    }

    #[test]
    fn state_home_is_the_recent_folder_and_home_is_the_fallback() {
        let xdg = OsStr::new("/var/state");
        let home = OsStr::new("/u/dad");
        assert_eq!(
            state_dir_from(Some(xdg), Some(home)),
            PathBuf::from("/var/state/manfred")
        );
        assert_eq!(
            state_dir_from(None, Some(home)),
            PathBuf::from("/u/dad/.local/state/manfred")
        );
        assert_eq!(
            state_dir_from(Some(OsStr::new("")), Some(home)),
            PathBuf::from("/u/dad/.local/state/manfred")
        );
    }

    #[test]
    fn showcase_is_the_folder_beside_the_repo_when_it_is_there() {
        let root = dir("open-walk");
        let showcase = root.join("showcase");
        std::fs::create_dir_all(showcase.join("nested")).unwrap();
        let start = root.join("target/debug/deps");
        std::fs::create_dir_all(&start).unwrap();
        assert_eq!(showcase_from(&start), Some(showcase));
        let lost = dir("open-walk-lost").join("a/b/c/d/e/f/g/h/i");
        std::fs::create_dir_all(&lost).unwrap();
        assert_eq!(showcase_from(&lost), None);
    }

    #[test]
    fn a_binary_outside_the_tree_lists_the_installed_showcase() {
        let outside = dir("open-bin").join("a/b/c/d/e/f/g/h/i");
        std::fs::create_dir_all(&outside).unwrap();
        let installed = dir("open-data").join("manfred/showcase");
        std::fs::create_dir_all(&installed).unwrap();
        assert_eq!(showcase_with(Some(&outside), None), None);
        assert_eq!(
            showcase_with(Some(&outside), Some(&installed)),
            Some(installed.clone())
        );
        assert_eq!(showcase_with(None, Some(&installed)), Some(installed));
        assert_eq!(
            showcase_with(Some(&outside), Some(&outside.join("missing"))),
            None
        );
        let tree = dir("open-tree");
        let beside = tree.join("showcase");
        std::fs::create_dir_all(&beside).unwrap();
        let start = tree.join("target/debug");
        std::fs::create_dir_all(&start).unwrap();
        assert_eq!(showcase_with(Some(&start), Some(&outside)), Some(beside));
    }

    #[test]
    fn the_data_folder_follows_xdg_data_home_then_home() {
        let xdg = OsStr::new("/var/share");
        let home = OsStr::new("/u/dad");
        assert_eq!(
            data_dir_from(Some(xdg), Some(home)),
            PathBuf::from("/var/share/manfred")
        );
        assert_eq!(
            data_dir_from(None, Some(home)),
            PathBuf::from("/u/dad/.local/share/manfred")
        );
        assert_eq!(
            data_dir_from(Some(OsStr::new("")), Some(home)),
            PathBuf::from("/u/dad/.local/share/manfred")
        );
    }

    #[test]
    fn known_folders_keep_showcase_once() {
        let root = dir("open-known");
        let showcase = root.join("showcase");
        let scenes = root.join("scenes");
        std::fs::create_dir_all(&showcase).unwrap();
        std::fs::create_dir_all(&scenes).unwrap();
        let folders = known(Some(&showcase), Some(&scenes), Some(&showcase));
        assert_eq!(
            folders,
            vec![
                ("showcase".to_string(), showcase),
                ("scenes".to_string(), scenes),
            ]
        );
    }
}
