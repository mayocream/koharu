use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail};
use image::DynamicImage;
use koharu_scene::{BlobId, Commands, ElementId, Frame, Page, PageAsset, PageId, Revision};

use crate::{CancellationToken, EventSink, ModelMeasurement, Phase, PipelineEvent, Scope};

#[derive(Clone)]
pub enum BlobBytes {
    Owned(Arc<[u8]>),
    Shared(koharu_worker::SharedBytes),
}

pub(crate) struct SharedSnapshot {
    _arena: Option<koharu_worker::ArenaFile>,
    pub descriptor: Option<koharu_worker::ArenaDescriptor>,
    pub blobs: Vec<(BlobId, koharu_worker::SharedSlice)>,
}

impl AsRef<[u8]> for BlobBytes {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Shared(bytes) => bytes.as_ref(),
        }
    }
}

#[derive(Clone)]
pub struct Context {
    phase: Option<Phase>,
    revision: Revision,
    scope: Scope,
    pages: Arc<[Page]>,
    blobs: Arc<HashMap<BlobId, BlobBytes>>,
    decoded: Arc<Mutex<HashMap<BlobId, Arc<DynamicImage>>>>,
    options: ContextOptions,
    shared: Arc<Mutex<Option<Arc<SharedSnapshot>>>>,
}

#[derive(Clone)]
pub(crate) struct ContextOptions {
    pub translation: TranslationOptions,
    pub cancellation: CancellationToken,
    pub events: Option<EventSink>,
    pub measurements: Arc<Mutex<Vec<ModelMeasurement>>>,
}

#[derive(Clone, Default)]
pub(crate) struct TranslationOptions {
    pub target_language: String,
    pub instructions: Option<String>,
}

impl Context {
    pub(crate) fn new(
        revision: Revision,
        scope: Scope,
        pages: Vec<Page>,
        blobs: HashMap<BlobId, BlobBytes>,
        decoded: Arc<Mutex<HashMap<BlobId, Arc<DynamicImage>>>>,
        options: ContextOptions,
    ) -> Self {
        Self {
            phase: None,
            revision,
            scope,
            pages: pages.into(),
            blobs: Arc::new(blobs),
            decoded,
            options,
            shared: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn for_phase(&self, phase: Phase) -> Self {
        let mut context = self.clone();
        context.phase = Some(phase);
        context
    }

    pub(crate) fn phase(&self) -> Phase {
        self.phase.expect("execution context has a phase")
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn scope(&self) -> &Scope {
        &self.scope
    }

    #[must_use]
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    #[must_use]
    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.iter().find(|page| page.id == id)
    }

    pub fn blob(&self, id: BlobId) -> Result<BlobBytes> {
        self.blobs
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow!("blob {id} is outside the captured pipeline input"))
    }

    pub fn image(&self, id: BlobId) -> Result<Arc<DynamicImage>> {
        let mut decoded = self
            .decoded
            .lock()
            .map_err(|_| anyhow!("pipeline image cache lock is poisoned"))?;
        if let Some(image) = decoded.get(&id) {
            return Ok(image.clone());
        }
        let bytes = self.blob(id)?;
        let image = Arc::new(
            image::load_from_memory(bytes.as_ref())
                .with_context(|| format!("failed to decode scene blob {id}"))?,
        );
        decoded.insert(id, image.clone());
        Ok(image)
    }

    pub fn source(&self, page: PageId) -> Result<Arc<DynamicImage>> {
        let page = self
            .page(page)
            .ok_or_else(|| anyhow!("page {page} is outside the pipeline scope"))?;
        self.image(page.source)
    }

    pub fn asset(&self, page: PageId, asset: PageAsset) -> Result<Option<Arc<DynamicImage>>> {
        let page = self
            .page(page)
            .ok_or_else(|| anyhow!("page {page} is outside the pipeline scope"))?;
        page.assets.get(asset).map(|id| self.image(id)).transpose()
    }

    #[must_use]
    pub fn target_language(&self) -> &str {
        &self.options.translation.target_language
    }

    #[must_use]
    pub fn instructions(&self) -> Option<&str> {
        self.options.translation.instructions.as_deref()
    }

    #[must_use]
    pub fn cancellation(&self) -> &CancellationToken {
        &self.options.cancellation
    }

    pub(crate) fn shared_snapshot(&self, directory: &Path) -> Result<Arc<SharedSnapshot>> {
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| anyhow!("pipeline shared snapshot lock is poisoned"))?;
        if let Some(snapshot) = shared.as_ref() {
            return Ok(snapshot.clone());
        }
        let mut blobs = self.blobs.iter().collect::<Vec<_>>();
        blobs.sort_by_key(|(id, _)| **id);
        let (arena, descriptor, slices) = if blobs.is_empty() {
            (None, None, Vec::new())
        } else {
            let (arena, slices) = koharu_worker::ArenaFile::create(
                directory,
                blobs.iter().map(|(_, bytes)| (*bytes).as_ref()),
            )?;
            let descriptor = arena.descriptor().clone();
            (Some(arena), Some(descriptor), slices)
        };
        let blobs = blobs.into_iter().map(|(id, _)| *id).zip(slices).collect();
        let snapshot = Arc::new(SharedSnapshot {
            _arena: arena,
            descriptor,
            blobs,
        });
        *shared = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub(crate) fn emit(&self, event: PipelineEvent) {
        if let Some(events) = &self.options.events {
            events(event);
        }
    }

    pub(crate) fn event_sink(&self) -> Option<EventSink> {
        self.options.events.clone()
    }

    pub(crate) fn record_measurement(&self, measurement: ModelMeasurement) {
        self.options
            .measurements
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(measurement.clone());
        self.emit(PipelineEvent::Measurement(measurement));
    }

    #[must_use]
    pub fn commands(&self) -> Commands {
        Commands::new(self.revision)
    }

    #[must_use]
    pub fn includes_element(&self, page: PageId, element: ElementId, frame: Frame) -> bool {
        match &self.scope {
            Scope::Project | Scope::Pages { .. } => self.page(page).is_some(),
            Scope::Region {
                page: scoped_page,
                frame: region,
            } => *scoped_page == page && intersects(*region, frame),
            Scope::Elements { elements } => elements.contains(&element),
        }
    }

    #[must_use]
    pub fn region(&self, page: PageId) -> Option<Frame> {
        match self.scope {
            Scope::Region {
                page: scoped_page,
                frame,
            } if scoped_page == page => Some(frame),
            _ => None,
        }
    }

    pub(crate) fn validate_scope(&self) -> Result<()> {
        if let Scope::Region { frame, .. } = self.scope
            && (!frame.x.is_finite()
                || !frame.y.is_finite()
                || !frame.width.is_finite()
                || frame.width <= 0.0
                || !frame.height.is_finite()
                || frame.height <= 0.0)
        {
            bail!("pipeline region must be finite and non-empty");
        }
        Ok(())
    }
}

#[must_use]
pub(crate) fn intersects(left: Frame, right: Frame) -> bool {
    left.x < right.x + right.width
        && right.x < left.x + left.width
        && left.y < right.y + right.height
        && right.y < left.y + left.height
}
