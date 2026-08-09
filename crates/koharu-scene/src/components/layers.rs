use std::collections::{BTreeMap, BTreeSet};

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    EntityId, Error, Result,
    component::{Component, ValidationContext},
    id::validate_namespaced,
};

use super::{AssetRole, Origin};

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
pub struct AssetRef {
    pub owner: EntityId,
    pub role: AssetRole,
}

impl AssetRef {
    #[must_use]
    pub const fn new(owner: EntityId, role: AssetRole) -> Self {
        Self { owner, role }
    }

    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        AssetRole::new(self.role.as_str())?;
        if context.contains_entity(self.owner) {
            Ok(())
        } else {
            Err(Error::invalid("pixel layer asset owner is missing"))
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MaskChannel {
    #[default]
    Luminance,
    Red,
    Green,
    Blue,
    Alpha,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PixelFormat {
    #[default]
    Color,
    Mask {
        channel: MaskChannel,
        tint: [u8; 4],
    },
}

/// Explicit visual intent for a stored color image or mask.
///
/// [`Geometry`](super::Geometry) owns non-page placement. The referenced asset
/// may live on this entity or another entity in the document.
#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct PixelLayer {
    pub origin: Origin,
    pub name: String,
    pub asset: AssetRef,
    pub format: PixelFormat,
}

impl PixelLayer {
    #[must_use]
    pub fn color(origin: Origin, name: impl Into<String>, asset: AssetRef) -> Self {
        Self {
            origin,
            name: name.into(),
            asset,
            format: PixelFormat::Color,
        }
    }

    #[must_use]
    pub fn mask(
        origin: Origin,
        name: impl Into<String>,
        asset: AssetRef,
        channel: MaskChannel,
        tint: [u8; 4],
    ) -> Self {
        Self {
            origin,
            name: name.into(),
            asset,
            format: PixelFormat::Mask { channel, tint },
        }
    }
}

impl Component for PixelLayer {
    const KIND: &'static str = "dev.koharu.layer.pixel";

    fn record_refs(&self) -> Vec<EntityId> {
        vec![self.asset.owner]
    }

    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        if self.name.is_empty() || self.name.len() > 4096 || self.name.contains('\0') {
            return Err(Error::invalid("pixel layer name is invalid"));
        }
        self.asset.validate(context)?;
        Ok(())
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
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TextLayoutKind {
    Point,
    #[default]
    Paragraph,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum VerticalAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TextLayout {
    pub origin: Origin,
    pub kind: TextLayoutKind,
    /// Insets in top, right, bottom, left order.
    pub insets: [f32; 4],
    pub vertical_alignment: VerticalAlignment,
}

impl TextLayout {
    #[must_use]
    pub fn new(kind: TextLayoutKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_origin(origin: Origin, kind: TextLayoutKind) -> Self {
        Self {
            origin,
            kind,
            ..Self::default()
        }
    }
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            origin: Origin::User,
            kind: TextLayoutKind::Paragraph,
            insets: [4.0; 4],
            vertical_alignment: VerticalAlignment::Center,
        }
    }
}

impl Component for TextLayout {
    const KIND: &'static str = "dev.koharu.layer.text";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        if self
            .insets
            .into_iter()
            .all(|inset| inset.is_finite() && inset >= 0.0)
        {
            Ok(())
        } else {
            Err(Error::invalid("text layout insets are invalid"))
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
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum TextAlignment {
    Start,
    #[default]
    Center,
    End,
    Justify,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum WritingMode {
    #[default]
    Horizontal,
    Vertical,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TextStroke {
    pub color: [u8; 4],
    pub width: f32,
}

impl Default for TextStroke {
    fn default() -> Self {
        Self {
            color: [u8::MAX; 4],
            width: 1.0,
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Typography {
    pub origin: Origin,
    /// Ordered authored fallback chain.
    pub font_families: Vec<String>,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub size: f32,
    pub minimum_size: f32,
    pub auto_fit: bool,
    pub color: [u8; 4],
    pub stroke: Option<TextStroke>,
    pub alignment: TextAlignment,
    pub writing_mode: WritingMode,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub extensions: BTreeMap<String, String>,
}

impl Typography {
    #[must_use]
    pub fn with_origin(origin: Origin) -> Self {
        Self {
            origin,
            ..Self::default()
        }
    }
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            origin: Origin::User,
            font_families: vec!["CCWildWords".to_owned(), "Arial".to_owned()],
            font_weight: 400,
            font_style: FontStyle::Normal,
            size: 24.0,
            minimum_size: 9.0,
            auto_fit: true,
            color: [0, 0, 0, u8::MAX],
            stroke: None,
            alignment: TextAlignment::Center,
            writing_mode: WritingMode::Horizontal,
            line_height: 1.2,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            extensions: BTreeMap::new(),
        }
    }
}

impl Component for Typography {
    const KIND: &'static str = "dev.koharu.text.typography";

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        let mut normalized_families = BTreeSet::new();
        let valid_families = !self.font_families.is_empty()
            && self.font_families.len() <= 64
            && self.font_families.iter().all(|family| {
                !family.trim().is_empty()
                    && family.len() <= 4096
                    && !family.contains('\0')
                    && normalized_families.insert(family.to_lowercase())
            });
        if !valid_families
            || !(1..=1000).contains(&self.font_weight)
            || !self.size.is_finite()
            || self.size <= 0.0
            || !self.minimum_size.is_finite()
            || self.minimum_size <= 0.0
            || self.minimum_size > self.size
            || self
                .stroke
                .is_some_and(|stroke| !stroke.width.is_finite() || stroke.width <= 0.0)
            || !self.line_height.is_finite()
            || self.line_height <= 0.0
            || !self.letter_spacing.is_finite()
            || !self.word_spacing.is_finite()
        {
            return Err(Error::invalid("typography intent is invalid"));
        }
        self.origin.validate()?;
        if self.extensions.len() > 1024
            || self.extensions.iter().any(|(key, value)| {
                validate_namespaced(key, "typography extension").is_err()
                    || value.len() > 64 * 1024
                    || value.contains('\0')
            })
        {
            return Err(Error::invalid("typography extensions are invalid"));
        }
        Ok(())
    }

    fn origin(&self) -> Option<&Origin> {
        Some(&self.origin)
    }

    fn set_origin(&mut self, origin: Origin) -> bool {
        self.origin = origin;
        true
    }
}
