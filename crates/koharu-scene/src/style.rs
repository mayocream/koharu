use serde::{Deserialize, Serialize};

pub type Color = [u8; 4];

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerticalAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum WritingMode {
    #[default]
    Horizontal,
    VerticalRightToLeft,
    VerticalLeftToRight,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TextOverflow {
    #[default]
    Visible,
    Clip,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum FontSlant {
    #[default]
    Normal,
    Italic,
    Oblique {
        angle_degrees: f32,
    },
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextDecoration {
    pub underline: bool,
    pub strikethrough: bool,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum StrokePosition {
    Inside,
    #[default]
    Center,
    Outside,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum BevelStyle {
    #[default]
    InnerBevel,
    OuterBevel,
    Emboss,
    PillowEmboss,
    StrokeEmboss,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum BevelTechnique {
    #[default]
    Smooth,
    ChiselHard,
    ChiselSoft,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
        unit_interval(self.opacity) && self.kind.is_valid()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TextEffectKind {
    Stroke {
        color: Color,
        width: f32,
        position: StrokePosition,
    },
    DropShadow {
        color: Color,
        angle_degrees: f32,
        distance: f32,
        spread: f32,
        size: f32,
    },
    InnerShadow {
        color: Color,
        angle_degrees: f32,
        distance: f32,
        choke: f32,
        size: f32,
    },
    OuterGlow {
        color: Color,
        spread: f32,
        size: f32,
    },
    InnerGlow {
        color: Color,
        choke: f32,
        size: f32,
    },
    BevelEmboss {
        style: BevelStyle,
        technique: BevelTechnique,
        depth: f32,
        size: f32,
        soften: f32,
        angle_degrees: f32,
        altitude_degrees: f32,
        highlight_color: Color,
        shadow_color: Color,
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
            Self::DropShadow {
                angle_degrees,
                distance,
                spread,
                size,
                ..
            }
            | Self::InnerShadow {
                angle_degrees,
                distance,
                choke: spread,
                size,
                ..
            } => {
                angle_degrees.is_finite()
                    && non_negative(*distance)
                    && unit_interval(*spread)
                    && non_negative(*size)
            }
            Self::OuterGlow { spread, size, .. }
            | Self::InnerGlow {
                choke: spread,
                size,
                ..
            } => unit_interval(*spread) && non_negative(*size),
            Self::BevelEmboss {
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
                    && stops.iter().all(|stop| unit_interval(stop.offset))
                    && stops
                        .windows(2)
                        .all(|pair| pair[0].offset <= pair[1].offset)
                    && angle_degrees.is_finite()
                    && scale.is_finite()
                    && *scale > 0.0
            }
        }
    }
}

/// Complete, uniform styling for one editable text node.
///
/// Font families are semantic preferences. Font files, resolution, shaping,
/// and rendering remain caller-owned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: f32,
    /// OpenType weight in the inclusive range `1..=1000`.
    pub font_weight: u16,
    /// Percentage of the normal face width. `100` is unchanged.
    pub font_stretch: f32,
    pub font_slant: FontSlant,
    pub color: Color,
    /// Ratio relative to `font_size`.
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    /// Percentage of the original glyph width. `100` is unchanged.
    pub horizontal_scale: f32,
    /// Percentage of the original glyph height. `100` is unchanged.
    pub vertical_scale: f32,
    pub baseline_shift: f32,
    /// Rotation applied to the laid-out text inside the node's transform.
    pub angle_degrees: f32,
    pub horizontal_align: TextAlign,
    pub vertical_align: VerticalAlign,
    pub writing_mode: WritingMode,
    pub decoration: TextDecoration,
    /// Ordered Photoshop-style appearance effects.
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
            horizontal_align: TextAlign::Start,
            vertical_align: VerticalAlign::Top,
            writing_mode: WritingMode::Horizontal,
            decoration: TextDecoration::default(),
            effects: Vec::new(),
        }
    }
}

impl TextStyle {
    pub(crate) fn is_valid(&self) -> bool {
        self.font_size.is_finite()
            && self.font_size > 0.0
            && (1..=1000).contains(&self.font_weight)
            && positive(self.font_stretch)
            && match self.font_slant {
                FontSlant::Oblique { angle_degrees } => angle_degrees.is_finite(),
                FontSlant::Normal | FontSlant::Italic => true,
            }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    /// Insets ordered as top, right, bottom, and left.
    pub inset: [f32; 4],
    pub overflow: TextOverflow,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            max_width: None,
            max_height: None,
            inset: [0.0; 4],
            overflow: TextOverflow::Visible,
        }
    }
}

impl TextLayout {
    pub(crate) fn is_valid(&self) -> bool {
        self.max_width.is_none_or(positive)
            && self.max_height.is_none_or(positive)
            && self.inset.into_iter().all(non_negative)
    }
}

fn positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn non_negative(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}

fn unit_interval(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photoshop_style_round_trips() {
        let style = TextStyle {
            font_families: vec!["Aptos".into(), "Noto Sans".into()],
            angle_degrees: 12.0,
            vertical_align: VerticalAlign::Center,
            effects: vec![TextEffect::new(TextEffectKind::DropShadow {
                color: [0, 0, 0, 180],
                angle_degrees: 120.0,
                distance: 8.0,
                spread: 0.15,
                size: 12.0,
            })],
            ..TextStyle::default()
        };

        let encoded = postcard::to_stdvec(&style).unwrap();
        let decoded: TextStyle = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(decoded, style);
        assert!(decoded.is_valid());
    }

    #[test]
    fn invalid_effect_parameters_are_rejected() {
        let style = TextStyle {
            effects: vec![TextEffect::new(TextEffectKind::OuterGlow {
                color: [255; 4],
                spread: 2.0,
                size: 4.0,
            })],
            ..TextStyle::default()
        };

        assert!(!style.is_valid());
    }
}
