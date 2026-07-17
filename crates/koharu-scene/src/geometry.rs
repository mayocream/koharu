use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CanvasSize {
    pub width: u32,
    pub height: u32,
}

impl CanvasSize {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

impl PixelSize {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    #[must_use]
    pub const fn pixels(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub xx: f64,
    pub yx: f64,
    pub xy: f64,
    pub yy: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        xx: 1.0,
        yx: 0.0,
        xy: 0.0,
        yy: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    #[must_use]
    pub const fn translation(x: f64, y: f64) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::IDENTITY
        }
    }

    #[must_use]
    pub fn then(self, child: Self) -> Self {
        Self {
            xx: self.xx * child.xx + self.xy * child.yx,
            xy: self.xx * child.xy + self.xy * child.yy,
            tx: self.xx * child.tx + self.xy * child.ty + self.tx,
            yx: self.yx * child.xx + self.yy * child.yx,
            yy: self.yx * child.xy + self.yy * child.yy,
            ty: self.yx * child.tx + self.yy * child.ty + self.ty,
        }
    }

    pub(crate) fn is_finite(self) -> bool {
        [self.xx, self.yx, self.xy, self.yy, self.tx, self.ty]
            .into_iter()
            .all(f64::is_finite)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}
