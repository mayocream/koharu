//! Common types and constants used throughout the renderer.

use swash::text::Script;

use crate::font::Font;

/// Color represented as RGBA bytes.
pub type Color = [u8; 4];

/// 2D coordinates as (x, y) tuple.
pub type Point = (f32, f32);

/// Shared text styling parameters for layout and rendering.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle<'a> {
    pub font: &'a Font,
    pub font_size: f32,
    pub line_height: f32,
    pub color: Color,
    pub script: Option<Script>,
}
