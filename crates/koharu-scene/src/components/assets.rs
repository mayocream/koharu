use std::{collections::BTreeMap, sync::Arc};

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    BlobId, Error, Result,
    component::{Component, ValidationContext},
};

use super::Origin;

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AssetMetadata {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub attributes: BTreeMap<String, String>,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Asset {
    pub origin: Origin,
    pub blob: BlobId,
    pub media_type: String,
    pub metadata: AssetMetadata,
}

impl Asset {
    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        let dimensions = self.metadata.width.is_some() == self.metadata.height.is_some()
            && self.metadata.width.is_none_or(|value| value > 0)
            && self.metadata.height.is_none_or(|value| value > 0);
        if !dimensions
            || self.media_type.len() > 255
            || !self.media_type.contains('/')
            || self.media_type.contains('\0')
            || self.metadata.attributes.len() > 4096
            || self.metadata.attributes.iter().any(|(key, value)| {
                key.is_empty()
                    || key.len() > 255
                    || key.contains('\0')
                    || value.len() > 64 * 1024
                    || value.contains('\0')
            })
        {
            return Err(Error::invalid("asset metadata is invalid"));
        }
        if context.contains_blob(self.blob) {
            Ok(())
        } else {
            Err(Error::invalid("asset blob is missing"))
        }
    }
}

#[derive(Clone, Debug)]
pub struct AssetInput {
    pub bytes: Arc<[u8]>,
    pub media_type: String,
    pub metadata: AssetMetadata,
}

impl AssetInput {
    #[must_use]
    pub fn new(
        bytes: impl Into<Arc<[u8]>>,
        media_type: impl Into<String>,
        metadata: AssetMetadata,
    ) -> Self {
        Self {
            bytes: bytes.into(),
            media_type: media_type.into(),
            metadata,
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AssetRole(String);

impl AssetRole {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 127
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            Err(Error::invalid("asset role is invalid"))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Assets {
    pub(crate) values: BTreeMap<AssetRole, Asset>,
}

impl Component for Assets {
    const KIND: &'static str = "dev.koharu.assets";

    fn blob_refs(&self) -> Vec<BlobId> {
        self.values.values().map(|asset| asset.blob).collect()
    }

    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        if self.values.len() > 1024 {
            return Err(Error::invalid("entity has too many assets"));
        }
        for (role, asset) in &self.values {
            AssetRole::new(role.as_str())?;
            asset.validate(context)?;
        }
        Ok(())
    }
}
