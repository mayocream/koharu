use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Error, Result,
    component::{Component, ValidationContext},
};

use super::Origin;

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Group {
    pub origin: Origin,
    pub name: String,
}

impl Component for Group {
    const KIND: &'static str = "dev.koharu.group";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        if self.name.len() <= 4096 && !self.name.contains('\0') {
            Ok(())
        } else {
            Err(Error::invalid("group name is invalid"))
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

/// Identifies the page-owned group whose child order is the canonical text order.
#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct TextGroup {}

impl Component for TextGroup {
    const KIND: &'static str = "dev.koharu.text.group";
}
