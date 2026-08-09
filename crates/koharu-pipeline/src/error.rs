use std::{error::Error as StdError, fmt};

use crate::Stage;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    InvalidInput,
    ModelLoad,
    Processing,
    Commit,
    InvalidOutput,
}

#[derive(Debug)]
pub struct PipelineError {
    pub stage: Option<Stage>,
    pub kind: ErrorKind,
    source: anyhow::Error,
}

impl PipelineError {
    pub(crate) fn new(
        kind: ErrorKind,
        stage: Option<Stage>,
        source: impl Into<anyhow::Error>,
    ) -> Self {
        Self {
            stage,
            kind,
            source: source.into(),
        }
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.stage {
            Some(stage) if formatter.alternate() => {
                write!(formatter, "pipeline {stage} failed: {:#}", self.source)
            }
            Some(stage) => write!(formatter, "pipeline {stage} failed: {}", self.source),
            None if formatter.alternate() => {
                write!(formatter, "pipeline failed: {:#}", self.source)
            }
            None => write!(formatter, "pipeline failed: {}", self.source),
        }
    }
}

impl StdError for PipelineError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}
