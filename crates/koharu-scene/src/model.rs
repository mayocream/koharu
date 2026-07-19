use std::collections::HashSet;

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{BlobId, ElementId, Error, Frame, PageId, Quad, Result, Size, TextLayout, TextStyle};

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Project {
    pub pages: Vec<Page>,
}

impl Project {
    #[must_use]
    pub const fn new() -> Self {
        Self { pages: Vec::new() }
    }

    #[must_use]
    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.iter().find(|page| page.id == id)
    }

    #[must_use]
    pub fn element(&self, id: ElementId) -> Option<(&Page, &Element)> {
        self.pages
            .iter()
            .find_map(|page| page.element(id).map(|element| (page, element)))
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let mut pages = HashSet::with_capacity(self.pages.len());
        let mut elements = HashSet::new();
        for page in &self.pages {
            if !pages.insert(page.id) {
                return Err(Error::invalid(format!("duplicate page {}", page.id)));
            }
            page.validate(&mut elements)?;
        }
        Ok(())
    }

    pub(crate) fn blob_ids(&self, output: &mut HashSet<BlobId>) {
        for page in &self.pages {
            page.blob_ids(output);
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Page {
    pub id: PageId,
    pub name: String,
    pub size: Size,
    pub source: BlobId,
    pub assets: PageAssets,
    /// Bottom-to-top editable content order.
    pub elements: Vec<Element>,
}

impl Page {
    #[must_use]
    pub fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements.iter().find(|element| element.id == id)
    }

    #[must_use]
    pub fn text(&self, id: ElementId) -> Option<&TextBlock> {
        self.element(id)?.text()
    }

    #[must_use]
    pub fn region(&self, id: ElementId) -> Option<&Region> {
        self.element(id)?.region()
    }

    pub fn texts(&self) -> impl Iterator<Item = (&Element, &TextBlock)> {
        self.elements
            .iter()
            .filter_map(|element| element.text().map(|text| (element, text)))
    }

    pub fn regions(&self) -> impl Iterator<Item = (&Element, &Region)> {
        self.elements
            .iter()
            .filter_map(|element| element.region().map(|region| (element, region)))
    }

    pub(crate) fn validate(&self, element_ids: &mut HashSet<ElementId>) -> Result<()> {
        if !self.size.is_valid() {
            return Err(Error::invalid(format!(
                "page {} has an empty size",
                self.id
            )));
        }
        let mut bubble_mask_ids = HashSet::new();
        for element in &self.elements {
            if !element_ids.insert(element.id) {
                return Err(Error::invalid(format!("duplicate element {}", element.id)));
            }
            element.validate()?;
            if let ElementKind::Region(Region {
                kind: RegionKind::Bubble,
                mask_id: Some(mask_id),
                ..
            }) = &element.kind
                && !bubble_mask_ids.insert(*mask_id)
            {
                return Err(Error::invalid(format!(
                    "duplicate bubble mask ID {mask_id} on page {}",
                    self.id
                )));
            }
        }
        for (element, text) in self.texts() {
            for (relation, expected) in [
                (text.panel, RegionKind::Panel),
                (text.bubble, RegionKind::Bubble),
            ] {
                let Some(relation) = relation else {
                    continue;
                };
                if !self.element(relation).is_some_and(|element| {
                    matches!(&element.kind, ElementKind::Region(region) if region.kind == expected)
                }) {
                    return Err(Error::invalid(format!(
                        "text {} references a missing or incompatible {expected:?} region",
                        element.id
                    )));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn blob_ids(&self, output: &mut HashSet<BlobId>) {
        output.insert(self.source);
        self.assets.blob_ids(output);
        for element in &self.elements {
            match &element.kind {
                ElementKind::Image(image) => {
                    output.insert(image.blob);
                }
                ElementKind::Text(_) | ElementKind::Region(_) => {}
            }
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct PageAssets {
    pub clean: Option<BlobId>,
    pub rendered: Option<BlobId>,
    /// Unclassified text foreground produced before semantic mask fusion.
    pub text_mask_candidate: Option<BlobId>,
    /// Text-instance foreground supplied by a layout model.
    pub layout_text_mask: Option<BlobId>,
    pub text_mask: Option<BlobId>,
    pub coo_mask: Option<BlobId>,
    pub bubble_mask: Option<BlobId>,
    pub brush_mask: Option<BlobId>,
}

impl PageAssets {
    #[must_use]
    pub const fn get(&self, asset: PageAsset) -> Option<BlobId> {
        match asset {
            PageAsset::Clean => self.clean,
            PageAsset::Rendered => self.rendered,
            PageAsset::TextMaskCandidate => self.text_mask_candidate,
            PageAsset::LayoutTextMask => self.layout_text_mask,
            PageAsset::TextMask => self.text_mask,
            PageAsset::CooMask => self.coo_mask,
            PageAsset::BubbleMask => self.bubble_mask,
            PageAsset::BrushMask => self.brush_mask,
        }
    }

    pub(crate) fn set(&mut self, asset: PageAsset, blob: Option<BlobId>) {
        *match asset {
            PageAsset::Clean => &mut self.clean,
            PageAsset::Rendered => &mut self.rendered,
            PageAsset::TextMaskCandidate => &mut self.text_mask_candidate,
            PageAsset::LayoutTextMask => &mut self.layout_text_mask,
            PageAsset::TextMask => &mut self.text_mask,
            PageAsset::CooMask => &mut self.coo_mask,
            PageAsset::BubbleMask => &mut self.bubble_mask,
            PageAsset::BrushMask => &mut self.brush_mask,
        } = blob;
    }

    fn blob_ids(&self, output: &mut HashSet<BlobId>) {
        output.extend(
            [
                self.clean,
                self.rendered,
                self.text_mask_candidate,
                self.layout_text_mask,
                self.text_mask,
                self.coo_mask,
                self.bubble_mask,
                self.brush_mask,
            ]
            .into_iter()
            .flatten(),
        );
    }
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
pub enum PageAsset {
    Clean,
    Rendered,
    TextMaskCandidate,
    LayoutTextMask,
    TextMask,
    CooMask,
    BubbleMask,
    BrushMask,
}

impl PageAsset {
    pub(crate) const fn is_mask(self) -> bool {
        matches!(
            self,
            Self::TextMaskCandidate
                | Self::LayoutTextMask
                | Self::TextMask
                | Self::CooMask
                | Self::BubbleMask
                | Self::BrushMask
        )
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Element {
    pub id: ElementId,
    pub frame: Frame,
    pub visible: bool,
    pub opacity: f32,
    pub kind: ElementKind,
}

impl Element {
    #[must_use]
    pub fn new_text(id: ElementId, frame: Frame, text: TextBlock) -> Self {
        Self {
            id,
            frame,
            visible: true,
            opacity: 1.0,
            kind: ElementKind::Text(text),
        }
    }

    #[must_use]
    pub fn new_image(id: ElementId, frame: Frame, image: ImageElement) -> Self {
        Self {
            id,
            frame,
            visible: true,
            opacity: 1.0,
            kind: ElementKind::Image(image),
        }
    }

    #[must_use]
    pub fn new_region(id: ElementId, frame: Frame, region: Region) -> Self {
        Self {
            id,
            frame,
            visible: true,
            opacity: 1.0,
            kind: ElementKind::Region(region),
        }
    }

    #[must_use]
    pub const fn text(&self) -> Option<&TextBlock> {
        match &self.kind {
            ElementKind::Text(text) => Some(text),
            ElementKind::Image(_) | ElementKind::Region(_) => None,
        }
    }

    #[must_use]
    pub const fn image_data(&self) -> Option<&ImageElement> {
        match &self.kind {
            ElementKind::Image(image) => Some(image),
            ElementKind::Text(_) | ElementKind::Region(_) => None,
        }
    }

    #[must_use]
    pub const fn region(&self) -> Option<&Region> {
        match &self.kind {
            ElementKind::Region(region) => Some(region),
            ElementKind::Text(_) | ElementKind::Image(_) => None,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !self.frame.is_valid() {
            return Err(Error::invalid(format!(
                "element {} has an invalid frame",
                self.id
            )));
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err(Error::invalid(format!(
                "element {} has invalid opacity",
                self.id
            )));
        }
        match &self.kind {
            ElementKind::Text(text) => text.validate(),
            ElementKind::Image(image) if image.natural_size.is_valid() => Ok(()),
            ElementKind::Image(_) => Err(Error::invalid("image has an empty natural size")),
            ElementKind::Region(region) => region.validate(),
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub enum ElementKind {
    Text(TextBlock),
    Image(ImageElement),
    Region(Region),
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum RegionKind {
    Panel,
    Bubble,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ModelPrediction {
    pub model: String,
    pub confidence: f32,
}

impl ModelPrediction {
    #[must_use]
    pub fn new(model: impl Into<String>, confidence: f32) -> Self {
        Self {
            model: model.into(),
            confidence,
        }
    }

    fn is_valid(&self) -> bool {
        !self.model.trim().is_empty()
            && self.confidence.is_finite()
            && (0.0..=1.0).contains(&self.confidence)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Region {
    pub kind: RegionKind,
    /// Page-space polygon. An empty polygon means the element frame is authoritative.
    pub polygon: Vec<[f32; 2]>,
    /// Non-zero label in the corresponding page mask, when an instance mask exists.
    pub mask_id: Option<u8>,
    pub reading_order: Option<u32>,
    pub predictions: Vec<ModelPrediction>,
}

impl Region {
    fn validate(&self) -> Result<()> {
        if (!self.polygon.is_empty() && self.polygon.len() < 3)
            || self
                .polygon
                .iter()
                .flatten()
                .any(|coordinate| !coordinate.is_finite())
            || self.mask_id == Some(0)
            || (self.kind == RegionKind::Panel && self.mask_id.is_some())
            || self
                .predictions
                .iter()
                .any(|prediction| !prediction.is_valid())
        {
            return Err(Error::invalid("region has invalid analysis metadata"));
        }
        Ok(())
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct ImageElement {
    pub blob: BlobId,
    pub natural_size: Size,
    pub name: String,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct TextBlock {
    pub source: Option<SourceText>,
    pub translation: Option<String>,
    pub style: TextStyle,
    pub layout: TextLayout,
    pub role: TextRole,
    pub panel: Option<ElementId>,
    pub bubble: Option<ElementId>,
    pub reading_order: Option<u32>,
    /// Page-space region polygon. Empty means the element frame is authoritative.
    pub polygon: Vec<[f32; 2]>,
    pub predictions: Vec<ModelPrediction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct TextAnalysis {
    pub role: TextRole,
    pub panel: Option<ElementId>,
    pub bubble: Option<ElementId>,
    pub reading_order: Option<u32>,
    pub polygon: Vec<[f32; 2]>,
    pub predictions: Vec<ModelPrediction>,
}

impl From<&TextBlock> for TextAnalysis {
    fn from(text: &TextBlock) -> Self {
        Self {
            role: text.role,
            panel: text.panel,
            bubble: text.bubble,
            reading_order: text.reading_order,
            polygon: text.polygon.clone(),
            predictions: text.predictions.clone(),
        }
    }
}

impl TextBlock {
    pub(crate) fn set_analysis(&mut self, analysis: TextAnalysis) {
        self.role = analysis.role;
        self.panel = analysis.panel;
        self.bubble = analysis.bubble;
        self.reading_order = analysis.reading_order;
        self.polygon = analysis.polygon;
        self.predictions = analysis.predictions;
    }

    fn validate(&self) -> Result<()> {
        if self
            .source
            .as_ref()
            .is_some_and(|source| !source.is_valid())
        {
            return Err(Error::invalid("text block has invalid source metadata"));
        }
        if !self.style.is_valid() || !self.layout.is_valid() {
            return Err(Error::invalid("text block has invalid style or layout"));
        }
        if (!self.polygon.is_empty() && self.polygon.len() < 3)
            || self
                .polygon
                .iter()
                .flatten()
                .any(|coordinate| !coordinate.is_finite())
            || self
                .predictions
                .iter()
                .any(|prediction| !prediction.is_valid())
        {
            return Err(Error::invalid("text block has invalid analysis metadata"));
        }
        Ok(())
    }
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub enum TextRole {
    #[default]
    Dialogue,
    Narration,
    FreeText,
    Onomatopoeia,
    Furigana,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SourceText {
    pub text: String,
    pub language: Option<String>,
    pub direction: TextDirection,
    pub confidence: Option<f32>,
    pub lines: Vec<Quad>,
}

impl SourceText {
    fn is_valid(&self) -> bool {
        self.confidence
            .is_none_or(|confidence| confidence.is_finite() && (0.0..=1.0).contains(&confidence))
            && self
                .lines
                .iter()
                .flatten()
                .flatten()
                .all(|coordinate| coordinate.is_finite())
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
