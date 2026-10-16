use crate::Error;
use crate::cards::{Layout, Picture};
use crate::light::Backdrop;
use manfred_recipe::Sky;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

#[derive(Clone, Default)]
pub struct Memory(Arc<Mutex<Kept>>);

pub(crate) type Stamp = Option<(SystemTime, u64)>;

type KeptSky = (
    Option<PathBuf>,
    Sky,
    Stamp,
    Option<(usize, u64)>,
    Arc<Backdrop>,
);

pub(crate) fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

#[derive(Default)]
struct Kept {
    sky: Option<KeptSky>,
    pictures: BTreeMap<PathBuf, (Stamp, u64, Arc<Picture>)>,
    layout: Option<Layout>,
    sky_loads: usize,
    decodes: usize,
    packs: usize,
}

pub(crate) fn digest(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl Memory {
    pub fn sky_loads(&self) -> usize {
        self.kept().sky_loads
    }

    pub fn decodes(&self) -> usize {
        self.kept().decodes
    }

    pub fn packs(&self) -> usize {
        self.kept().packs
    }

    pub(crate) fn packed(&self) {
        self.kept().packs += 1;
    }

    pub(crate) fn layout(&self, key: u64) -> Option<Layout> {
        self.kept()
            .layout
            .as_ref()
            .filter(|layout| layout.key == key)
            .cloned()
    }

    pub(crate) fn keep_layout(&self, layout: &Layout) {
        self.kept().layout = Some(layout.clone());
    }

    pub(crate) fn forget_layout(&self) {
        self.kept().layout = None;
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn backdrop(
        &self,
        base: &Option<PathBuf>,
        sky: &Sky,
        file: Option<&Path>,
        load: impl FnOnce() -> Result<Backdrop, Error>,
    ) -> Result<Arc<Backdrop>, Error> {
        let moved = file.and_then(stamp);
        let held = self
            .kept()
            .sky
            .as_ref()
            .filter(|(kept_base, kept_sky, held, _, _)| {
                moved.is_some() && *held == moved && kept_base == base && kept_sky == sky
            })
            .map(|(_, _, _, content, _)| *content);
        let stamp = match held {
            Some(content) => content,
            None => file
                .and_then(|file| manfred_recipe::files::read(file).ok())
                .map(|bytes| (bytes.len(), digest(&bytes))),
        };
        let mut kept = self.kept();
        if let Some((kept_base, kept_sky, _, kept_stamp, backdrop)) = &kept.sky
            && kept_base == base
            && kept_sky == sky
            && *kept_stamp == stamp
        {
            return Ok(backdrop.clone());
        }
        kept.sky = None;
        kept.sky_loads += 1;
        let backdrop = Arc::new(load()?);
        kept.sky = Some((base.clone(), sky.clone(), moved, stamp, backdrop.clone()));
        Ok(backdrop)
    }

    pub(crate) fn kept_picture(&self, file: &Path, stamp: Stamp) -> Option<(u64, Arc<Picture>)> {
        stamp?;
        self.kept()
            .pictures
            .get(file)
            .filter(|(held, _, _)| *held == stamp)
            .map(|(_, hash, picture)| (*hash, picture.clone()))
    }

    pub(crate) fn picture(
        &self,
        file: &Path,
        stamp: Stamp,
        bytes: &[u8],
        decode: impl FnOnce(&[u8]) -> Result<Picture, String>,
    ) -> Result<(u64, Arc<Picture>), String> {
        let hash = digest(bytes);
        let mut kept = self.kept();
        if let Some((held, kept_hash, picture)) = kept.pictures.get_mut(file)
            && *kept_hash == hash
        {
            *held = stamp;
            return Ok((hash, picture.clone()));
        }
        kept.pictures.remove(file);
        kept.decodes += 1;
        let picture = Arc::new(decode(bytes)?);
        kept.pictures
            .insert(file.to_path_buf(), (stamp, hash, picture.clone()));
        Ok((hash, picture))
    }

    pub(crate) fn retain(&self, used: &[&Path]) {
        self.kept()
            .pictures
            .retain(|file, _| used.contains(&file.as_path()));
    }
}
