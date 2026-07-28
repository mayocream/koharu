use std::{collections::BTreeMap, fmt, str::FromStr, sync::Arc};

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    BlobId, EncodedSceneComponent, EntityId, Error, ProducerId, Result, SceneComponent,
    ValidationContext,
    component::{revision_decode, revision_encode},
    id::validate_namespaced,
};

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct Children {
    ordered: Vec<EntityId>,
}

impl Children {
    #[must_use]
    pub fn new(ordered: impl IntoIterator<Item = EntityId>) -> Self {
        Self {
            ordered: ordered.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[EntityId] {
        &self.ordered
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = EntityId> + DoubleEndedIterator + '_ {
        self.ordered.iter().copied()
    }

    pub(crate) fn as_mut_vec(&mut self) -> &mut Vec<EntityId> {
        &mut self.ordered
    }
}

impl SceneComponent for Children {
    const KIND: &'static str = "dev.koharu.scene.children";
    const CURRENT_SCHEMA: u32 = 1;

    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, Self::CURRENT_SCHEMA)
    }

    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, Self::CURRENT_SCHEMA, payload)
    }

    fn record_refs(&self) -> Vec<EntityId> {
        self.ordered.clone()
    }

    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        let mut unique = self.ordered.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != self.ordered.len() {
            return Err(Error::invalid("children contain duplicate entities"));
        }
        if self.ordered.iter().all(|id| context.contains_entity(*id)) {
            Ok(())
        } else {
            Err(Error::invalid("children reference a missing entity"))
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct ProjectSettings {
    pub source_locale: Option<LanguageTag>,
    pub target_locales: Vec<LanguageTag>,
}

impl SceneComponent for ProjectSettings {
    const KIND: &'static str = "dev.koharu.scene.project-settings";
    const CURRENT_SCHEMA: u32 = 1;

    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, Self::CURRENT_SCHEMA)
    }

    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, Self::CURRENT_SCHEMA, payload)
    }

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        if let Some(locale) = &self.source_locale {
            locale.validate()?;
        }
        for locale in &self.target_locales {
            locale.validate()?;
        }
        let mut targets = self.target_locales.clone();
        targets.sort();
        targets.dedup();
        if targets.len() == self.target_locales.len() {
            Ok(())
        } else {
            Err(Error::invalid("target locales contain duplicates"))
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Page {
    pub label: String,
    pub width: f64,
    pub height: f64,
}

impl SceneComponent for Page {
    const KIND: &'static str = "dev.koharu.scene.page";
    const CURRENT_SCHEMA: u32 = 1;

    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, Self::CURRENT_SCHEMA)
    }

    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, Self::CURRENT_SCHEMA, payload)
    }

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        if self.label.len() <= 4096
            && !self.label.contains('\0')
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
        {
            Ok(())
        } else {
            Err(Error::invalid("page label or dimensions are invalid"))
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageDraft {
    pub label: String,
    pub width: f64,
    pub height: f64,
}

impl PageDraft {
    #[must_use]
    pub fn new(label: impl Into<String>, width: f64, height: f64) -> Self {
        Self {
            label: label.into(),
            width,
            height,
        }
    }
}

impl From<PageDraft> for Page {
    fn from(value: PageDraft) -> Self {
        Self {
            label: value.label,
            width: value.width,
            height: value.height,
        }
    }
}

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

impl SceneComponent for Geometry {
    const KIND: &'static str = "dev.koharu.scene.geometry";
    const CURRENT_SCHEMA: u32 = 1;

    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, Self::CURRENT_SCHEMA)
    }

    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, Self::CURRENT_SCHEMA, payload)
    }

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

impl SceneComponent for Visibility {
    const KIND: &'static str = "dev.koharu.scene.visibility";
    const CURRENT_SCHEMA: u32 = 1;

    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, Self::CURRENT_SCHEMA)
    }

    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, Self::CURRENT_SCHEMA, payload)
    }

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

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct LanguageTag(String);

impl LanguageTag {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = (2..=63).contains(&value.len())
            && !value.starts_with('-')
            && !value.ends_with('-')
            && value.split('-').all(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            });
        if valid {
            Ok(Self(value))
        } else {
            Err(Error::invalid(format!("invalid language tag: {value}")))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validate(&self) -> Result<()> {
        Self::new(self.0.clone()).map(|_| ())
    }
}

impl FromStr for LanguageTag {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Generation {
    pub producer: ProducerId,
    pub model: Option<String>,
    pub confidence: Option<f32>,
}

impl Generation {
    pub fn new(producer: ProducerId) -> Self {
        Self {
            producer,
            model: None,
            confidence: None,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        self.producer.validate()?;
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.len() > 4096 || model.contains('\0'))
            || self
                .confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            Err(Error::invalid("generation metadata is invalid"))
        } else {
            Ok(())
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub enum Origin {
    User,
    Generated(Generation),
}

impl Origin {
    pub(crate) fn validate(&self) -> Result<()> {
        if let Self::Generated(generation) = self {
            generation.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Authored<T> {
    pub value: T,
    pub origin: Origin,
}

impl<T> Authored<T> {
    #[must_use]
    pub fn user(value: T) -> Self {
        Self {
            value,
            origin: Origin::User,
        }
    }

    #[must_use]
    pub fn generated(value: T, generation: Generation) -> Self {
        Self {
            value,
            origin: Origin::Generated(generation),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SourceText {
    pub text: Authored<String>,
    pub language: Option<LanguageTag>,
}

impl SceneComponent for SourceText {
    const KIND: &'static str = "dev.koharu.scene.source-text";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(
            &StoredSourceText {
                text: self.text.value.clone(),
                origin: self.text.origin.clone(),
                language: self.language.clone(),
            },
            1,
        )
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        let value: StoredSourceText = revision_decode(Self::KIND, schema, 1, payload)?;
        Ok(Self {
            text: Authored {
                value: value.text,
                origin: value.origin,
            },
            language: value.language,
        })
    }
    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        if let Some(language) = &self.language {
            language.validate()?;
        }
        validate_authored_text(&self.text)
    }
    fn origin(&self) -> Option<&Origin> {
        Some(&self.text.origin)
    }
    fn set_origin(&mut self, origin: Origin) -> bool {
        self.text.origin = origin;
        true
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Translation {
    pub text: Authored<String>,
}

impl SceneComponent for Translation {
    const KIND: &'static str = "dev.koharu.scene.translation";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(
            &StoredTranslation {
                text: self.text.value.clone(),
                origin: self.text.origin.clone(),
            },
            1,
        )
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        let value: StoredTranslation = revision_decode(Self::KIND, schema, 1, payload)?;
        Ok(Self {
            text: Authored {
                value: value.text,
                origin: value.origin,
            },
        })
    }
    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        validate_authored_text(&self.text)
    }
    fn origin(&self) -> Option<&Origin> {
        Some(&self.text.origin)
    }
    fn set_origin(&mut self, origin: Origin) -> bool {
        self.text.origin = origin;
        true
    }
}

fn validate_authored_text(value: &Authored<String>) -> Result<()> {
    if value.value.len() > 16 * 1024 * 1024 || value.value.contains('\0') {
        return Err(Error::invalid("text is too large or contains NUL"));
    }
    if let Origin::Generated(generation) = &value.origin {
        generation.validate()?;
    }
    Ok(())
}

#[revisioned(revision = 1)]
#[derive(Clone)]
struct StoredSourceText {
    text: String,
    origin: Origin,
    language: Option<LanguageTag>,
}

#[revisioned(revision = 1)]
#[derive(Clone)]
struct StoredTranslation {
    text: String,
    origin: Origin,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TextRole {
    pub origin: Origin,
    pub role: String,
}
impl SceneComponent for TextRole {
    const KIND: &'static str = "dev.koharu.scene.text-role";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }
    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        validate_namespaced(&self.role, "text role")
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

impl SceneComponent for OcrAnalysis {
    const KIND: &'static str = "dev.koharu.scene.ocr-analysis";
    const CURRENT_SCHEMA: u32 = 1;

    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }

    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }

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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ReadingOrder {
    pub origin: Origin,
    pub index: u32,
}

impl SceneComponent for ReadingOrder {
    const KIND: &'static str = "dev.koharu.scene.reading-order";
    const CURRENT_SCHEMA: u32 = 1;

    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }

    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()
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
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum TextAlignment {
    Start,
    Center,
    End,
    Justify,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum WritingMode {
    Horizontal,
    Vertical,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Typography {
    pub origin: Origin,
    pub preferred_font: Option<String>,
    pub size: Option<f32>,
    pub alignment: Option<TextAlignment>,
    pub writing_mode: Option<WritingMode>,
    pub extensions: BTreeMap<String, String>,
}

impl SceneComponent for Typography {
    const KIND: &'static str = "dev.koharu.scene.typography";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }
    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        if self
            .preferred_font
            .as_ref()
            .is_some_and(|font| font.len() > 4096)
            || self
                .size
                .is_some_and(|size| !size.is_finite() || size <= 0.0)
        {
            return Err(Error::invalid("typography intent is invalid"));
        }
        if let Origin::Generated(generation) = &self.origin {
            generation.validate()?;
        }
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
    fn from_str(v: &str) -> Result<Self> {
        Self::new(v)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Region {
    pub origin: Origin,
    pub kind: RegionKind,
    pub label: Option<String>,
}
impl SceneComponent for Region {
    const KIND: &'static str = "dev.koharu.scene.region";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }
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

impl SceneComponent for DetectionAnalysis {
    const KIND: &'static str = "dev.koharu.scene.detection-analysis";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }
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

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct EntityOrigin {
    pub origin: Origin,
}
impl SceneComponent for EntityOrigin {
    const KIND: &'static str = "dev.koharu.scene.entity-origin";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }
    fn origin(&self) -> Option<&Origin> {
        Some(&self.origin)
    }
    fn set_origin(&mut self, origin: Origin) -> bool {
        self.origin = origin;
        true
    }
    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        if let Origin::Generated(generation) = &self.origin {
            generation.validate()?;
        }
        Ok(())
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AssetMetadata {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub attributes: BTreeMap<String, String>,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Asset {
    pub origin: Origin,
    pub blob: BlobId,
    pub media_type: String,
    pub metadata: AssetMetadata,
}

impl SceneComponent for Asset {
    const KIND: &'static str = "dev.koharu.scene.asset";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }
    fn blob_refs(&self) -> Vec<BlobId> {
        vec![self.blob]
    }
    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        let dimensions = self.metadata.width.is_some() == self.metadata.height.is_some()
            && self.metadata.width.is_none_or(|value| value > 0)
            && self.metadata.height.is_none_or(|value| value > 0);
        if !dimensions
            || self.media_type.len() > 255
            || !self.media_type.contains('/')
            || self.media_type.contains('\0')
            || self.metadata.attributes.len() > 4096
            || self.metadata.attributes.iter().any(|(key, value)| {
                key.is_empty()
                    || key.len() > 255
                    || key.contains('\0')
                    || value.len() > 64 * 1024
                    || value.contains('\0')
            })
        {
            return Err(Error::invalid("asset metadata is invalid"));
        }
        if context.contains_blob(self.blob) {
            Ok(())
        } else {
            Err(Error::invalid("asset blob is missing"))
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

#[derive(Clone, Debug)]
pub struct AssetInput {
    pub bytes: Arc<[u8]>,
    pub media_type: String,
    pub metadata: AssetMetadata,
}

impl AssetInput {
    #[must_use]
    pub fn new(
        bytes: impl Into<Arc<[u8]>>,
        media_type: impl Into<String>,
        metadata: AssetMetadata,
    ) -> Self {
        Self {
            bytes: bytes.into(),
            media_type: media_type.into(),
            metadata,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetRole(String);

impl AssetRole {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 127
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            Err(Error::invalid("asset role is invalid"))
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RelationKind(String);

impl RelationKind {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_namespaced(&value, "relation kind")?;
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn validate(&self) -> Result<()> {
        validate_namespaced(&self.0, "relation kind")
    }
}
impl FromStr for RelationKind {
    type Err = Error;
    fn from_str(v: &str) -> Result<Self> {
        Self::new(v)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Relation {
    pub origin: Origin,
    pub kind: RelationKind,
    pub source: EntityId,
    pub target: EntityId,
}

impl SceneComponent for Relation {
    const KIND: &'static str = "dev.koharu.scene.relation";
    const CURRENT_SCHEMA: u32 = 1;
    fn encode(&self) -> Result<EncodedSceneComponent> {
        revision_encode(self, 1)
    }
    fn decode(schema: u32, payload: &[u8]) -> Result<Self> {
        revision_decode(Self::KIND, schema, 1, payload)
    }
    fn record_refs(&self) -> Vec<EntityId> {
        vec![self.source, self.target]
    }
    fn validate(&self, context: &ValidationContext<'_>) -> Result<()> {
        self.origin.validate()?;
        self.kind.validate()?;
        if context.contains_entity(self.source) && context.contains_entity(self.target) {
            Ok(())
        } else {
            Err(Error::invalid("relation endpoint is missing"))
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
