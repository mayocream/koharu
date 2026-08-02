use std::str::FromStr;

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Error, Result,
    component::{Component, ValidationContext},
    id::validate_namespaced,
};

use super::{Origin, Point};

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum TextDirection {
    #[default]
    Auto,
    Horizontal,
    Vertical,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct OcrAnalysis {
    pub origin: Origin,
    pub direction: TextDirection,
    pub confidence: Option<f32>,
    pub line_boundaries: Vec<[Point; 4]>,
}

impl Component for OcrAnalysis {
    const KIND: &'static str = "dev.koharu.analysis.ocr";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        if self
            .confidence
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || self.line_boundaries.len() > 1_000_000
            || self
                .line_boundaries
                .iter()
                .flatten()
                .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            Err(Error::invalid("OCR analysis is invalid"))
        } else {
            Ok(())
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

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RegionKind(String);

impl RegionKind {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_namespaced(&value, "region kind")?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> Result<()> {
        validate_namespaced(&self.0, "region kind")
    }
}

impl FromStr for RegionKind {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Region {
    pub origin: Origin,
    pub kind: RegionKind,
    pub label: Option<String>,
}

impl Component for Region {
    const KIND: &'static str = "dev.koharu.analysis.region";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        self.kind.validate()?;
        if self
            .label
            .as_ref()
            .is_some_and(|label| label.len() > 4096 || label.contains('\0'))
        {
            Err(Error::invalid("region label is invalid"))
        } else {
            Ok(())
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

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DetectionLabel {
    pub kind: RegionKind,
    pub confidence: f32,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct DetectionAnalysis {
    pub origin: Origin,
    pub labels: Vec<DetectionLabel>,
}

impl Component for DetectionAnalysis {
    const KIND: &'static str = "dev.koharu.analysis.detection";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        for label in &self.labels {
            label.kind.validate()?;
        }
        if self
            .labels
            .iter()
            .all(|label| label.confidence.is_finite() && (0.0..=1.0).contains(&label.confidence))
        {
            Ok(())
        } else {
            Err(Error::invalid("detection confidence is invalid"))
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
