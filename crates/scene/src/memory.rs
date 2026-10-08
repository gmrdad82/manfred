use crate::Error;
use crate::cards::{Layout, Picture};
use crate::light::Backdrop;
use manfred_recipe::Sky;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[derive(Clone, Default)]
pub struct Memory(Arc<Mutex<Kept>>);

type KeptSky = (Option<PathBuf>, Sky, Option<(usize, u64)>, Arc<Backdrop>);

#[derive(Default)]
struct Kept {
    sky: Option<KeptSky>,
    pictures: BTreeMap<PathBuf, (u64, Arc<Picture>)>,
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
        let stamp = file
            .and_then(|file| std::fs::read(file).ok())
            .map(|bytes| (bytes.len(), digest(&bytes)));
        let mut kept = self.kept();
        if let Some((kept_base, kept_sky, kept_stamp, backdrop)) = &kept.sky
            && kept_base == base
            && kept_sky == sky
            && *kept_stamp == stamp
        {
            return Ok(backdrop.clone());
        }
        kept.sky = None;
        kept.sky_loads += 1;
        let backdrop = Arc::new(load()?);
        kept.sky = Some((base.clone(), sky.clone(), stamp, backdrop.clone()));
        Ok(backdrop)
    }

    pub(crate) fn picture(
        &self,
        file: &Path,
        bytes: &[u8],
        decode: impl FnOnce(&[u8]) -> Result<Picture, String>,
    ) -> Result<(u64, Arc<Picture>), String> {
        let hash = digest(bytes);
        let mut kept = self.kept();
        if let Some((kept_hash, picture)) = kept.pictures.get(file)
            && *kept_hash == hash
        {
            return Ok((hash, picture.clone()));
        }
        kept.pictures.remove(file);
        kept.decodes += 1;
        let picture = Arc::new(decode(bytes)?);
        kept.pictures
            .insert(file.to_path_buf(), (hash, picture.clone()));
        Ok((hash, picture))
    }

    pub(crate) fn retain(&self, used: &[&Path]) {
        self.kept()
            .pictures
            .retain(|file, _| used.contains(&file.as_path()));
    }
}
