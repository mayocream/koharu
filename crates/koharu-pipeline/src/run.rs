use std::{
    error::Error as StdError,
    fmt,
    sync::{Arc, Mutex},
};

use koharu_scene::{ElementId, Frame, PageId, Revision, Session};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Artifact, CancellationToken, EventSink, ModelMeasurement, Phase, Pipeline, ProcessorId, Scope,
    context::ContextOptions,
};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum RunTarget {
    #[default]
    All,
    Phase {
        phase: Phase,
    },
    Processors {
        processors: Vec<ProcessorId>,
    },
    Artifacts {
        artifacts: Vec<Artifact>,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Force {
    None,
    #[default]
    Targets,
    All,
}

#[derive(Clone)]
pub(crate) struct RunRequest {
    pub(crate) scope: Scope,
    pub(crate) target: RunTarget,
    pub(crate) force: Force,
    pub(crate) context: ContextOptions,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            scope: Scope::Project,
            target: RunTarget::All,
            force: Force::Targets,
            context: ContextOptions {
                translation: Default::default(),
                cancellation: CancellationToken::default(),
                events: None,
                measurements: Arc::new(Mutex::new(Vec::new())),
            },
        }
    }
}

pub struct Run<'pipeline, 'session> {
    pub(crate) pipeline: &'pipeline Pipeline,
    pub(crate) session: &'session mut Session,
    pub(crate) request: RunRequest,
}

impl Run<'_, '_> {
    #[must_use]
    pub fn pages(mut self, pages: impl IntoIterator<Item = PageId>) -> Self {
        self.request.scope = Scope::Pages {
            pages: pages.into_iter().collect(),
        };
        self
    }

    #[must_use]
    pub fn region(mut self, page: PageId, frame: Frame) -> Self {
        self.request.scope = Scope::Region { page, frame };
        self
    }

    #[must_use]
    pub fn elements(mut self, elements: impl IntoIterator<Item = ElementId>) -> Self {
        self.request.scope = Scope::Elements {
            elements: elements.into_iter().collect(),
        };
        self
    }

    #[must_use]
    pub fn phase(mut self, phase: Phase) -> Self {
        self.request.target = RunTarget::Phase { phase };
        self.request.force = Force::Targets;
        self
    }

    #[must_use]
    pub fn processors(mut self, processors: impl IntoIterator<Item = ProcessorId>) -> Self {
        self.request.target = RunTarget::Processors {
            processors: processors.into_iter().collect(),
        };
        self.request.force = Force::Targets;
        self
    }

    #[must_use]
    pub fn artifacts(mut self, artifacts: impl IntoIterator<Item = Artifact>) -> Self {
        self.request.target = RunTarget::Artifacts {
            artifacts: artifacts.into_iter().collect(),
        };
        self.request.force = Force::Targets;
        self
    }

    #[must_use]
    pub fn target(mut self, target: RunTarget) -> Self {
        self.request.target = target;
        self
    }

    #[must_use]
    pub fn force(mut self, force: Force) -> Self {
        self.request.force = force;
        self
    }

    #[must_use]
    pub fn cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.request.context.cancellation = cancellation;
        self
    }

    #[must_use]
    pub fn scope(mut self, scope: Scope) -> Self {
        self.request.scope = scope;
        self
    }

    #[must_use]
    pub fn events(mut self, events: EventSink) -> Self {
        self.request.context.events = Some(events);
        self
    }

    pub async fn execute(self) -> std::result::Result<RunReport, RunError> {
        self.pipeline.execute(self.session, self.request).await
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunReport {
    pub revisions: Vec<Revision>,
    pub processors: usize,
    pub skipped: usize,
    pub measurements: Vec<ModelMeasurement>,
}

#[derive(Debug)]
pub struct RunError {
    pub(crate) source: anyhow::Error,
    pub committed_revisions: Vec<Revision>,
    pub measurements: Vec<ModelMeasurement>,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl StdError for RunError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.source()
    }
}
