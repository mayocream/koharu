use anyhow::{Context as _, Result};
use async_trait::async_trait;
use koharu_pipeline::{Committer, Pipeline, Report, Request, ResourceSnapshot};
use tokio::sync::{OnceCell, watch};

#[async_trait]
pub trait ProcessingRuntime: Send + Sync {
    /// Initialize model ownership and return the application resource stream.
    async fn initialize(&self) -> Result<Option<watch::Receiver<ResourceSnapshot>>>;

    async fn execute(
        &self,
        snapshot: koharu_scene::Snapshot,
        request: Request,
        committer: &mut dyn Committer,
    ) -> Result<Report>;
}

pub struct KoharuProcessingRuntime {
    pipeline: OnceCell<Pipeline>,
}

impl KoharuProcessingRuntime {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pipeline: OnceCell::const_new(),
        }
    }
}

impl Default for KoharuProcessingRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessingRuntime for KoharuProcessingRuntime {
    async fn initialize(&self) -> Result<Option<watch::Receiver<ResourceSnapshot>>> {
        let pipeline = self
            .pipeline
            .get_or_try_init(|| async {
                koharu_ml::init()
                    .await
                    .context("failed to initialize the ML runtime")?;
                Pipeline::load(koharu_ml::device(false))
            })
            .await?;
        Ok(Some(pipeline.subscribe_resources()))
    }

    async fn execute(
        &self,
        snapshot: koharu_scene::Snapshot,
        request: Request,
        committer: &mut dyn Committer,
    ) -> Result<Report> {
        let pipeline = self
            .pipeline
            .get()
            .context("processing runtime has not initialized")?;
        pipeline
            .execute(snapshot, request, committer)
            .await
            .map_err(Into::into)
    }
}
