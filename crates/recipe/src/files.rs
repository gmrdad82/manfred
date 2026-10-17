use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

type Shared = Arc<Mutex<Layer>>;

#[derive(Default)]
struct Layer {
    files: BTreeMap<PathBuf, Arc<[u8]>>,
    changed: BTreeSet<PathBuf>,
}

#[derive(Default)]
struct Table {
    mounts: Vec<(u64, Shared)>,
    next: u64,
}

static TABLE: Mutex<Table> = Mutex::new(Table {
    mounts: Vec::new(),
    next: 0,
});

fn table() -> MutexGuard<'static, Table> {
    TABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn clean(path: &Path) -> PathBuf {
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for component in full.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

pub struct Mount {
    id: u64,
    layer: Shared,
}

impl Mount {
    pub fn changed(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let layer = self.layer.lock().unwrap_or_else(|e| e.into_inner());
        layer
            .changed
            .iter()
            .filter_map(|path| Some((path.clone(), layer.files.get(path)?.to_vec())))
            .collect()
    }

    pub fn get(&self, path: &Path) -> Option<Vec<u8>> {
        let layer = self.layer.lock().unwrap_or_else(|e| e.into_inner());
        layer.files.get(&clean(path)).map(|bytes| bytes.to_vec())
    }

    pub fn settle(&self) {
        let mut layer = self.layer.lock().unwrap_or_else(|e| e.into_inner());
        layer.changed.clear();
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        table().mounts.retain(|(id, _)| *id != self.id);
    }
}

pub fn mount(files: BTreeMap<PathBuf, Vec<u8>>) -> Mount {
    let layer = Layer {
        files: files
            .into_iter()
            .map(|(path, bytes)| (clean(&path), Arc::from(bytes)))
            .collect(),
        changed: BTreeSet::new(),
    };
    let layer = Arc::new(Mutex::new(layer));
    let mut table = table();
    table.next += 1;
    let id = table.next;
    table.mounts.push((id, layer.clone()));
    Mount { id, layer }
}

pub struct Recorder(());

impl Recorder {
    pub fn seen(&self) -> BTreeSet<PathBuf> {
        RECORDING.with(|r| r.borrow().clone().unwrap_or_default())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        RECORDING.with(|r| *r.borrow_mut() = None);
    }
}

thread_local! {
    static RECORDING: RefCell<Option<BTreeSet<PathBuf>>> = const { RefCell::new(None) };
}

pub fn record() -> Recorder {
    RECORDING.with(|r| *r.borrow_mut() = Some(BTreeSet::new()));
    Recorder(())
}

fn layer_of(path: &Path) -> Option<(Shared, PathBuf)> {
    let table = table();
    if table.mounts.is_empty() {
        return None;
    }
    let key = clean(path);
    table
        .mounts
        .iter()
        .rev()
        .find(|(_, layer)| {
            layer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .files
                .contains_key(&key)
        })
        .map(|(_, layer)| (layer.clone(), key))
}

fn noted(path: &Path) {
    RECORDING.with(|r| {
        if let Some(seen) = r.borrow_mut().as_mut() {
            seen.insert(clean(path));
        }
    });
}

pub fn mounted(path: &Path) -> bool {
    layer_of(path).is_some()
}

pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    if let Some((layer, key)) = layer_of(path) {
        let layer = layer.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bytes) = layer.files.get(&key) {
            return Ok(bytes.to_vec());
        }
    }
    let bytes = std::fs::read(path)?;
    noted(path);
    Ok(bytes)
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn is_file(path: &Path) -> bool {
    mounted(path) || path.is_file()
}

pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some((layer, key)) = layer_of(path) {
        let mut layer = layer.lock().unwrap_or_else(|e| e.into_inner());
        if layer.files.get(&key).map(|b| &b[..]) != Some(bytes) {
            layer.files.insert(key.clone(), Arc::from(bytes));
            layer.changed.insert(key);
        }
        return Ok(());
    }
    let name = path
        .file_name()
        .map_or_else(|| "file".to_string(), |n| n.to_string_lossy().into_owned());
    let temp = path.with_file_name(format!(".{name}.manfred-tmp"));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}
