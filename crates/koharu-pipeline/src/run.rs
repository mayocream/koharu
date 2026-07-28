use std::{collections::BTreeSet, error::Error as StdError, fmt, time::Duration};

use crate::{CancellationToken, EventSink, Pipeline, RunId, Scope, Stage, Target};

#[derive(Clone)]
pub(crate) struct RunRequest {
    pub scope: Scope,
    pub target: Target,
    pub cancellation: CancellationToken,
    pub events: Option<EventSink>,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            scope: Scope::Project,
            target: Target::All,
            cancellation: CancellationToken::default(),
            events: None,
        }
    }
}

pub struct Run<'pipeline> {
    pub(crate) pipeline: &'pipeline Pipeline,
    pub(crate) snapshot: koharu_scene::SceneSnapshot,
    pub(crate) request: RunRequest,
}

impl Run<'_> {
    #[must_use]
    pub fn pages(mut self, pages: impl IntoIterator<Item = koharu_scene::EntityId>) -> Self {
        self.request.scope = Scope::Pages(pages.into_iter().collect());
        self
    }

    #[must_use]
    pub fn region(mut self, page: koharu_scene::EntityId, bounds: crate::Bounds) -> Self {
        self.request.scope = Scope::Region { page, bounds };
        self
    }

    #[must_use]
    pub fn entities(mut self, entities: impl IntoIterator<Item = koharu_scene::EntityId>) -> Self {
        self.request.scope = Scope::Entities(entities.into_iter().collect());
        self
    }

    #[must_use]
    pub fn stage(mut self, stage: Stage) -> Self {
        self.request.target = Target::Stage(stage);
        self
    }

    #[must_use]
    pub fn stages(mut self, stages: impl IntoIterator<Item = Stage>) -> Self {
        self.request.target = Target::Stages(stages.into_iter().collect::<BTreeSet<_>>());
        self
    }

    #[must_use]
    pub fn target(mut self, target: Target) -> Self {
        self.request.target = target;
        self
    }

    #[must_use]
    pub fn cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.request.cancellation = cancellation;
        self
    }

    #[must_use]
    pub fn scope(mut self, scope: Scope) -> Self {
        self.request.scope = scope;
        self
    }

    #[must_use]
    pub fn events(mut self, events: EventSink) -> Self {
        self.request.events = Some(events);
        self
    }

    pub async fn execute(self) -> Result<RunReport, RunError> {
        self.pipeline.execute(self.snapshot, self.request).await
    }
}

#[derive(Clone, Debug, Default)]
pub struct NodeMeasurements {
    pub queue: Duration,
    pub load: Duration,
    pub execution: Duration,
}

#[derive(Clone, Debug)]
pub struct NodeReport {
    pub stage: Stage,
    pub model: String,
    pub elapsed: Duration,
    pub measurements: NodeMeasurements,
}

#[derive(Clone, Debug)]
pub struct RunReport {
    pub run: RunId,
    pub base: koharu_scene::Revision,
    pub patch: koharu_scene::ScenePatch,
    pub preview: koharu_scene::SceneSnapshot,
    pub nodes: Vec<NodeReport>,
    pub elapsed: Duration,
}

#[derive(Debug)]
pub struct RunError {
    pub run: RunId,
    pub stage: Option<Stage>,
    pub(crate) source: anyhow::Error,
}

impl RunError {
    pub(crate) fn new(run: RunId, stage: Option<Stage>, source: impl Into<anyhow::Error>) -> Self {
        Self {
            run,
            stage,
            source: source.into(),
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.source.downcast_ref::<Cancelled>().is_some()
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(stage) = self.stage {
            write!(
                formatter,
                "pipeline run {} failed in {stage}: {}",
                self.run.0, self.source
            )
        } else {
            write!(
                formatter,
                "pipeline run {} failed: {}",
                self.run.0, self.source
            )
        }
    }
}

impl StdError for RunError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

#[derive(Debug)]
pub(crate) struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("pipeline run was cancelled")
    }
}

impl StdError for Cancelled {}
