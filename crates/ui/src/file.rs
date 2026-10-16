use std::path::{Path, PathBuf};

use manfred_build::{Document, Registry, gltf_io};
use manfred_recipe::{Key, Loaded, Patch};
use toml_edit::{ArrayOfTables, DocumentMut, Item};

pub const MARK: &str = "\u{25cf}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Recipe,
    Scene,
    Manf,
    Glb,
}

pub fn kind(path: &Path) -> Option<Kind> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("manf") => Some(Kind::Manf),
        Some("glb") => Some(Kind::Glb),
        Some("toml") if crate::watched::is_scene(path) => Some(Kind::Scene),
        Some("toml") => Some(Kind::Recipe),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Open(PathBuf),
    Save,
    SaveAs(PathBuf),
    Import(PathBuf),
    Export { path: PathBuf, selected: bool },
    Drop { path: PathBuf, shift: bool },
}

pub fn step(command: &str, rest: &str) -> Option<Result<Step, String>> {
    let rest = rest.trim();
    let path = |text: &str, why: &str| {
        let text = text.trim();
        if text.is_empty() {
            Err(why.to_string())
        } else {
            Ok(PathBuf::from(text))
        }
    };
    let flagged = |flag: &str| match rest.strip_suffix(flag) {
        Some(head) if head.ends_with(char::is_whitespace) => (head, true),
        _ => (rest, false),
    };
    Some(match command {
        "save" if rest.is_empty() => Ok(Step::Save),
        "save" => Err("save takes nothing; save-as <path> saves elsewhere".to_string()),
        "open" => path(rest, "open wants a path").map(Step::Open),
        "save-as" => path(rest, "save-as wants a path").map(Step::SaveAs),
        "import" => path(rest, "import wants the path of a .glb").map(Step::Import),
        "export" => {
            let (head, selected) = flagged("--selected");
            path(head, "export wants a path and an optional --selected")
                .map(|path| Step::Export { path, selected })
        }
        "drop" => {
            let (head, shift) = flagged("shift");
            path(head, "drop wants a path and an optional shift")
                .map(|path| Step::Drop { path, shift })
        }
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub kind: Kind,
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub work: PathBuf,
    pub saved: usize,
}

impl Held {
    pub fn shown(&self) -> &Path {
        self.target.as_deref().unwrap_or(&self.source)
    }
}

#[cfg(not(test))]
fn work_root() -> PathBuf {
    crate::open::state_dir().join("documents")
}

#[cfg(test)]
fn work_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/ui-tests/documents")
}

fn work_dir(source: &Path) -> PathBuf {
    let whole = std::path::absolute(source).unwrap_or_else(|_| source.to_path_buf());
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());
    work_root().join(format!(
        "{stem}-{:016x}",
        crate::follow::hash(&whole.display().to_string())
    ))
}

fn fresh_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))
}

pub fn hold(path: &Path) -> Result<(Held, PathBuf), String> {
    let work = work_dir(path);
    match kind(path) {
        Some(Kind::Manf) => {
            let document = Document::open(path)?;
            fresh_dir(&work)?;
            let recipe = document.unpack(&work)?;
            Ok((
                Held {
                    kind: Kind::Manf,
                    source: path.to_path_buf(),
                    target: Some(path.to_path_buf()),
                    work,
                    saved: 0,
                },
                recipe,
            ))
        }
        Some(Kind::Glb) => {
            if !is_glb(path) {
                return Err(format!("{}: not a binary glTF 2.0 file", path.display()));
            }
            fresh_dir(&work)?;
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "imported".to_string());
            let file = path.file_name().map(PathBuf::from).unwrap_or_default();
            std::fs::copy(path, work.join(&file))
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let head = format!(
                "manfred = 1\nname = {}\n",
                toml_edit::Value::from(stem.as_str())
            );
            let text = gltf_io::import_glb(&head, &file, &object_name(&stem, &[]))?;
            let recipe = work.join(format!("{stem}.toml"));
            std::fs::write(&recipe, text)
                .map_err(|error| format!("{}: {error}", recipe.display()))?;
            Ok((
                Held {
                    kind: Kind::Glb,
                    source: path.to_path_buf(),
                    target: None,
                    work,
                    saved: 0,
                },
                recipe,
            ))
        }
        _ => Err(format!("{}: not a .manf or a .glb", path.display())),
    }
}

pub fn with_extension(path: &Path, ext: &str) -> PathBuf {
    if path.extension().is_some_and(|found| found == ext) {
        return path.to_path_buf();
    }
    let mut text = path.as_os_str().to_os_string();
    text.push(".");
    text.push(ext);
    PathBuf::from(text)
}

pub fn pack(recipe: &Path, registry: &Registry, out: &Path) -> Result<PathBuf, String> {
    let out = with_extension(out, "manf");
    let mut document = Document::pack(recipe, registry)?;
    if let Ok(old) = Document::open(&out) {
        document.overrides = old.overrides;
        if document.thumb.is_none() {
            document.thumb = old.thumb;
        }
    }
    document.write(&out)?;
    Ok(out)
}

pub fn export(
    recipe: &Path,
    registry: &Registry,
    only: Option<&str>,
    out: &Path,
) -> Result<PathBuf, String> {
    let out = with_extension(out, "glb");
    let bytes = gltf_io::export_glb(recipe, registry, only)?;
    crate::open::write_atomic(&out, &bytes)
        .map_err(|error| format!("{}: {error}", out.display()))?;
    Ok(out)
}

pub fn is_glb(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 8];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok()
        && &head[..4] == b"glTF"
        && u32::from_le_bytes([head[4], head[5], head[6], head[7]]) == 2
}

pub fn object_name(stem: &str, known: &[&str]) -> String {
    let mut base: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
        base.insert_str(0, "mesh_");
    }
    if !known.contains(&base.as_str()) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|name| !known.contains(&name.as_str()))
        .unwrap_or(base)
}

pub fn copy_in(glb: &Path, work: &Path) -> Result<PathBuf, String> {
    let name = glb
        .file_name()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{}: not a file", glb.display()))?;
    let stem = glb
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut file = name.clone();
    let mut n = 2;
    while work.join(&file).exists() {
        if std::fs::read(work.join(&file)).ok() == std::fs::read(glb).ok() {
            return Ok(file);
        }
        file = PathBuf::from(format!("{stem}_{n}.glb"));
        n += 1;
    }
    std::fs::copy(glb, work.join(&file)).map_err(|error| format!("{}: {error}", glb.display()))?;
    Ok(file)
}

pub fn relative(path: &Path, base: &Path) -> PathBuf {
    let whole = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let base = std::path::absolute(base).unwrap_or_else(|_| base.to_path_buf());
    whole
        .strip_prefix(&base)
        .map(Path::to_path_buf)
        .unwrap_or(whole)
}

pub fn import_patches(loaded: &Loaded, glb: &Path, name: &str) -> Result<Vec<Patch>, String> {
    let text = gltf_io::import_glb(&loaded.doc.to_string(), glb, name)?;
    let doc: DocumentMut = text
        .parse()
        .map_err(|error| format!("import: the recipe would not read: {error}"))?;
    let mut table = doc
        .get("object")
        .and_then(Item::as_array_of_tables)
        .and_then(|objects| objects.iter().last())
        .cloned()
        .ok_or_else(|| "import: no object was added".to_string())?;
    table.decor_mut().set_prefix("\n");
    let had = loaded
        .doc
        .get("object")
        .and_then(Item::as_array_of_tables)
        .is_some_and(|objects| !objects.is_empty());
    Ok(vec![if had {
        Patch {
            path: vec![
                Key::Table("object".to_string()),
                Key::Object(name.to_string()),
            ],
            value: Some(Item::Table(table)),
        }
    } else {
        let mut tables = ArrayOfTables::new();
        tables.push(table);
        Patch {
            path: vec![Key::Table("object".to_string())],
            value: Some(Item::ArrayOfTables(tables)),
        }
    }])
}

pub fn thumb(path: &Path) -> Option<Vec<u8>> {
    let reader = manf::Reader::open(path).ok()?;
    let thumb = reader.chunk(manf::THMB).ok()??;
    Some(thumb.into_owned())
}

pub fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let size = [info.width as usize, info.height as usize];
    let pixels = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
}
