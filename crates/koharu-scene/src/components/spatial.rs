use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Error, Result,
    component::{Component, ValidationContext},
};

use super::Origin;

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Geometry {
    pub origin: Origin,
    pub points: Vec<Point>,
}

impl Geometry {
    #[must_use]
    pub fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: Origin::User,
            points: vec![
                Point { x, y },
                Point { x: x + width, y },
                Point {
                    x: x + width,
                    y: y + height,
                },
                Point { x, y: y + height },
            ],
        }
    }
}

impl Component for Geometry {
    const KIND: &'static str = "dev.koharu.geometry";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        if (3..=1_000_000).contains(&self.points.len())
            && self
                .points
                .iter()
                .all(|point| point.x.is_finite() && point.y.is_finite())
        {
            Ok(())
        } else {
            Err(Error::invalid(
                "geometry must contain finite polygon points",
            ))
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
pub struct Visibility {
    pub origin: Origin,
    pub visible: bool,
    pub opacity: f32,
}

impl Component for Visibility {
    const KIND: &'static str = "dev.koharu.visibility";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        if self.opacity.is_finite() && (0.0..=1.0).contains(&self.opacity) {
            Ok(())
        } else {
            Err(Error::invalid(
                "opacity must be finite and between zero and one",
            ))
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
