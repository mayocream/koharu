use std::collections::HashSet;

use revision::revisioned;
use serde::{Deserialize, Serialize};

use crate::{BlobId, ElementId, Error, Frame, PageId, Quad, Result, Size, TextLayout, TextStyle};

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub pages: Vec<Page>,
}

impl Project {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pages: Vec::new(),
        }
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

    pub fn texts(&self) -> impl Iterator<Item = (&Element, &TextBlock)> {
        self.elements
            .iter()
            .filter_map(|element| element.text().map(|text| (element, text)))
    }

    pub(crate) fn validate(&self, element_ids: &mut HashSet<ElementId>) -> Result<()> {
        if !self.size.is_valid() {
            return Err(Error::invalid(format!(
                "page {} has an empty size",
                self.id
            )));
        }
        for element in &self.elements {
            if !element_ids.insert(element.id) {
                return Err(Error::invalid(format!("duplicate element {}", element.id)));
            }
            element.validate()?;
        }
        Ok(())
    }

    pub(crate) fn blob_ids(&self, output: &mut HashSet<BlobId>) {
        output.insert(self.source);
        self.assets.blob_ids(output);
        for element in &self.elements {
            if let ElementKind::Image(image) = &element.kind {
                output.insert(image.blob);
            }
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageAssets {
    pub clean: Option<BlobId>,
    pub rendered: Option<BlobId>,
    pub text_mask: Option<BlobId>,
    pub bubble_mask: Option<BlobId>,
    pub brush_mask: Option<BlobId>,
}

impl PageAssets {
    #[must_use]
    pub const fn get(&self, asset: PageAsset) -> Option<BlobId> {
        match asset {
            PageAsset::Clean => self.clean,
            PageAsset::Rendered => self.rendered,
            PageAsset::TextMask => self.text_mask,
            PageAsset::BubbleMask => self.bubble_mask,
            PageAsset::BrushMask => self.brush_mask,
        }
    }

    pub(crate) fn set(&mut self, asset: PageAsset, blob: Option<BlobId>) {
        *match asset {
            PageAsset::Clean => &mut self.clean,
            PageAsset::Rendered => &mut self.rendered,
            PageAsset::TextMask => &mut self.text_mask,
            PageAsset::BubbleMask => &mut self.bubble_mask,
            PageAsset::BrushMask => &mut self.brush_mask,
        } = blob;
    }

    fn blob_ids(&self, output: &mut HashSet<BlobId>) {
        output.extend(
            [
                self.clean,
                self.rendered,
                self.text_mask,
                self.bubble_mask,
                self.brush_mask,
            ]
            .into_iter()
            .flatten(),
        );
    }
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum PageAsset {
    Clean,
    Rendered,
    TextMask,
    BubbleMask,
    BrushMask,
}

impl PageAsset {
    pub(crate) const fn is_mask(self) -> bool {
        matches!(self, Self::TextMask | Self::BubbleMask | Self::BrushMask)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    pub const fn text(&self) -> Option<&TextBlock> {
        match &self.kind {
            ElementKind::Text(text) => Some(text),
            ElementKind::Image(_) => None,
        }
    }

    #[must_use]
    pub const fn image_data(&self) -> Option<&ImageElement> {
        match &self.kind {
            ElementKind::Image(image) => Some(image),
            ElementKind::Text(_) => None,
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
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ElementKind {
    Text(TextBlock),
    Image(ImageElement),
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageElement {
    pub blob: BlobId,
    pub natural_size: Size,
    pub name: String,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    pub source: Option<SourceText>,
    pub translation: Option<String>,
    pub style: TextStyle,
    pub layout: TextLayout,
}

impl TextBlock {
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
        Ok(())
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TextDirection {
    #[default]
    Auto,
    Horizontal,
    Vertical,
}
