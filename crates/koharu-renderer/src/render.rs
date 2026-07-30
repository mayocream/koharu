//! High-level renderer facade and revision-aware prepared-page cache.

use std::{collections::VecDeque, sync::Arc};

use image::RgbaImage;
use koharu_scene::{
    EntityChange, EntityId, RelationChange, Revision, SceneChangeSet, SceneSnapshot,
};
use parking_lot::Mutex;

use crate::{
    Error, PreparedPage, RasterOptions, RenderDependency, RenderDiagnostic, RenderPlan,
    RenderRequest, RenderResources, RenderedEntity, Result, WgpuRenderer,
};

const DEFAULT_PREPARED_PAGES: usize = 8;

#[derive(Debug)]
pub struct RenderOutput {
    pub image: RgbaImage,
    pub revision: Revision,
    pub page: EntityId,
    pub entities: Vec<RenderedEntity>,
    pub dependencies: Vec<RenderDependency>,
    pub diagnostics: Vec<RenderDiagnostic>,
}

pub struct Renderer {
    resources: Arc<RenderResources>,
    rasterizer: WgpuRenderer,
    prepared: Mutex<PreparedCache>,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        Self::with_resources(Arc::new(RenderResources::new()))
    }

    pub fn with_resources(resources: Arc<RenderResources>) -> Result<Self> {
        let rasterizer = WgpuRenderer::new().map_err(Error::Backend)?;
        Ok(Self::from_parts(resources, rasterizer))
    }

    #[must_use]
    pub fn from_parts(resources: Arc<RenderResources>, rasterizer: WgpuRenderer) -> Self {
        Self {
            resources,
            rasterizer,
            prepared: Mutex::new(PreparedCache::new(DEFAULT_PREPARED_PAGES)),
        }
    }

    #[must_use]
    pub fn resources(&self) -> &Arc<RenderResources> {
        &self.resources
    }

    pub fn compile(&self, snapshot: &SceneSnapshot, request: &RenderRequest) -> Result<RenderPlan> {
        RenderPlan::compile(snapshot, request)
    }

    pub fn prepare(
        &self,
        snapshot: &SceneSnapshot,
        request: &RenderRequest,
    ) -> Result<Arc<PreparedPage>> {
        let key = PrepareKey::new(
            snapshot.revision(),
            self.resources.fonts().generation(),
            request,
        );
        if let Some(prepared) = self.prepared.lock().get(&key) {
            return Ok(prepared);
        }
        let plan = self.compile(snapshot, request)?;
        let prepared = Arc::new(PreparedPage::prepare(
            &plan,
            snapshot,
            &self.resources,
            &request.theme,
        )?);
        Ok(self.prepared.lock().insert(key, prepared))
    }

    pub fn render(
        &self,
        snapshot: &SceneSnapshot,
        request: &RenderRequest,
    ) -> Result<RenderOutput> {
        let prepared = self.prepare(snapshot, request)?;
        let (width, height) = prepared.size();
        let image = self
            .rasterizer
            .rasterize(
                prepared.scene(),
                width,
                height,
                [0, 0, 0, 0],
                request.raster,
            )
            .map_err(Error::Backend)?;
        Ok(RenderOutput {
            image,
            revision: prepared.revision(),
            page: prepared.page(),
            entities: prepared.entities().to_vec(),
            dependencies: prepared.dependencies().to_vec(),
            diagnostics: prepared.diagnostics().to_vec(),
        })
    }

    pub fn clear_cache(&self) {
        self.prepared.lock().clear();
    }

    /// Advances unaffected cache entries to the new revision and removes stale ones.
    pub fn apply_changes(&self, changes: &SceneChangeSet) {
        self.prepared.lock().apply_changes(changes);
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PrepareKey {
    revision: Revision,
    font_generation: u64,
    request: RenderRequest,
}

impl PrepareKey {
    fn new(revision: Revision, font_generation: u64, request: &RenderRequest) -> Self {
        let mut request = request.clone();
        request.raster = RasterOptions::default();
        Self {
            revision,
            font_generation,
            request,
        }
    }
}

struct PreparedEntry {
    key: PrepareKey,
    page: Arc<PreparedPage>,
}

struct PreparedCache {
    entries: VecDeque<PreparedEntry>,
    capacity: usize,
}

impl PreparedCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn get(&mut self, key: &PrepareKey) -> Option<Arc<PreparedPage>> {
        let position = self.entries.iter().position(|entry| entry.key == *key)?;
        let entry = self.entries.remove(position)?;
        let page = entry.page.clone();
        self.entries.push_back(entry);
        Some(page)
    }

    fn insert(&mut self, key: PrepareKey, page: Arc<PreparedPage>) -> Arc<PreparedPage> {
        if let Some(existing) = self.get(&key) {
            return existing;
        }
        if self.capacity == 0 {
            return page;
        }
        while self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(PreparedEntry {
            key,
            page: page.clone(),
        });
        page
    }

    fn apply_changes(&mut self, changes: &SceneChangeSet) {
        let invalidate_all = changes.pages_changed
            || !changes.relations.is_empty()
            || changes
                .entities
                .iter()
                .any(|change| matches!(change, EntityChange::Inserted(_)));
        if invalidate_all {
            self.entries
                .retain(|entry| entry.key.revision != changes.from);
            return;
        }
        self.entries.retain_mut(|entry| {
            if entry.key.revision != changes.from {
                return true;
            }
            let depends_on_entity = |entity| {
                entry
                    .page
                    .dependencies()
                    .contains(&RenderDependency::Entity(entity))
            };
            let entity_changed = changes.entities.iter().any(|change| match *change {
                EntityChange::Inserted(entity) | EntityChange::Removed(entity) => {
                    depends_on_entity(entity)
                }
            }) || changes
                .components
                .iter()
                .any(|change| depends_on_entity(change.entity));
            let relation_changed = changes.relations.iter().any(|change| {
                let relation = match *change {
                    RelationChange::Inserted(id)
                    | RelationChange::Removed(id)
                    | RelationChange::Changed(id) => id,
                };
                entry
                    .page
                    .dependencies()
                    .contains(&RenderDependency::Relation(relation))
            });
            if entity_changed || relation_changed {
                false
            } else {
                entry.key.revision = changes.to;
                entry.page = Arc::new(entry.page.at_revision(changes.to));
                true
            }
        });
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_scene::{At, Geometry, PageDraft, SceneSession};

    fn prepared(
        revision: Revision,
        page: EntityId,
        dependencies: Vec<RenderDependency>,
    ) -> Arc<PreparedPage> {
        Arc::new(PreparedPage::empty_for_test(revision, page, dependencies))
    }

    #[test]
    fn change_sets_reuse_only_unaffected_prepared_pages() {
        let mut session = SceneSession::memory().unwrap();
        let mut ids = None;
        let create = session
            .snapshot()
            .patch(|edit| {
                let first_page = edit.add_page(PageDraft::new("first", 100.0, 100.0), At::End)?;
                let first_entity = edit.add_entity(first_page, At::End)?;
                edit.set(
                    first_entity,
                    "default",
                    &Geometry::rectangle(0.0, 0.0, 10.0, 10.0),
                )?;
                let second_page = edit.add_page(PageDraft::new("second", 100.0, 100.0), At::End)?;
                let second_entity = edit.add_entity(second_page, At::End)?;
                edit.set(
                    second_entity,
                    "default",
                    &Geometry::rectangle(0.0, 0.0, 10.0, 10.0),
                )?;
                ids = Some((first_page, first_entity, second_entity));
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(create).unwrap().snapshot;
        let (first_page, first_entity, second_entity) = ids.unwrap();
        let request = RenderRequest::transparent(first_page);
        let mut cache = PreparedCache::new(2);
        cache.insert(
            PrepareKey::new(snapshot.revision(), 0, &request),
            prepared(
                snapshot.revision(),
                first_page,
                vec![
                    RenderDependency::Entity(first_page),
                    RenderDependency::Entity(first_entity),
                ],
            ),
        );

        let unrelated = snapshot
            .patch(|edit| {
                edit.set(
                    second_entity,
                    "default",
                    &Geometry::rectangle(1.0, 1.0, 10.0, 10.0),
                )
            })
            .unwrap();
        let commit = session.commit(unrelated).unwrap();
        cache.apply_changes(&commit.changes);
        let reused = cache
            .get(&PrepareKey::new(commit.snapshot.revision(), 0, &request))
            .expect("unaffected page should be advanced to the new revision");
        assert_eq!(reused.revision(), commit.snapshot.revision());

        let relevant = commit
            .snapshot
            .patch(|edit| {
                edit.set(
                    first_entity,
                    "default",
                    &Geometry::rectangle(2.0, 2.0, 10.0, 10.0),
                )
            })
            .unwrap();
        let commit = session.commit(relevant).unwrap();
        cache.apply_changes(&commit.changes);

        assert!(
            cache
                .get(&PrepareKey::new(commit.snapshot.revision(), 0, &request))
                .is_none()
        );
    }
}
