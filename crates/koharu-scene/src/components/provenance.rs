use revision::{DeserializeRevisioned, Revisioned, SerializeRevisioned, revisioned};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{ProducerId, Result};

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Generation {
    pub producer: ProducerId,
    pub model: Option<String>,
    pub confidence: Option<f32>,
}

impl Generation {
    pub fn new(producer: ProducerId) -> Self {
        Self {
            producer,
            model: None,
            confidence: None,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        self.producer.validate()?;
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.len() > 4096 || model.contains('\0'))
            || self
                .confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            Err(crate::Error::invalid("generation metadata is invalid"))
        } else {
            Ok(())
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub enum Origin {
    User,
    Generated(Generation),
}

impl Origin {
    pub(crate) fn validate(&self) -> Result<()> {
        if let Self::Generated(generation) = self {
            generation.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Authored<T> {
    pub value: T,
    pub origin: Origin,
}

impl<T> Authored<T> {
    #[must_use]
    pub fn user(value: T) -> Self {
        Self {
            value,
            origin: Origin::User,
        }
    }

    #[must_use]
    pub fn generated(value: T, generation: Generation) -> Self {
        Self {
            value,
            origin: Origin::Generated(generation),
        }
    }
}

impl<T: Revisioned> Revisioned for Authored<T> {
    fn revision() -> u16 {
        1
    }
}

impl<T: SerializeRevisioned> SerializeRevisioned for Authored<T> {
    fn serialize_revisioned<W: std::io::Write>(
        &self,
        writer: &mut W,
    ) -> std::result::Result<(), revision::Error> {
        self.value.serialize_revisioned(writer)?;
        self.origin.serialize_revisioned(writer)
    }
}

impl<T: DeserializeRevisioned> DeserializeRevisioned for Authored<T> {
    fn deserialize_revisioned<R: std::io::Read>(
        reader: &mut R,
    ) -> std::result::Result<Self, revision::Error> {
        Ok(Self {
            value: T::deserialize_revisioned(reader)?,
            origin: Origin::deserialize_revisioned(reader)?,
        })
    }
}
