use std::{fmt, str::FromStr};

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Error, Result};

macro_rules! record_id {
    ($name:ident) => {
        #[revisioned(revision = 1)]
        #[derive(
            Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type,
        )]
        #[serde(transparent)]
        pub struct $name(koharu_storage::RecordId);

        impl $name {
            pub(crate) const fn from_storage(id: koharu_storage::RecordId) -> Self {
                Self(id)
            }

            pub(crate) const fn storage(self) -> koharu_storage::RecordId {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

record_id!(EntityId);
record_id!(RelationId);

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

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentSlot(String);

impl ComponentSlot {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self(if value.is_empty() {
            "default".to_owned()
        } else {
            value
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        if self.0.is_empty() {
            "default"
        } else {
            &self.0
        }
    }

    pub(crate) fn storage(&self) -> Result<koharu_storage::ComponentSlot> {
        koharu_storage::ComponentSlot::new(self.as_str()).map_err(Into::into)
    }
}

impl From<&str> for ComponentSlot {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ComponentSlot {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for ComponentSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
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
