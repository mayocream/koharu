use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

pub type Color = [u8; 4];

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum VerticalAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum WritingMode {
    #[default]
    Auto,
    Horizontal,
    VerticalRightToLeft,
    VerticalLeftToRight,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum TextOverflow {
    #[default]
    Visible,
    Clip,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum TextFit {
    #[default]
    Frame,
    Bubble,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub enum FontSlant {
    #[default]
    Normal,
    Italic,
    Oblique {
        angle_degrees: f32,
    },
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum StrokePosition {
    Inside,
    #[default]
    Center,
    Outside,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum BevelStyle {
    #[default]
    Inner,
    Outer,
    Emboss,
    Pillow,
    Stroke,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct TextDecoration {
    pub underline: bool,
    pub strikethrough: bool,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TextEffect {
    pub enabled: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub kind: TextEffectKind,
}

impl TextEffect {
    #[must_use]
    pub fn new(kind: TextEffectKind) -> Self {
        Self {
            enabled: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            kind,
        }
    }

    fn is_valid(&self) -> bool {
        unit(self.opacity) && self.kind.is_valid()
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub enum TextEffectKind {
    Stroke {
        color: Color,
        width: f32,
        position: StrokePosition,
    },
    Shadow {
        inner: bool,
        color: Color,
        angle_degrees: f32,
        distance: f32,
        spread: f32,
        size: f32,
    },
    Glow {
        inner: bool,
        color: Color,
        spread: f32,
        size: f32,
    },
    Bevel {
        style: BevelStyle,
        depth: f32,
        size: f32,
        soften: f32,
        angle_degrees: f32,
        altitude_degrees: f32,
        highlight: Color,
        shadow: Color,
    },
    Satin {
        color: Color,
        angle_degrees: f32,
        distance: f32,
        size: f32,
        invert: bool,
    },
    ColorOverlay {
        color: Color,
    },
    GradientOverlay {
        stops: Vec<GradientStop>,
        angle_degrees: f32,
        scale: f32,
        reverse: bool,
    },
}

impl TextEffectKind {
    fn is_valid(&self) -> bool {
        match self {
            Self::Stroke { width, .. } => non_negative(*width),
            Self::Shadow {
                angle_degrees,
                distance,
                spread,
                size,
                ..
            } => {
                angle_degrees.is_finite()
                    && non_negative(*distance)
                    && unit(*spread)
                    && non_negative(*size)
            }
            Self::Glow { spread, size, .. } => unit(*spread) && non_negative(*size),
            Self::Bevel {
                depth,
                size,
                soften,
                angle_degrees,
                altitude_degrees,
                ..
            } => {
                non_negative(*depth)
                    && non_negative(*size)
                    && non_negative(*soften)
                    && angle_degrees.is_finite()
                    && altitude_degrees.is_finite()
            }
            Self::Satin {
                angle_degrees,
                distance,
                size,
                ..
            } => angle_degrees.is_finite() && non_negative(*distance) && non_negative(*size),
            Self::ColorOverlay { .. } => true,
            Self::GradientOverlay {
                stops,
                angle_degrees,
                scale,
                ..
            } => {
                stops.len() >= 2
                    && stops.iter().all(|stop| unit(stop.offset))
                    && stops
                        .windows(2)
                        .all(|pair| pair[0].offset <= pair[1].offset)
                    && angle_degrees.is_finite()
                    && positive(*scale)
            }
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: f32,
    /// OpenType weight in `1..=1000`.
    pub font_weight: u16,
    /// Percentage where `100` is the normal face width.
    pub font_stretch: f32,
    pub font_slant: FontSlant,
    pub color: Color,
    /// Ratio relative to `font_size`.
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub horizontal_scale: f32,
    pub vertical_scale: f32,
    pub baseline_shift: f32,
    /// Rotation inside the text frame, separate from element rotation.
    pub angle_degrees: f32,
    pub decoration: TextDecoration,
    pub effects: Vec<TextEffect>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_families: Vec::new(),
            font_size: 16.0,
            font_weight: 400,
            font_stretch: 100.0,
            font_slant: FontSlant::Normal,
            color: [0, 0, 0, 255],
            line_height: 1.2,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 100.0,
            vertical_scale: 100.0,
            baseline_shift: 0.0,
            angle_degrees: 0.0,
            decoration: TextDecoration::default(),
            effects: Vec::new(),
        }
    }
}

impl TextStyle {
    pub(crate) fn is_valid(&self) -> bool {
        positive(self.font_size)
            && (1..=1000).contains(&self.font_weight)
            && positive(self.font_stretch)
            && !matches!(self.font_slant, FontSlant::Oblique { angle_degrees } if !angle_degrees.is_finite())
            && positive(self.line_height)
            && self.letter_spacing.is_finite()
            && self.word_spacing.is_finite()
            && positive(self.horizontal_scale)
            && positive(self.vertical_scale)
            && self.baseline_shift.is_finite()
            && self.angle_degrees.is_finite()
            && self
                .font_families
                .iter()
                .all(|family| !family.trim().is_empty())
            && self.effects.iter().all(TextEffect::is_valid)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TextLayout {
    pub horizontal_align: TextAlign,
    pub vertical_align: VerticalAlign,
    pub writing_mode: WritingMode,
    /// Insets ordered top, right, bottom, left.
    pub inset: [f32; 4],
    pub overflow: TextOverflow,
    pub fit: TextFit,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            horizontal_align: TextAlign::Center,
            vertical_align: VerticalAlign::Center,
            writing_mode: WritingMode::Auto,
            inset: [0.0; 4],
            overflow: TextOverflow::Visible,
            fit: TextFit::Bubble,
        }
    }
}

impl TextLayout {
    pub(crate) fn is_valid(&self) -> bool {
        self.inset.into_iter().all(non_negative)
    }
}

fn positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}
fn non_negative(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}
fn unit(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
