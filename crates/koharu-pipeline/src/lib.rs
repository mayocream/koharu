//! Scene-native model orchestration for Koharu.

mod builtin;
mod config;
mod context;
mod events;
mod execute;
mod node;
mod plan;
mod run;
mod worker;

use std::{collections::BTreeMap, fmt, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use koharu_config::Config;
use koharu_ml::Device;
use koharu_scene::{Commands, ElementId, Frame, PageId, Session};
use koharu_translator::TranslationConfig;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Mutex as AsyncMutex;

pub use builtin::{
    AotInpaintingConfig, BaberuOcrConfig, Flux2KleinConfig, FontDetectorConfig,
    KoharuLayoutRFDetrSeg2XLConfig, LaMaConfig, LaMaHDStrategy, MangaOcrConfig,
    PaddleOcrVl1_6Config, RoremMixedConfig,
};
pub use config::*;
pub use context::{BlobBytes, Context};
pub use events::*;
pub use run::{Run, RunError, RunReport, RunTarget};
pub use worker::serve_worker;

use node::{ConfiguredNode, ModelRuntime};
use plan::Plan;
use run::RunRequest;
use worker::WorkerFactory;

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Type,
    strum::Display,
    strum::EnumIter,
    strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum Phase {
    #[strum(to_string = "detection", serialize = "detect")]
    Detection,
    Ocr,
    #[strum(to_string = "translation", serialize = "translate")]
    Translation,
    #[strum(to_string = "typography", serialize = "type")]
    Typography,
    #[strum(to_string = "inpainting", serialize = "inpaint")]
    Inpainting,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(rename_all = "kebab-case")]
pub enum ProcessorId {
    #[serde(rename = "koharu-layout-rfdetr-seg-2xl")]
    KoharuLayoutRFDetrSeg2XL,
    #[serde(rename = "paddleocr-vl-1.6")]
    PaddleOcrVl1_6,
    MangaOcr,
    BaberuOcr,
    Translation,
    FontDetector,
    #[serde(rename = "lama")]
    LaMa,
    AotInpainting,
    Flux2Klein,
    RoremMixed,
}

impl fmt::Display for ProcessorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::KoharuLayoutRFDetrSeg2XL => "koharu-layout-rfdetr-seg-2xl",
            Self::PaddleOcrVl1_6 => "paddleocr-vl-1.6",
            Self::MangaOcr => "manga-ocr",
            Self::BaberuOcr => "baberu-ocr",
            Self::Translation => "translation",
            Self::FontDetector => "font-detector",
            Self::LaMa => "lama",
            Self::AotInpainting => "aot-inpainting",
            Self::Flux2Klein => "flux2-klein",
            Self::RoremMixed => "rorem-mixed",
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Project,
    Pages {
        pages: Vec<PageId>,
    },
    Region {
        page: PageId,
        frame: Frame,
    },
    Elements {
        elements: Vec<ElementId>,
    },
}

#[async_trait]
pub trait Processor: Send {
    async fn shutdown(&mut self) {}
    async fn run(&mut self, context: &Context) -> Result<Commands>;
}

#[async_trait]
trait ProcessorFactory: Send + Sync {
    async fn create(&self, node: &ConfiguredNode, device: Device) -> Result<Box<dyn Processor>>;
}

struct ProcessorEntry {
    node: ConfiguredNode,
    processor: Arc<AsyncMutex<Box<dyn Processor>>>,
}

pub struct Pipeline {
    config: Config<PipelineConfig>,
    translation: Config<TranslationConfig>,
    device: Device,
    factory: Arc<dyn ProcessorFactory>,
    processors: AsyncMutex<BTreeMap<ProcessorId, ProcessorEntry>>,
    accelerator: AsyncMutex<()>,
    run_lock: AsyncMutex<()>,
}

impl Pipeline {
    #[must_use]
    pub fn new(
        config: impl Into<Config<PipelineConfig>>,
        translation: impl Into<Config<TranslationConfig>>,
    ) -> Self {
        Self::with_factory(
            config.into(),
            translation.into(),
            Arc::new(WorkerFactory::default()),
        )
    }

    #[must_use]
    pub fn with_worker_executable(
        config: impl Into<Config<PipelineConfig>>,
        translation: impl Into<Config<TranslationConfig>>,
        executable: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self::with_factory(
            config.into(),
            translation.into(),
            Arc::new(WorkerFactory::with_executable(executable.into())),
        )
    }

    fn with_factory(
        config: Config<PipelineConfig>,
        translation: Config<TranslationConfig>,
        factory: Arc<dyn ProcessorFactory>,
    ) -> Self {
        Self {
            config,
            translation,
            device: koharu_ml::device(false),
            factory,
            processors: AsyncMutex::new(BTreeMap::new()),
            accelerator: AsyncMutex::new(()),
            run_lock: AsyncMutex::new(()),
        }
    }

    pub fn check(&self) -> Result<()> {
        self.build_plan().map(|_| ())
    }

    pub fn graph(&self) -> Result<String> {
        Ok(self.build_plan()?.dot())
    }

    fn build_plan(&self) -> Result<Plan> {
        let config = self.config.read()?.clone();
        let translation = self.translation.read()?.clone();
        Plan::build(&config, &translation.model)
    }

    #[must_use]
    pub fn run<'pipeline, 'session>(
        &'pipeline self,
        session: &'session mut Session,
    ) -> Run<'pipeline, 'session> {
        Run {
            pipeline: self,
            session,
            request: RunRequest::default(),
        }
    }

    pub async fn unload_all(&self) -> Result<()> {
        let _run = self.run_lock.lock().await;
        let removed = std::mem::take(&mut *self.processors.lock().await);
        Self::shutdown_loaded(removed.into_values()).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
