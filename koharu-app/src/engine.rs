//! Engine system — pluggable, type-safe, auto-wired pipeline.
//!
//! An **engine** is a loadable model that transforms a document page.
//! Engines declare what artifacts they need and produce. The DAG is
//! derived automatically and engines are loaded on demand.
//!
//! ## Adding an engine
//!
//! 1. Define a struct holding your model.
//! 2. Implement [`Engine`] for it.
//! 3. Register it with [`inventory::submit!`] via [`EngineInfo`].

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use async_trait::async_trait;
use koharu_core::{Document, TextShaderEffect, TextStrokeStyle};
use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use tokio::sync::RwLock;
use tracing::Instrument;

use crate::AppResources;

// ---------------------------------------------------------------------------
// Artifact — what engines produce and consume
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Artifact {
    TextBlocks,
    Bubbles,
    Segment,
    FontPredictions,
    OcrText,
    Translations,
    Inpainted,
    Rendered,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineRunOptions {
    pub target_language: Option<String>,
    pub system_prompt: Option<String>,
    pub shader_effect: Option<TextShaderEffect>,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub default_font: Option<String>,
}

impl PipelineRunOptions {
    pub fn from_process_request(req: &koharu_core::commands::ProcessRequest) -> Self {
        Self {
            target_language: req.language.clone(),
            system_prompt: req.system_prompt.clone(),
            shader_effect: req.shader_effect,
            shader_stroke: req.shader_stroke.clone(),
            default_font: req.default_font.clone(),
        }
    }
}

fn has_non_empty_text(value: Option<&str>) -> bool {
    value.is_some_and(|text| !text.trim().is_empty())
}

fn block_has_ocr_text(block: &koharu_core::TextBlock) -> bool {
    has_non_empty_text(block.text.as_deref())
}

fn block_has_translation(block: &koharu_core::TextBlock) -> bool {
    if !block_has_ocr_text(block) {
        return true;
    }

    has_non_empty_text(block.translation.as_deref())
}

impl Artifact {
    pub fn ready(&self, doc: &Document) -> bool {
        match self {
            Self::TextBlocks => !doc.text_blocks.is_empty(),
            Self::Bubbles => !doc.bubbles.is_empty(),
            Self::Segment => doc.segment.is_some(),
            Self::FontPredictions => {
                doc.text_blocks.is_empty()
                    || doc.text_blocks.iter().all(|b| b.font_prediction.is_some())
            }
            Self::OcrText => {
                doc.text_blocks.is_empty() || doc.text_blocks.iter().all(block_has_ocr_text)
            }
            Self::Translations => {
                doc.text_blocks.is_empty() || doc.text_blocks.iter().all(block_has_translation)
            }
            Self::Inpainted => doc.inpainted.is_some(),
            Self::Rendered => doc.rendered.is_some(),
        }
    }
}

// ---------------------------------------------------------------------------
// Patch — document mutation returned by an engine
// ---------------------------------------------------------------------------

type PatchFn = Box<dyn FnOnce(&mut Document) + Send>;

pub struct Patch(Option<PatchFn>);

impl Patch {
    pub fn none() -> Self {
        Self(None)
    }

    pub fn apply(f: impl FnOnce(&mut Document) + Send + 'static) -> Self {
        Self(Some(Box::new(f)))
    }

    pub fn take(self) -> Option<PatchFn> {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Engine trait — typed model logic
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Engine: Send + Sync + 'static {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        options: &PipelineRunOptions,
    ) -> Result<Patch>;
}

// ---------------------------------------------------------------------------
// EngineInfo — static descriptor + factory (registered via inventory)
// ---------------------------------------------------------------------------

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type LoadFn = for<'a> fn(&'a AppResources) -> BoxFuture<'a, Result<Box<dyn Engine>>>;

pub struct EngineInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub needs: &'static [Artifact],
    pub produces: &'static [Artifact],
    pub load: LoadFn,
}

inventory::collect!(EngineInfo);

// ---------------------------------------------------------------------------
// Registry — lazy load + cache engine instances
// ---------------------------------------------------------------------------

pub struct Registry {
    engines: RwLock<HashMap<&'static str, Arc<dyn Engine>>>,
    models: RwLock<HashMap<&'static str, Arc<dyn std::any::Any + Send + Sync>>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            engines: RwLock::new(HashMap::new()),
            models: RwLock::new(HashMap::new()),
        }
    }
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or load an engine instance.
    pub async fn get(&self, id: &str, res: &AppResources) -> Result<Arc<dyn Engine>> {
        if let Some(engine) = self.engines.read().await.get(id) {
            return Ok(engine.clone());
        }
        let info = Self::find(id)?;
        let engine: Arc<dyn Engine> = Arc::from(
            async { (info.load)(res).await }
                .instrument(tracing::info_span!("engine_load", engine = id))
                .await?,
        );
        self.engines.write().await.insert(info.id, engine.clone());
        Ok(engine)
    }

    /// Get or load a shared typed model (for direct access outside the pipeline).
    pub async fn model<T, F, Fut>(&self, key: &'static str, load: F) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        if let Some(cached) = self.models.read().await.get(key) {
            return cached
                .clone()
                .downcast::<T>()
                .map_err(|_| anyhow::anyhow!("model type mismatch: {key}"));
        }
        let m = Arc::new(load().await?);
        self.models.write().await.insert(key, m.clone());
        Ok(m)
    }

    /// Drop all cached engines and models, freeing GPU memory.
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn clear(&self) {
        self.engines.write().await.clear();
        self.models.write().await.clear();
    }

    /// Find engine descriptor by id.
    pub fn find(id: &str) -> Result<&'static EngineInfo> {
        Self::catalog()
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown engine: {id}"))
    }

    /// All registered engine descriptors.
    pub fn catalog() -> Vec<&'static EngineInfo> {
        inventory::iter::<EngineInfo>.into_iter().collect()
    }

    /// Engines that produce a given artifact.
    pub fn providers(artifact: Artifact) -> Vec<&'static EngineInfo> {
        Self::catalog()
            .into_iter()
            .filter(|e| e.produces.contains(&artifact))
            .collect()
    }
}

/// Build the engine catalog for the API.
pub fn catalog() -> koharu_core::EngineCatalog {
    use koharu_core::{EngineCatalog, EngineCatalogEntry};

    let entry = |info: &&EngineInfo| EngineCatalogEntry {
        id: info.id.to_string(),
        name: info.name.to_string(),
        produces: info.produces.iter().map(|a| format!("{a:?}")).collect(),
    };

    EngineCatalog {
        detectors: Registry::providers(Artifact::TextBlocks)
            .iter()
            .map(entry)
            .collect(),
        bubble_detectors: Registry::providers(Artifact::Bubbles)
            .iter()
            .map(entry)
            .collect(),
        font_detectors: Registry::providers(Artifact::FontPredictions)
            .iter()
            .map(entry)
            .collect(),
        segmenters: Registry::providers(Artifact::Segment)
            .iter()
            .map(entry)
            .collect(),
        ocr: Registry::providers(Artifact::OcrText)
            .iter()
            .map(entry)
            .collect(),
        translators: Registry::providers(Artifact::Translations)
            .iter()
            .map(entry)
            .collect(),
        inpainters: Registry::providers(Artifact::Inpainted)
            .iter()
            .map(entry)
            .collect(),
        renderers: Registry::providers(Artifact::Rendered)
            .iter()
            .map(entry)
            .collect(),
    }
}

/// Resolve pipeline config to a list of engine IDs.
/// Collects all non-empty engine IDs, deduplicates, and lets the DAG sort order.
pub fn resolve_pipeline(config: &crate::config::PipelineConfig) -> Vec<&'static str> {
    let candidates = [
        &config.detector,
        &config.bubble_detector,
        &config.font_detector,
        &config.segmenter,
        &config.ocr,
        &config.translator,
        &config.inpainter,
        &config.renderer,
    ];
    let mut ids: Vec<&str> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in candidates {
        if !id.is_empty()
            && let Ok(info) = Registry::find(id)
            && seen.insert(info.id)
        {
            ids.push(info.id);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// DAG builder — derives execution order from artifact dependencies
// ---------------------------------------------------------------------------

/// Build topological execution order from a set of engine infos.
pub fn build_order(infos: &[&EngineInfo]) -> Result<Vec<usize>> {
    let mut g = DiGraph::<usize, ()>::new();
    let mut id_to_node: HashMap<&str, _> = HashMap::new();

    for (i, info) in infos.iter().enumerate() {
        let n = g.add_node(i);
        if id_to_node.insert(info.id, n).is_some() {
            bail!("duplicate engine: {}", info.id);
        }
    }

    let mut producers: HashMap<Artifact, usize> = HashMap::new();
    for (i, info) in infos.iter().enumerate() {
        for &artifact in info.produces {
            producers.insert(artifact, i);
        }
    }

    for info in infos.iter() {
        let to = id_to_node[info.id];
        for &artifact in info.needs {
            if let Some(&producer) = producers.get(&artifact) {
                g.add_edge(id_to_node[infos[producer].id], to, ());
            }
        }
    }

    let order = toposort(&g, None)
        .map_err(|c| anyhow::anyhow!("cycle at '{}'", infos[g[c.node_id()]].id))?;
    Ok(order.into_iter().map(|n| g[n]).collect())
}

// ---------------------------------------------------------------------------
// Pipeline execution
// ---------------------------------------------------------------------------

/// Resolve engine selection → load engines → build DAG → execute per page.
pub async fn execute_pipeline<F, Fut>(
    selection: &[&str],
    res: &AppResources,
    page_id: &str,
    cancel: &AtomicBool,
    options: &PipelineRunOptions,
    on_step: F,
) -> Result<()>
where
    F: Fn(usize, &str) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let infos: Vec<&EngineInfo> = selection
        .iter()
        .map(|id| Registry::find(id))
        .collect::<Result<_>>()?;
    let order = build_order(&infos)?;

    for (seq, &i) in order.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let info = infos[i];
        let doc = res.storage.page(page_id).await?;
        if info.produces.iter().all(|a| a.ready(&doc)) {
            tracing::info!(step = info.id, "skipped");
            continue;
        }
        on_step(seq, info.id).await;
        async {
            let engine = res.registry.get(info.id, res).await?;
            let patch = engine.run(&doc, res, options).await?;
            if let Some(f) = patch.take() {
                res.storage.update_page(page_id, f).await?;
            }
            let updated = res.storage.page(page_id).await?;
            verify_step_outputs(info, &updated)?;
            Ok::<_, anyhow::Error>(())
        }
        .instrument(tracing::info_span!("step", engine = info.id))
        .await?;
    }
    Ok(())
}

fn verify_step_outputs(info: &EngineInfo, doc: &Document) -> Result<()> {
    let missing = info
        .produces
        .iter()
        .filter(|artifact| !artifact.ready(doc))
        .map(|artifact| format!("{artifact:?}"))
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return Ok(());
    }

    bail!(
        "step '{}' did not produce required artifacts: {}",
        info.id,
        missing.join(", ")
    )
}

/// Run a single engine by id.
#[tracing::instrument(level = "info", skip_all, fields(engine = id))]
pub async fn run_one(id: &str, res: &AppResources, page_id: &str) -> Result<()> {
    let _info = Registry::find(id)?;
    let doc = res.storage.page(page_id).await?;
    let engine = res.registry.get(id, res).await?;
    let options = PipelineRunOptions::default();
    let patch = engine.run(&doc, res, &options).await?;
    if let Some(f) = patch.take() {
        res.storage.update_page(page_id, f).await?;
    }
    Ok(())
}

/// Run multiple engines by id, sequentially.
pub async fn run_many(ids: &[&str], res: &AppResources, page_id: &str) -> Result<()> {
    for &id in ids {
        run_one(id, res, page_id).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Default pipeline selection
// ---------------------------------------------------------------------------

/// Default engine selection for a full pipeline.
pub const DEFAULT_PIPELINE: &[&str] = &[
    "comic-text-bubble-detector",
    "comic-text-detector-seg",
    "yuzumarker-font-detection",
    "paddle-ocr-vl-1.5",
    "llm",
    "aot-inpainting",
    "koharu-renderer",
];

// =========================================================================
// Engine implementations
// =========================================================================

use image::DynamicImage;
use koharu_core::{FontPrediction, SerializableDynamicImage, TextBlock, TextDirection};

// --- PP-DocLayout V3 (detector) -------------------------------------------

struct PpDocLayoutEngine(koharu_ml::pp_doclayout_v3::PPDocLayoutV3);

#[async_trait]
impl Engine for PpDocLayoutEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let source: SerializableDynamicImage = {
            let _s = tracing::info_span!("load_image").entered();
            res.storage.images.load(&doc.source)?.into()
        };
        let blocks = {
            let _s = tracing::info_span!("inference").entered();
            let layout = self.0.inference_one_fast(&source, 0.25)?;
            build_text_blocks(&layout.regions)
        };
        Ok(Patch::apply(|doc| doc.text_blocks = blocks))
    }
}

inventory::submit! {
    EngineInfo {
        id: "pp-doclayout-v3",
        name: "PP-DocLayout V3",
        needs: &[],
        produces: &[Artifact::TextBlocks],
        load: |res| Box::pin(async move {
            let m = koharu_ml::pp_doclayout_v3::PPDocLayoutV3::load(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu)).await?;
            Ok(Box::new(PpDocLayoutEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- CTD Full (detector + segmenter) --------------------------------------

struct CtdFullEngine(koharu_ml::comic_text_detector::ComicTextDetector);

#[async_trait]
impl Engine for CtdFullEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let source: SerializableDynamicImage = {
            let _s = tracing::info_span!("load_image").entered();
            res.storage.images.load(&doc.source)?.into()
        };
        let det = {
            let _s = tracing::info_span!("inference").entered();
            self.0.inference(&source)?
        };
        let blob = {
            let _s = tracing::info_span!("save").entered();
            res.storage
                .images
                .store_webp(&DynamicImage::ImageLuma8(det.mask))?
        };
        let mut blocks = det.text_blocks;
        sort_manga_reading_order(&mut blocks);
        Ok(Patch::apply(move |doc| {
            doc.text_blocks = blocks;
            doc.segment = Some(blob);
        }))
    }
}

inventory::submit! {
    EngineInfo {
        id: "comic-text-detector",
        name: "Comic Text Detector",
        needs: &[],
        produces: &[Artifact::TextBlocks, Artifact::Segment],
        load: |res| Box::pin(async move {
            let m = koharu_ml::comic_text_detector::ComicTextDetector::load(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu)).await?;
            Ok(Box::new(CtdFullEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- CTD Segmentation only ------------------------------------------------

struct CtdSegmentEngine(koharu_ml::comic_text_detector::ComicTextDetector);

#[async_trait]
impl Engine for CtdSegmentEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let source: SerializableDynamicImage = {
            let _s = tracing::info_span!("load_image").entered();
            res.storage.images.load(&doc.source)?.into()
        };
        let mask = {
            let _s = tracing::info_span!("inference").entered();
            let prob = self.0.inference_segmentation(&source)?;
            koharu_ml::comic_text_detector::refine_segmentation_mask(
                &source,
                &prob,
                &doc.text_blocks,
            )
        };
        let blob = {
            let _s = tracing::info_span!("save").entered();
            res.storage
                .images
                .store_webp(&DynamicImage::ImageLuma8(mask))?
        };
        Ok(Patch::apply(|doc| doc.segment = Some(blob)))
    }
}

inventory::submit! {
    EngineInfo {
        id: "comic-text-detector-seg",
        name: "Comic Text Detector (Segmentation)",
        needs: &[Artifact::TextBlocks],
        produces: &[Artifact::Segment],
        load: |res| Box::pin(async move {
            let m = koharu_ml::comic_text_detector::ComicTextDetector::load_segmentation_only(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu)).await?;
            Ok(Box::new(CtdSegmentEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- Comic Text & Bubble Detector (ogkalu RT-DETR) -------------------------

struct ComicTextBubbleDetectorEngine(
    koharu_ml::comic_text_bubble_detector::ComicTextBubbleDetector,
);

#[async_trait]
impl Engine for ComicTextBubbleDetectorEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let source = {
            let _s = tracing::info_span!("load_image").entered();
            res.storage.images.load(&doc.source)?
        };
        let det = {
            let _s = tracing::info_span!("inference").entered();
            self.0.inference(&source)?
        };
        let mut blocks = det.text_blocks;
        sort_manga_reading_order(&mut blocks);
        let bubbles: Vec<koharu_core::BubbleRegion> = det
            .detections
            .iter()
            .filter(|r| r.is_bubble())
            .map(|r| {
                let [x1, y1, x2, y2] = r.bbox;
                koharu_core::BubbleRegion {
                    x: x1.max(0.0),
                    y: y1.max(0.0),
                    width: (x2 - x1).max(0.0),
                    height: (y2 - y1).max(0.0),
                    confidence: r.score,
                }
            })
            .collect();
        Ok(Patch::apply(move |doc| {
            doc.text_blocks = blocks;
            doc.bubbles = bubbles;
        }))
    }
}

inventory::submit! {
    EngineInfo {
        id: "comic-text-bubble-detector",
        name: "Comic Text & Bubble Detector",
        needs: &[],
        produces: &[Artifact::TextBlocks, Artifact::Bubbles],
        load: |res| Box::pin(async move {
            let m = koharu_ml::comic_text_bubble_detector::ComicTextBubbleDetector::load(
                &res.runtime,
                matches!(res.device, koharu_ml::Device::Cpu),
            ).await?;
            Ok(Box::new(ComicTextBubbleDetectorEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- Speech Bubble Segmentation (YOLOv8-seg) --------------------------------

struct SpeechBubbleSegEngine(koharu_ml::speech_bubble_segmentation::SpeechBubbleSegmentation);

#[async_trait]
impl Engine for SpeechBubbleSegEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let source = {
            let _s = tracing::info_span!("load_image").entered();
            res.storage.images.load(&doc.source)?
        };
        let result = {
            let _s = tracing::info_span!("inference").entered();
            self.0.inference(&source)?
        };
        let bubbles: Vec<koharu_core::BubbleRegion> = result
            .regions
            .iter()
            .map(|r| {
                let [x1, y1, x2, y2] = r.bbox;
                koharu_core::BubbleRegion {
                    x: x1.max(0.0),
                    y: y1.max(0.0),
                    width: (x2 - x1).max(0.0),
                    height: (y2 - y1).max(0.0),
                    confidence: r.score,
                }
            })
            .collect();
        Ok(Patch::apply(move |doc| doc.bubbles = bubbles))
    }
}

inventory::submit! {
    EngineInfo {
        id: "speech-bubble-segmentation",
        name: "Speech Bubble Segmentation",
        needs: &[],
        produces: &[Artifact::Bubbles],
        load: |res| Box::pin(async move {
            let m = koharu_ml::speech_bubble_segmentation::SpeechBubbleSegmentation::load(
                &res.runtime,
                matches!(res.device, koharu_ml::Device::Cpu),
            ).await?;
            Ok(Box::new(SpeechBubbleSegEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- Font detection -------------------------------------------------------

struct FontDetectEngine(koharu_ml::font_detector::FontDetector);

#[async_trait]
impl Engine for FontDetectEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        if doc.text_blocks.is_empty() {
            return Ok(Patch::none());
        }
        let (source, crops) = {
            let _s = tracing::info_span!("load_image").entered();
            let source: SerializableDynamicImage = res.storage.images.load(&doc.source)?.into();
            let crops: Vec<DynamicImage> = doc
                .text_blocks
                .iter()
                .map(|b| source.crop_imm(b.x as u32, b.y as u32, b.width as u32, b.height as u32))
                .collect();
            (source, crops)
        };
        let mut preds = {
            let _s = tracing::info_span!("inference", blocks = crops.len()).entered();
            self.0.inference(&crops, 1)?
        };
        for p in &mut preds {
            normalize_font_prediction(p);
        }
        let mut blocks = doc.text_blocks.clone();
        for (block, pred) in blocks.iter_mut().zip(preds) {
            block.font_prediction = Some(pred);
            block.style = None;
        }
        let _ = source; // keep alive
        Ok(Patch::apply(|doc| doc.text_blocks = blocks))
    }
}

inventory::submit! {
    EngineInfo {
        id: "yuzumarker-font-detection",
        name: "YuzuMarker Font Detection",
        needs: &[Artifact::TextBlocks],
        produces: &[Artifact::FontPredictions],
        load: |res| Box::pin(async move {
            let m = koharu_ml::font_detector::FontDetector::load(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu)).await?;
            Ok(Box::new(FontDetectEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- PaddleOCR-VL ---------------------------------------------------------

struct PaddleOcrEngine(std::sync::Mutex<koharu_llm::paddleocr_vl::PaddleOcrVl>);

#[async_trait]
impl Engine for PaddleOcrEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        if doc.text_blocks.is_empty() {
            return Ok(Patch::none());
        }
        let (source, regions) = {
            let _s = tracing::info_span!("load_image").entered();
            let source: SerializableDynamicImage = res.storage.images.load(&doc.source)?.into();
            let regions: Vec<_> = doc
                .text_blocks
                .iter()
                .map(|b| koharu_ml::comic_text_detector::crop_text_block_bbox(&source, b))
                .collect();
            (source, regions)
        };
        let outputs = {
            let _s = tracing::info_span!("inference", blocks = regions.len()).entered();
            let mut ocr = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("OCR mutex poisoned"))?;
            ocr.inference_images(
                &regions,
                koharu_llm::paddleocr_vl::PaddleOcrVlTask::Ocr,
                128,
            )?
        };
        let mut blocks = doc.text_blocks.clone();
        for (block, out) in blocks.iter_mut().zip(outputs) {
            block.text = Some(out.text);
        }
        let _ = source;
        Ok(Patch::apply(|doc| doc.text_blocks = blocks))
    }
}

inventory::submit! {
    EngineInfo {
        id: "paddle-ocr-vl-1.5",
        name: "PaddleOCR-VL",
        needs: &[Artifact::TextBlocks],
        produces: &[Artifact::OcrText],
        load: |res| Box::pin(async move {
            let backend = res.llm.backend();
            let m = koharu_llm::paddleocr_vl::PaddleOcrVl::load(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu), backend).await?;
            Ok(Box::new(PaddleOcrEngine(std::sync::Mutex::new(m))) as Box<dyn Engine>)
        }),
    }
}

// --- Manga OCR ---------------------------------------------------------------

struct MangaOcrEngine(koharu_ml::manga_ocr::MangaOcr);

#[async_trait]
impl Engine for MangaOcrEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        if doc.text_blocks.is_empty() {
            return Ok(Patch::none());
        }
        let crops = {
            let _s = tracing::info_span!("load_image").entered();
            let source: SerializableDynamicImage = res.storage.images.load(&doc.source)?.into();
            doc.text_blocks
                .iter()
                .map(|b| koharu_ml::comic_text_detector::crop_text_block_bbox(&source, b))
                .collect::<Vec<_>>()
        };
        let texts = {
            let _s = tracing::info_span!("inference", blocks = crops.len()).entered();
            self.0.inference(&crops)?
        };
        let mut blocks = doc.text_blocks.clone();
        for (block, text) in blocks.iter_mut().zip(texts) {
            block.text = Some(text);
        }
        Ok(Patch::apply(|doc| doc.text_blocks = blocks))
    }
}

inventory::submit! {
    EngineInfo {
        id: "manga-ocr",
        name: "Manga OCR",
        needs: &[Artifact::TextBlocks],
        produces: &[Artifact::OcrText],
        load: |res| Box::pin(async move {
            let m = koharu_ml::manga_ocr::MangaOcr::load(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu)).await?;
            Ok(Box::new(MangaOcrEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- Mit48px OCR -------------------------------------------------------------

struct Mit48pxOcrEngine(koharu_ml::mit48px_ocr::Mit48pxOcr);

#[async_trait]
impl Engine for Mit48pxOcrEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        if doc.text_blocks.is_empty() {
            return Ok(Patch::none());
        }
        let source: SerializableDynamicImage = {
            let _s = tracing::info_span!("load_image").entered();
            res.storage.images.load(&doc.source)?.into()
        };
        let preds = {
            let _s = tracing::info_span!("inference", blocks = doc.text_blocks.len()).entered();
            self.0.inference_text_blocks(&source, &doc.text_blocks)?
        };
        let mut blocks = doc.text_blocks.clone();
        for (block, pred) in blocks.iter_mut().zip(preds) {
            block.text = Some(pred.text);
        }
        Ok(Patch::apply(|doc| doc.text_blocks = blocks))
    }
}

inventory::submit! {
    EngineInfo {
        id: "mit48px-ocr",
        name: "MIT 48px OCR",
        needs: &[Artifact::TextBlocks],
        produces: &[Artifact::OcrText],
        load: |res| Box::pin(async move {
            let m = koharu_ml::mit48px_ocr::Mit48pxOcr::load(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu)).await?;
            Ok(Box::new(Mit48pxOcrEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- LLM Translation ------------------------------------------------------

struct LlmTranslateEngine;

#[async_trait]
impl Engine for LlmTranslateEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let mut page = doc.clone();
        let block_count = page.text_blocks.len();
        async {
            res.llm
                .translate(
                    &mut page,
                    options.target_language.as_deref(),
                    options.system_prompt.as_deref(),
                )
                .await
        }
        .instrument(tracing::info_span!("inference", blocks = block_count))
        .await?;
        let blocks = page.text_blocks;
        Ok(Patch::apply(|doc| doc.text_blocks = blocks))
    }
}

inventory::submit! {
    EngineInfo {
        id: "llm",
        name: "LLM",
        needs: &[Artifact::OcrText],
        produces: &[Artifact::Translations],
        load: |_res| Box::pin(async move {
            Ok(Box::new(LlmTranslateEngine) as Box<dyn Engine>)
        }),
    }
}

// --- Lama Inpainting ------------------------------------------------------

struct LamaInpaintEngine(koharu_ml::lama::Lama);

#[async_trait]
impl Engine for LamaInpaintEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let seg_ref = doc
            .segment
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no segment mask"))?;
        let (source, segment) = {
            let _s = tracing::info_span!("load_image").entered();
            let source: SerializableDynamicImage = res.storage.images.load(&doc.source)?.into();
            let segment: SerializableDynamicImage = res.storage.images.load(seg_ref)?.into();
            (source, segment)
        };
        let result = {
            let _s = tracing::info_span!("inference").entered();
            self.0
                .inference_with_blocks(&source, &segment, Some(&doc.text_blocks))?
        };
        let blob = {
            let _s = tracing::info_span!("save").entered();
            res.storage.images.store_webp(&result)?
        };
        Ok(Patch::apply(|doc| doc.inpainted = Some(blob)))
    }
}

inventory::submit! {
    EngineInfo {
        id: "lama-manga",
        name: "Lama Manga",
        needs: &[Artifact::Segment],
        produces: &[Artifact::Inpainted],
        load: |res| Box::pin(async move {
            let m = koharu_ml::lama::Lama::load(&res.runtime, matches!(res.device, koharu_ml::Device::Cpu)).await?;
            Ok(Box::new(LamaInpaintEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- AOT Inpainting -------------------------------------------------------

struct AotInpaintEngine(koharu_ml::aot_inpainting::AotInpainting);

#[async_trait]
impl Engine for AotInpaintEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        _options: &PipelineRunOptions,
    ) -> Result<Patch> {
        let seg_ref = doc
            .segment
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no segment mask"))?;
        let (source, segment) = {
            let _s = tracing::info_span!("load_image").entered();
            let source = res.storage.images.load(&doc.source)?;
            let segment = res.storage.images.load(seg_ref)?;
            (source, segment)
        };
        let result = {
            let _s = tracing::info_span!("inference").entered();
            self.0.inference(&source, &segment)?
        };
        let blob = {
            let _s = tracing::info_span!("save").entered();
            res.storage.images.store_webp(&result)?
        };
        Ok(Patch::apply(|doc| doc.inpainted = Some(blob)))
    }
}

inventory::submit! {
    EngineInfo {
        id: "aot-inpainting",
        name: "AOT Inpainting",
        needs: &[Artifact::Segment],
        produces: &[Artifact::Inpainted],
        load: |res| Box::pin(async move {
            let m = koharu_ml::aot_inpainting::AotInpainting::load(
                &res.runtime,
                matches!(res.device, koharu_ml::Device::Cpu),
            ).await?;
            Ok(Box::new(AotInpaintEngine(m)) as Box<dyn Engine>)
        }),
    }
}

// --- Koharu Renderer ------------------------------------------------------

struct KoharuRenderEngine;

#[async_trait]
impl Engine for KoharuRenderEngine {
    async fn run(
        &self,
        doc: &Document,
        res: &AppResources,
        options: &PipelineRunOptions,
    ) -> Result<Patch> {
        render_document(
            res,
            &doc.id,
            None,
            options.shader_effect,
            options.shader_stroke.clone(),
            options.default_font.clone(),
        )
        .await?;
        Ok(Patch::none())
    }
}

inventory::submit! {
    EngineInfo {
        id: "koharu-renderer",
        name: "Koharu Renderer",
        needs: &[Artifact::Inpainted, Artifact::Translations, Artifact::FontPredictions],
        produces: &[Artifact::Rendered],
        load: |_res| Box::pin(async move {
            Ok(Box::new(KoharuRenderEngine) as Box<dyn Engine>)
        }),
    }
}

// ---------------------------------------------------------------------------
// render_document — public helper for API endpoints with custom params
// ---------------------------------------------------------------------------

use crate::renderer::{RenderTextOptions, Renderer};

/// Get or load the renderer instance.
pub async fn get_renderer(res: &AppResources) -> Result<Arc<Renderer>> {
    res.registry
        .model("renderer", || async { Renderer::new() })
        .await
}

#[tracing::instrument(level = "info", skip_all, fields(document_id))]
pub async fn render_document(
    res: &AppResources,
    document_id: &str,
    text_block_index: Option<usize>,
    shader_effect: Option<TextShaderEffect>,
    shader_stroke: Option<TextStrokeStyle>,
    fallback_font: Option<String>,
) -> Result<()> {
    let renderer = get_renderer(res).await?;
    let doc = res.storage.page(document_id).await?;
    let document_font = doc
        .style
        .as_ref()
        .and_then(|s| s.default_font.as_deref())
        .or(fallback_font.as_deref());
    let source = res.storage.images.load(&doc.source)?;
    let inpainted = doc
        .inpainted
        .as_ref()
        .map(|r| res.storage.images.load(r))
        .transpose()?;
    let brush = doc
        .brush_layer
        .as_ref()
        .map(|r| res.storage.images.load(r))
        .transpose()?;
    let mut blocks = doc.text_blocks.clone();
    let bubbles = doc.bubbles.clone();
    let rendered_ref = renderer.render_to_blob(
        &res.storage.images,
        &source,
        inpainted.as_ref(),
        brush.as_ref(),
        &mut blocks,
        RenderTextOptions {
            text_block_index,
            shader_effect: shader_effect.unwrap_or_default(),
            shader_stroke,
            document_font,
            bubbles: &bubbles,
            image_width: doc.width,
            image_height: doc.height,
        },
    )?;
    res.storage
        .update_page(document_id, |doc| {
            doc.text_blocks = blocks;
            if let Some(r) = rendered_ref {
                doc.rendered = Some(r);
            }
        })
        .await
}

// ---------------------------------------------------------------------------
// Detection helpers
// ---------------------------------------------------------------------------

use koharu_ml::pp_doclayout_v3::LayoutRegion;

const VERTICAL_ASPECT: f32 = 1.15;
const OVERLAP_THRESHOLD: f32 = 0.9;

fn build_text_blocks(regions: &[LayoutRegion]) -> Vec<TextBlock> {
    let mut blocks: Vec<TextBlock> = regions
        .iter()
        .filter(|r| {
            let l = r.label.to_ascii_lowercase();
            l == "content" || l.contains("text") || l.contains("title")
        })
        .filter_map(|r| {
            let x1 = r.bbox[0].min(r.bbox[2]).max(0.0);
            let y1 = r.bbox[1].min(r.bbox[3]).max(0.0);
            let w = (r.bbox[0].max(r.bbox[2]).max(x1 + 1.0) - x1).max(1.0);
            let h = (r.bbox[1].max(r.bbox[3]).max(y1 + 1.0) - y1).max(1.0);
            (w >= 6.0 && h >= 6.0 && w * h >= 48.0).then(|| TextBlock {
                x: x1,
                y: y1,
                width: w,
                height: h,
                confidence: r.score,
                source_direction: Some(if h >= w * VERTICAL_ASPECT {
                    TextDirection::Vertical
                } else {
                    TextDirection::Horizontal
                }),
                source_language: Some("unknown".to_string()),
                rotation_deg: Some(0.0),
                detected_font_size_px: Some(w.min(h).max(1.0)),
                detector: Some("pp-doclayout-v3".to_string()),
                ..Default::default()
            })
        })
        .collect();
    if blocks.len() >= 2 {
        let mut out = Vec::with_capacity(blocks.len());
        for b in std::mem::take(&mut blocks) {
            let a = (b.width * b.height).max(1.0);
            let dup = out.iter().any(|e: &TextBlock| {
                let ea = (e.width * e.height).max(1.0);
                let ov = overlap(
                    [b.x, b.y, b.x + b.width, b.y + b.height],
                    [e.x, e.y, e.x + e.width, e.y + e.height],
                );
                ov / a >= OVERLAP_THRESHOLD || ov / ea >= OVERLAP_THRESHOLD
            });
            if !dup {
                out.push(b);
            }
        }
        blocks = out;
    }
    blocks
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = a[2].min(b[2]) - a[0].max(b[0]);
    let h = a[3].min(b[3]) - a[1].max(b[1]);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

// ---------------------------------------------------------------------------
// Reading order — right-to-left columns, top-to-bottom within each column
// ---------------------------------------------------------------------------

/// Sort text blocks in manga reading order (right-to-left, top-to-bottom).
/// Uses a Recursive XY-Cut algorithm to divide the page into columns and rows based on whitespace gaps.
fn sort_manga_reading_order(blocks: &mut [TextBlock]) {
    #[derive(Debug, PartialEq, Clone, Copy)]
    enum Axis {
        X,
        Y,
    }

    if blocks.len() <= 1 {
        return;
    }

    // Determine dynamic gap thresholds by calculating the median dimensions
    // of all text blocks on the page.
    let mut widths: Vec<f32> = blocks.iter().map(|b| b.width).collect();
    let mut heights: Vec<f32> = blocks.iter().map(|b| b.height).collect();

    widths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_w = widths[widths.len() / 2].max(1.0);
    let median_h = heights[heights.len() / 2].max(1.0);

    // Manga horizontal gutters are often tighter than vertical ones.
    let min_gap_x = (median_w * 0.15).max(10.0);
    let min_gap_y = (median_h * 0.10).max(8.0);

    // Initiate recursive sorting directly on the mutable slice.
    xy_cut_recursive(blocks, min_gap_x, min_gap_y);

    fn xy_cut_recursive(blocks: &mut [TextBlock], min_gap_x: f32, min_gap_y: f32) {
        if blocks.len() <= 1 {
            return;
        }

        let cut_result = find_best_cut(blocks, min_gap_x, min_gap_y);

        let Some((best_axis, best_gap)) = cut_result else {
            // FALLBACK: Row-Aware sort for clusters that can't be cleanly cut.
            let row_height = min_gap_y * 4.0;

            blocks.sort_by(|a, b| {
                let row_a = (a.y / row_height).floor();
                let row_b = (b.y / row_height).floor();

                row_a
                    .partial_cmp(&row_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.x.partial_cmp(&a.x).unwrap_or(std::cmp::Ordering::Equal))
            });
            return;
        };

        let cut_coord = (best_gap.0 + best_gap.1) / 2.0;

        // In-place stable partition to avoid recursively allocating new vectors.
        // Rust's `bool` sorts `false` before `true`. By mapping items destined
        // for the primary partition (Right or Top) to `false`, we separate
        // the geometry cleanly into two contiguous segments.
        blocks.sort_by_key(|block| {
            if best_axis == Axis::X {
                (block.x + block.width * 0.5) < cut_coord
            } else {
                (block.y + block.height * 0.5) > cut_coord
            }
        });

        // Find where the split boundary lies
        let group1_len = blocks
            .iter()
            .filter(|block| {
                if best_axis == Axis::X {
                    (block.x + block.width * 0.5) >= cut_coord
                } else {
                    (block.y + block.height * 0.5) <= cut_coord
                }
            })
            .count();

        if group1_len == 0 || group1_len == blocks.len() {
            blocks.sort_by(|a, b| b.x.partial_cmp(&a.x).unwrap_or(std::cmp::Ordering::Equal));
            return;
        }

        // Subdivide the slice bounds along the partition line and recurse.
        let (left, right) = blocks.split_at_mut(group1_len);
        xy_cut_recursive(left, min_gap_x, min_gap_y);
        xy_cut_recursive(right, min_gap_x, min_gap_y);
    }

    fn find_best_cut(
        blocks: &[TextBlock],
        min_gap_x: f32,
        min_gap_y: f32,
    ) -> Option<(Axis, (f32, f32))> {
        let mut x_intervals: Vec<(f32, f32)> =
            blocks.iter().map(|b| (b.x, b.x + b.width)).collect();
        let mut y_intervals: Vec<(f32, f32)> =
            blocks.iter().map(|b| (b.y, b.y + b.height)).collect();

        x_intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        y_intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let gap_x = find_largest_gap(&x_intervals, min_gap_x);
        let gap_y = find_largest_gap(&y_intervals, min_gap_y);

        match (gap_x, gap_y) {
            (Some(gx), Some(gy)) => {
                let width_y = gy.1 - gy.0;
                let width_x = gx.1 - gx.0;

                // Evaluate projection dominance:
                // Select the horizontal cut unless the vertical visual gap is overwhelmingly wider.
                if width_y > 12.0 || width_y > (width_x * 0.4) {
                    Some((Axis::Y, gy))
                } else {
                    Some((Axis::X, gx))
                }
            }
            (None, Some(gy)) => Some((Axis::Y, gy)),
            (Some(gx), None) => Some((Axis::X, gx)),
            (None, None) => None,
        }
    }

    fn find_largest_gap(intervals: &[(f32, f32)], min_gap: f32) -> Option<(f32, f32)> {
        if intervals.is_empty() {
            return None;
        }

        let mut largest_gap: Option<(f32, f32)> = None;
        let mut current_max_end = intervals[0].1;

        for interval in intervals.iter().skip(1) {
            // A valid separation exists if the start of the current interval
            // does not intersect the bounding maximum of all preceding intervals.
            if interval.0 > current_max_end {
                let gap_size = interval.0 - current_max_end;
                if gap_size >= min_gap {
                    if let Some(ref mut best_gap) = largest_gap {
                        if gap_size > (best_gap.1 - best_gap.0) {
                            *best_gap = (current_max_end, interval.0);
                        }
                    } else {
                        largest_gap = Some((current_max_end, interval.0));
                    }
                }
            }
            current_max_end = current_max_end.max(interval.1);
        }
        largest_gap
    }
}

// ---------------------------------------------------------------------------
// Font normalization
// ---------------------------------------------------------------------------

fn normalize_font_prediction(p: &mut FontPrediction) {
    p.text_color = clamp_white(clamp_black(p.text_color));
    p.stroke_color = clamp_white(clamp_black(p.stroke_color));
    if p.stroke_width_px > 0.0 && colors_similar(p.text_color, p.stroke_color) {
        p.stroke_width_px = 0.0;
        p.stroke_color = p.text_color;
    }
}

fn clamp_black(c: [u8; 3]) -> [u8; 3] {
    let t = if gray(c) { 60 } else { 12 };
    if c[0] <= t && c[1] <= t && c[2] <= t {
        [0, 0, 0]
    } else {
        c
    }
}

fn clamp_white(c: [u8; 3]) -> [u8; 3] {
    let t = 255 - if gray(c) { 60 } else { 12 };
    if c[0] >= t && c[1] >= t && c[2] >= t {
        [255, 255, 255]
    } else {
        c
    }
}

fn gray(c: [u8; 3]) -> bool {
    c.iter().max().unwrap().abs_diff(*c.iter().min().unwrap()) <= 10
}
fn colors_similar(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= 16)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn region(label: &str, bbox: [f32; 4]) -> LayoutRegion {
        LayoutRegion {
            order: 0,
            label_id: 0,
            label: label.to_string(),
            score: 0.9,
            bbox,
            polygon_points: vec![],
        }
    }

    #[test]
    fn detect_keeps_text_and_dedupes() {
        let blocks = build_text_blocks(&[
            region("text", [10.0, 10.0, 40.0, 40.0]),
            region("image", [0.0, 0.0, 128.0, 128.0]),
            region("aside_text", [12.0, 12.0, 39.0, 39.0]),
            region("doc_title", [60.0, 8.0, 90.0, 24.0]),
        ]);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn tall_region_is_vertical() {
        let blocks = build_text_blocks(&[region("text", [5.0, 5.0, 20.0, 60.0])]);
        assert_eq!(blocks[0].source_direction, Some(TextDirection::Vertical));
    }

    #[test]
    fn dag_auto_wiring() {
        let catalog = Registry::catalog();
        // Verify we have engines registered
        assert!(catalog.len() >= 7);
        // Find default pipeline engines
        let infos: Vec<&EngineInfo> = DEFAULT_PIPELINE
            .iter()
            .filter_map(|id| catalog.iter().find(|e| e.id == *id).copied())
            .collect();
        assert_eq!(infos.len(), DEFAULT_PIPELINE.len());
        let order = build_order(&infos).unwrap();
        let ids: Vec<&str> = order.iter().map(|&i| infos[i].id).collect();
        let pos = |id: &str| ids.iter().position(|&s| s == id).unwrap();
        assert!(pos("comic-text-bubble-detector") < pos("comic-text-detector-seg"));
        assert!(pos("comic-text-bubble-detector") < pos("yuzumarker-font-detection"));
        assert!(pos("comic-text-detector-seg") < pos("aot-inpainting"));
        assert!(pos("aot-inpainting") < pos("koharu-renderer"));
    }

    #[test]
    fn translation_readiness_requires_non_empty_translations_for_non_empty_source_text() {
        let empty_doc = Document::default();
        assert!(Artifact::OcrText.ready(&empty_doc));
        assert!(Artifact::Translations.ready(&empty_doc));

        let mut doc = Document::default();
        doc.text_blocks = vec![TextBlock {
            text: Some("hello".to_string()),
            translation: Some(String::new()),
            ..Default::default()
        }];

        assert!(Artifact::OcrText.ready(&doc));
        assert!(!Artifact::Translations.ready(&doc));

        doc.text_blocks[0].translation = Some("hi".to_string());
        assert!(Artifact::Translations.ready(&doc));
    }

    #[test]
    fn verify_step_outputs_reports_missing_artifacts() {
        let info = EngineInfo {
            id: "llm",
            name: "LLM",
            needs: &[Artifact::OcrText],
            produces: &[Artifact::Translations],
            load: |_res| Box::pin(async move { unreachable!("not used in test") }),
        };
        let doc = Document {
            text_blocks: vec![TextBlock {
                text: Some("hello".to_string()),
                translation: Some(String::new()),
                ..Default::default()
            }],
            ..Default::default()
        };

        let err = verify_step_outputs(&info, &doc).expect_err("missing translations");
        assert!(
            err.to_string()
                .contains("did not produce required artifacts")
        );
    }

    #[test]
    fn pipeline_run_options_copy_request_overrides() {
        let req = koharu_core::commands::ProcessRequest {
            document_id: Some("page-1".to_string()),
            llm: None,
            language: Some("es-ES".to_string()),
            system_prompt: Some("Translate tersely".to_string()),
            shader_effect: Some(TextShaderEffect {
                italic: true,
                bold: false,
            }),
            shader_stroke: Some(TextStrokeStyle {
                enabled: false,
                color: [0, 0, 0, 255],
                width_px: Some(3.0),
            }),
            default_font: None,
        };

        let options = PipelineRunOptions::from_process_request(&req);

        assert_eq!(options.target_language.as_deref(), Some("es-ES"));
        assert_eq!(options.system_prompt.as_deref(), Some("Translate tersely"));
        assert_eq!(
            options.shader_effect,
            Some(TextShaderEffect {
                italic: true,
                bold: false,
            })
        );
        assert_eq!(
            options
                .shader_stroke
                .as_ref()
                .and_then(|stroke| stroke.width_px),
            Some(3.0)
        );
    }
}
