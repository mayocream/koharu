use koharu_canvas::PhysicalSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhysicalRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalRect {
    #[must_use]
    pub const fn size(self) -> PhysicalSize {
        PhysicalSize::new(self.width, self.height)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    #[must_use]
    pub fn clipped_to(self, size: PhysicalSize) -> Self {
        let x = self.x.min(size.width);
        let y = self.y.min(size.height);
        Self {
            x,
            y,
            width: self.width.min(size.width.saturating_sub(x)),
            height: self.height.min(size.height.saturating_sub(y)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMetrics {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

impl WindowMetrics {
    #[must_use]
    pub const fn physical_size(self) -> PhysicalSize {
        PhysicalSize::new(self.width, self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewportReport {
    pub window_generation: u64,
    pub bounds: LogicalRect,
    pub scale_factor: f64,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum LayoutError {
    #[error("desktop runtime could not apply viewport: {0}")]
    Runtime(String),
    #[error("window scale factor must be finite and positive")]
    InvalidWindowScale,
    #[error("viewport coordinates must be finite and non-negative")]
    InvalidViewport,
    #[error(
        "viewport belongs to stale window generation {reported}; current generation is {current}"
    )]
    StaleViewport { reported: u64, current: u64 },
    #[error("viewport scale factor does not match the current window")]
    ScaleMismatch,
    #[error("physical coordinate exceeds the supported range")]
    CoordinateOverflow,
}

#[derive(Clone, Debug)]
pub struct Layout {
    metrics: WindowMetrics,
    viewport: PhysicalRect,
}

impl Layout {
    pub fn new(width: u32, height: u32, scale_factor: f64) -> Result<Self, LayoutError> {
        if !scale_factor.is_finite() || scale_factor <= 0.0 {
            return Err(LayoutError::InvalidWindowScale);
        }
        Ok(Self {
            metrics: WindowMetrics {
                generation: 1,
                width,
                height,
                scale_factor,
            },
            viewport: PhysicalRect::default(),
        })
    }

    #[must_use]
    pub const fn metrics(&self) -> WindowMetrics {
        self.metrics
    }

    #[must_use]
    pub const fn viewport(&self) -> PhysicalRect {
        self.viewport
    }

    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<bool, LayoutError> {
        if !scale_factor.is_finite() || scale_factor <= 0.0 {
            return Err(LayoutError::InvalidWindowScale);
        }
        if self.metrics.width == width
            && self.metrics.height == height
            && self.metrics.scale_factor == scale_factor
        {
            return Ok(false);
        }
        self.metrics.generation = self.metrics.generation.wrapping_add(1).max(1);
        self.metrics.width = width;
        self.metrics.height = height;
        self.metrics.scale_factor = scale_factor;
        self.viewport = self.viewport.clipped_to(self.metrics.physical_size());
        Ok(true)
    }

    /// Applies browser-owned logical bounds against the current window
    /// generation. The generation never crosses the public desktop boundary.
    pub fn apply_current_viewport(
        &mut self,
        bounds: LogicalRect,
        scale_factor: f64,
    ) -> Result<PhysicalRect, LayoutError> {
        self.apply_viewport(ViewportReport {
            window_generation: self.metrics.generation,
            bounds,
            scale_factor,
        })
    }

    fn apply_viewport(&mut self, report: ViewportReport) -> Result<PhysicalRect, LayoutError> {
        if report.window_generation != self.metrics.generation {
            return Err(LayoutError::StaleViewport {
                reported: report.window_generation,
                current: self.metrics.generation,
            });
        }
        if !report.scale_factor.is_finite() || report.scale_factor <= 0.0 {
            return Err(LayoutError::InvalidWindowScale);
        }
        let tolerance = 1e-6 * self.metrics.scale_factor.abs().max(1.0);
        if (report.scale_factor - self.metrics.scale_factor).abs() > tolerance {
            return Err(LayoutError::ScaleMismatch);
        }
        let LogicalRect {
            x,
            y,
            width,
            height,
        } = report.bounds;
        if ![x, y, width, height].into_iter().all(f64::is_finite)
            || x < 0.0
            || y < 0.0
            || width < 0.0
            || height < 0.0
        {
            return Err(LayoutError::InvalidViewport);
        }
        let window_scale = self.metrics.scale_factor;
        let left = physical_value(x, window_scale)?;
        let top = physical_value(y, window_scale)?;
        let right = physical_value(x + width, window_scale)?;
        let bottom = physical_value(y + height, window_scale)?;
        self.viewport = PhysicalRect {
            x: left,
            y: top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        }
        .clipped_to(self.metrics.physical_size());
        Ok(self.viewport)
    }
}

fn physical_value(value: f64, scale_factor: f64) -> Result<u32, LayoutError> {
    let value = (value * scale_factor).round();
    if value > f64::from(u32::MAX) {
        return Err(LayoutError::CoordinateOverflow);
    }
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(generation: u64) -> ViewportReport {
        ViewportReport {
            window_generation: generation,
            bounds: LogicalRect {
                x: 10.25,
                y: 20.5,
                width: 300.25,
                height: 200.5,
            },
            scale_factor: 1.5,
        }
    }

    #[test]
    fn logical_edges_are_rounded_once() {
        let mut layout = Layout::new(800, 600, 1.5).unwrap();
        assert_eq!(
            layout.apply_viewport(report(1)).unwrap(),
            PhysicalRect {
                x: 15,
                y: 31,
                width: 451,
                height: 301,
            }
        );
    }

    #[test]
    fn stale_viewport_cannot_overwrite_resized_layout() {
        let mut layout = Layout::new(800, 600, 1.5).unwrap();
        layout.resize(900, 700, 1.5).unwrap();
        assert_eq!(
            layout.apply_viewport(report(1)),
            Err(LayoutError::StaleViewport {
                reported: 1,
                current: 2,
            })
        );
    }

    #[test]
    fn browser_bounds_bind_to_runtime_owned_generation() {
        let mut layout = Layout::new(800, 600, 1.5).unwrap();
        layout.resize(900, 700, 1.5).unwrap();
        assert!(layout.apply_current_viewport(report(1).bounds, 1.5).is_ok());
        assert_eq!(layout.metrics().generation, 2);
    }

    #[test]
    fn viewport_accepts_browser_dpr_rounding_but_not_a_different_window_scale() {
        let mut layout = Layout::new(800, 600, 4.0 / 3.0).unwrap();
        assert!(
            layout
                .apply_current_viewport(report(1).bounds, 1.333_333_3)
                .is_ok()
        );
        assert_eq!(
            layout.apply_current_viewport(report(1).bounds, 1.25),
            Err(LayoutError::ScaleMismatch)
        );
    }

    #[test]
    fn viewport_is_clipped_to_surface() {
        let mut layout = Layout::new(100, 80, 1.0).unwrap();
        let viewport = layout
            .apply_viewport(ViewportReport {
                window_generation: 1,
                bounds: LogicalRect {
                    x: 90.0,
                    y: 70.0,
                    width: 50.0,
                    height: 50.0,
                },
                scale_factor: 1.0,
            })
            .unwrap();
        assert_eq!(
            viewport,
            PhysicalRect {
                x: 90,
                y: 70,
                width: 10,
                height: 10,
            }
        );
    }
}
