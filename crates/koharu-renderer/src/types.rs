//! Self-contained primitives for the rendering API.
//!
//! The app layer (`koharu-app`) translates scene `TextStyle` / `TextShaderEffect`
//! values into these before calling the renderer.

/// Horizontal alignment within a text layout box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Simple shader effect flags applied to glyph rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextShaderEffect {
    pub italic: bool,
    pub bold: bool,
}

impl TextShaderEffect {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_empty(self) -> bool {
        !self.italic && !self.bold
    }
}

/// Reading axis hint for a block of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    Horizontal,
    Vertical,
}

/// A single text block staged for rendering. Callers (i.e. `koharu-app`) translate
/// scene `TextData` nodes into these and hand a slice to the renderer.
///
/// `text` is the string to render (typically the translation). Empty-text blocks
/// should be filtered out by the caller; the renderer assumes `text` is non-empty.
///
/// `source_direction` is the OCR/detector's recorded reading axis for the
/// original source text. The writing-mode decision prefers this over bbox
/// aspect ratio for CJK content, so a wide-manga bubble with vertical
/// Japanese doesn't get flipped to horizontal just because of its shape.
#[derive(Debug, Clone, Default)]
pub struct RenderBlock {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: String,
    pub source_direction: Option<TextDirection>,
}
