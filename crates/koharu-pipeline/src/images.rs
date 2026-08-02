use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result};
use koharu_scene::{AssetRole, BlobId, EntityId, Snapshot};

#[derive(Default)]
pub(crate) struct ImageCache {
    decoded: Mutex<HashMap<BlobId, Arc<image::DynamicImage>>>,
}

impl ImageCache {
    pub(crate) fn get(
        &self,
        scene: &Snapshot,
        entity: EntityId,
        role: &str,
    ) -> Result<Option<Arc<image::DynamicImage>>> {
        let role = AssetRole::new(role)?;
        let Some(asset) = scene.asset(entity, &role)? else {
            return Ok(None);
        };
        if let Some(image) = self
            .decoded
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&asset.blob)
            .cloned()
        {
            return Ok(Some(image));
        }

        let bytes = scene.read_blob(asset.blob)?;
        let image = Arc::new(image::load_from_memory(&bytes).with_context(|| {
            format!(
                "failed to decode {} image for entity {entity}",
                role.as_str()
            )
        })?);
        let image = self
            .decoded
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(asset.blob)
            .or_insert(image)
            .clone();
        Ok(Some(image))
    }
}
