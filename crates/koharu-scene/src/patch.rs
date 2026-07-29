use std::sync::Arc;

use crate::{ProjectId, Result, SceneSnapshot, index::SceneIndex};

#[derive(Clone, Debug)]
pub struct ScenePatch {
    pub(crate) storage: koharu_storage::Patch,
    pub(crate) result_index: Option<Arc<SceneIndex>>,
}

impl ScenePatch {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    #[must_use]
    pub fn with_label(mut self, label: impl Into<std::sync::Arc<str>>) -> Self {
        self.storage = self.storage.with_label(label);
        self
    }

    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        ProjectId(self.storage.base().document)
    }

    #[must_use]
    pub fn base_revision(&self) -> crate::Revision {
        self.storage.base().revision
    }

    #[must_use]
    pub fn fingerprint(&self) -> crate::PatchId {
        self.storage.fingerprint()
    }

    pub fn validate_on(&self, snapshot: &SceneSnapshot) -> Result<()> {
        snapshot.preview([self]).map(|_| ())
    }

    pub fn rebase_on(&self, snapshot: &SceneSnapshot) -> Result<Self> {
        Ok(Self {
            storage: self.storage.rebase_on(&snapshot.storage)?,
            result_index: None,
        })
    }
}
