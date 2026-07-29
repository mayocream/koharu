use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{ProgressSink, Scope, Stage};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "operation", content = "stage", rename_all = "snake_case")]
pub enum Operation {
    #[default]
    Full,
    Through(Stage),
    Only(Stage),
}

impl Operation {
    pub(crate) fn stages(self) -> Vec<Stage> {
        match self {
            Self::Full => Stage::ALL.to_vec(),
            Self::Through(Stage::Detection) => vec![Stage::Detection],
            Self::Through(Stage::Ocr) => vec![Stage::Detection, Stage::Ocr],
            Self::Through(Stage::Translation) => {
                vec![Stage::Detection, Stage::Ocr, Stage::Translation]
            }
            Self::Through(Stage::Inpainting) => vec![Stage::Detection, Stage::Inpainting],
            Self::Only(stage) => vec![stage],
        }
    }
}

#[derive(Clone)]
pub struct Request {
    pub operation: Operation,
    pub scope: Scope,
    pub stop: StopToken,
    pub progress: Option<ProgressSink>,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            operation: Operation::Full,
            scope: Scope::Project,
            stop: StopToken::default(),
            progress: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct StopToken(Arc<AtomicBool>);

impl StopToken {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
