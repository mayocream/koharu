use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail};
use image::DynamicImage;
use koharu_scene::{BlobId, Commands, ElementId, Frame, Page, PageAsset, PageId, Revision};

use crate::{CancellationToken, Scope};

#[derive(Clone)]
pub struct Context {
    revision: Revision,
    scope: Scope,
    pages: Arc<[Page]>,
    blobs: Arc<HashMap<BlobId, Arc<[u8]>>>,
    decoded: Arc<Mutex<HashMap<BlobId, Arc<DynamicImage>>>>,
    target_language: Option<String>,
    instructions: Option<String>,
    cancellation: CancellationToken,
}

pub(crate) struct ContextOptions {
    pub target_language: Option<String>,
    pub instructions: Option<String>,
    pub cancellation: CancellationToken,
}

impl Context {
    pub(crate) fn new(
        revision: Revision,
        scope: Scope,
        pages: Vec<Page>,
        blobs: HashMap<BlobId, Arc<[u8]>>,
        decoded: Arc<Mutex<HashMap<BlobId, Arc<DynamicImage>>>>,
        options: ContextOptions,
    ) -> Self {
        Self {
            revision,
            scope,
            pages: pages.into(),
            blobs: Arc::new(blobs),
            decoded,
            target_language: options.target_language,
            instructions: options.instructions,
            cancellation: options.cancellation,
        }
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

    pub fn blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
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
            image::load_from_memory(&bytes)
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
    pub fn target_language(&self) -> Option<&str> {
        self.target_language.as_deref()
    }

    #[must_use]
    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }

    #[must_use]
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    #[must_use]
    pub fn commands(&self) -> Commands {
        Commands::new(self.revision)
    }

    #[must_use]
    pub fn includes_element(&self, page: PageId, element: ElementId, frame: Frame) -> bool {
        match &self.scope {
            Scope::Project | Scope::Pages(_) => self.page(page).is_some(),
            Scope::Region {
                page: scoped_page,
                frame: region,
            } => *scoped_page == page && intersects(*region, frame),
            Scope::Elements(elements) => elements.contains(&element),
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
