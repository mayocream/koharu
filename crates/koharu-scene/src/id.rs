use std::{fmt, str::FromStr};

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::{Error, Result};

macro_rules! entity_id {
    ($name:ident) => {
        #[revisioned(revision = 1)]
        #[derive(
            Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

entity_id!(EntityId);
entity_id!(RelationId);

#[derive(
    Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type,
)]
#[serde(transparent)]
pub struct ProjectId(pub(crate) koharu_storage::DocumentId);

impl fmt::Display for ProjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for ProjectId {
    type Err = <koharu_storage::DocumentId as FromStr>::Err;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ProducerId(String);

impl ProducerId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_namespaced(&value, "producer")?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_namespaced(&self.0, "producer")
    }
}

impl FromStr for ProducerId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl fmt::Display for ProducerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) fn validate_namespaced(value: &str, what: &str) -> Result<()> {
    let valid = value.len() <= 255
        && value.contains('.')
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        });
    if valid {
        Ok(())
    } else {
        Err(Error::invalid(format!("invalid {what}: {value}")))
    }
}
