use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub(crate) fn is_valid(self) -> bool {
        self.width != 0 && self.height != 0
    }

    pub(crate) fn pixels(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub angle_degrees: f32,
}

impl Frame {
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            angle_degrees: 0.0,
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.width > 0.0
            && self.height.is_finite()
            && self.height > 0.0
            && self.angle_degrees.is_finite()
    }
}

pub type Quad = [[f32; 2]; 4];
