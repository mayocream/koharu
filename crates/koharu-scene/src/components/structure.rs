use std::str::FromStr;

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    EntityId, Error, Result,
    component::{Component, ValidationContext},
    id::validate_namespaced,
};

use super::{LanguageTag, Origin};

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct Project {
    pub source_locale: Option<LanguageTag>,
    pub target_locales: Vec<LanguageTag>,
}

impl Component for Project {
    const KIND: &'static str = "dev.koharu.project";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        if let Some(locale) = &self.source_locale {
            locale.validate()?;
        }
        for locale in &self.target_locales {
            locale.validate()?;
        }
        let mut targets = self.target_locales.clone();
        targets.sort();
        targets.dedup();
        if targets.len() == self.target_locales.len() {
            Ok(())
        } else {
            Err(Error::invalid("target locales contain duplicates"))
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Page {
    pub label: String,
    pub width: f64,
    pub height: f64,
}

impl Component for Page {
    const KIND: &'static str = "dev.koharu.page";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        if self.label.len() <= 4096
            && !self.label.contains('\0')
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
        {
            Ok(())
        } else {
            Err(Error::invalid("page label or dimensions are invalid"))
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageDraft {
    pub label: String,
    pub width: f64,
    pub height: f64,
}

impl PageDraft {
    #[must_use]
    pub fn new(label: impl Into<String>, width: f64, height: f64) -> Self {
        Self {
            label: label.into(),
            width,
            height,
        }
    }
}

impl From<PageDraft> for Page {
    fn from(value: PageDraft) -> Self {
        Self {
            label: value.label,
            width: value.width,
            height: value.height,
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct EntityOrigin {
    pub origin: Origin,
}

impl Component for EntityOrigin {
    const KIND: &'static str = "dev.koharu.entity.origin";

    fn origin(&self) -> Option<&Origin> {
        Some(&self.origin)
    }

    fn set_origin(&mut self, origin: Origin) -> bool {
        self.origin = origin;
        true
    }

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RelationKind(String);

impl RelationKind {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_namespaced(&value, "relation kind")?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> Result<()> {
        validate_namespaced(&self.0, "relation kind")
    }
}

impl FromStr for RelationKind {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Relation {
    pub origin: Origin,
    pub kind: RelationKind,
    pub source: EntityId,
    pub target: EntityId,
}

impl Component for Relation {
    const KIND: &'static str = "dev.koharu.relation";

    fn record_refs(&self) -> Vec<EntityId> {
        vec![self.source, self.target]
    }

    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        self.kind.validate()?;
        if context.contains_entity(self.source) && context.contains_entity(self.target) {
            Ok(())
        } else {
            Err(Error::invalid("relation endpoint is missing"))
        }
    }

    fn origin(&self) -> Option<&Origin> {
        Some(&self.origin)
    }

    fn set_origin(&mut self, origin: Origin) -> bool {
        self.origin = origin;
        true
    }
}
