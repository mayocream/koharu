use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{Context as _, Result};
use koharu_scene::{Asset, BlobId, EntityId, SceneSnapshot};

type SharedCell<T> = Arc<OnceLock<std::result::Result<Arc<T>, String>>>;

#[derive(Default)]
pub(crate) struct RunCache {
    blobs: Mutex<HashMap<BlobId, SharedCell<[u8]>>>,
    images: Mutex<HashMap<BlobId, SharedCell<image::DynamicImage>>>,
}

impl RunCache {
    pub(crate) fn blob(&self, scene: &SceneSnapshot, id: BlobId) -> Result<Arc<[u8]>> {
        let cell = self
            .blobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(id)
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone();
        cell.get_or_init(|| scene.read_blob(id).map_err(|error| error.to_string()))
            .clone()
            .map_err(anyhow::Error::msg)
    }

    pub(crate) fn asset(
        &self,
        scene: &SceneSnapshot,
        entity: EntityId,
        role: &str,
    ) -> Result<Option<Asset>> {
        scene.component(entity, role).map_err(Into::into)
    }

    pub(crate) fn image(
        &self,
        scene: &SceneSnapshot,
        entity: EntityId,
        role: &str,
    ) -> Result<Option<Arc<image::DynamicImage>>> {
        let Some(asset) = self.asset(scene, entity, role)? else {
            return Ok(None);
        };
        let cell = self
            .images
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(asset.blob)
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone();
        let image = cell
            .get_or_init(|| {
                let bytes = self
                    .blob(scene, asset.blob)
                    .map_err(|error| error.to_string())?;
                image::load_from_memory(&bytes)
                    .map(Arc::new)
                    .with_context(|| format!("failed to decode {role} image for entity {entity}"))
                    .map_err(|error| error.to_string())
            })
            .clone()
            .map_err(anyhow::Error::msg)?;
        Ok(Some(image))
    }
}
