use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use revision::revisioned;
use serde::{Deserialize, Serialize};

use crate::{
    BlobId, Element, ElementId, Frame, ImageElement, Page, PageAsset, PageAssets, PageId, Result,
    Revision, Size, SourceText, TextBlock, TextLayout, TextStyle, blob,
};

#[derive(Clone)]
pub struct Commands {
    pub(crate) base: Revision,
    pub(crate) ops: Vec<Command>,
    pub(crate) attachments: HashMap<BlobId, blob::Attachment>,
}

impl Commands {
    #[must_use]
    pub fn new(base: Revision) -> Self {
        Self {
            base,
            ops: Vec::new(),
            attachments: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn base(&self) -> Revision {
        self.base
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Command] {
        &self.ops
    }

    pub fn push(&mut self, command: Command) {
        self.ops.push(command);
    }

    pub fn merge(&mut self, other: Self) -> Result<()> {
        if self.base != other.base {
            return Err(crate::Error::RevisionConflict {
                expected: self.base,
                actual: other.base,
            });
        }
        if Footprint::of(&self.ops).overlaps(&Footprint::of(&other.ops)) {
            return Err(crate::Error::CommandConflict);
        }
        self.ops.extend(other.ops);
        for (id, attachment) in other.attachments {
            if let Some(existing) = self.attachments.get(&id) {
                debug_assert_eq!(existing.bytes.as_ref(), attachment.bytes.as_ref());
            } else {
                self.attachments.insert(id, attachment);
            }
        }
        Ok(())
    }

    pub fn add_page(
        &mut self,
        name: impl Into<String>,
        source: impl Into<Arc<[u8]>>,
    ) -> Result<PageId> {
        let source = self.attach(source, false)?;
        let id = PageId::new();
        self.push(Command::InsertPage {
            page: Page {
                id,
                name: name.into(),
                size: source.size,
                source: source.id,
                assets: PageAssets::default(),
                elements: Vec::new(),
            },
            index: usize::MAX,
        });
        Ok(id)
    }

    pub fn replace_source(&mut self, page: PageId, bytes: impl Into<Arc<[u8]>>) -> Result<BlobId> {
        let source = self.attach(bytes, false)?;
        self.push(Command::ReplaceSource {
            page,
            blob: source.id,
            size: source.size,
        });
        Ok(source.id)
    }

    pub fn set_asset(
        &mut self,
        page: PageId,
        asset: PageAsset,
        bytes: Option<impl Into<Arc<[u8]>>>,
    ) -> Result<Option<BlobId>> {
        let blob = bytes
            .map(|bytes| self.attach(bytes, asset.is_mask()))
            .transpose()?
            .map(|attachment| attachment.id);
        self.push(Command::SetPageAsset { page, asset, blob });
        Ok(blob)
    }

    pub fn add_text(&mut self, page: PageId, frame: Frame) -> ElementId {
        let id = ElementId::new();
        self.push(Command::InsertElement {
            page,
            element: Element::new_text(id, frame, TextBlock::default()),
            index: usize::MAX,
        });
        id
    }

    pub fn add_image(
        &mut self,
        page: PageId,
        frame: Frame,
        name: impl Into<String>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<ElementId> {
        let image = self.attach(bytes, false)?;
        let id = ElementId::new();
        self.push(Command::InsertElement {
            page,
            element: Element::new_image(
                id,
                frame,
                ImageElement {
                    blob: image.id,
                    natural_size: image.size,
                    name: name.into(),
                },
            ),
            index: usize::MAX,
        });
        Ok(id)
    }

    pub fn replace_image(
        &mut self,
        page: PageId,
        element: ElementId,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<BlobId> {
        let image = self.attach(bytes, false)?;
        self.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Image {
                blob: image.id,
                natural_size: image.size,
            },
        });
        Ok(image.id)
    }

    fn attach(&mut self, bytes: impl Into<Arc<[u8]>>, mask: bool) -> Result<blob::Attachment> {
        let attachment = blob::attach(bytes, mask)?;
        self.attachments
            .entry(attachment.id)
            .or_insert_with(|| attachment.clone());
        Ok(attachment)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    InsertPage {
        page: Page,
        index: usize,
    },
    DeletePage(PageId),
    MovePage {
        page: PageId,
        index: usize,
    },
    RenamePage {
        page: PageId,
        name: String,
    },
    ReplaceSource {
        page: PageId,
        blob: BlobId,
        size: Size,
    },
    SetPageAsset {
        page: PageId,
        asset: PageAsset,
        blob: Option<BlobId>,
    },
    InsertElement {
        page: PageId,
        element: Element,
        index: usize,
    },
    DeleteElement {
        page: PageId,
        element: ElementId,
    },
    MoveElement {
        page: PageId,
        element: ElementId,
        index: usize,
    },
    EditElement {
        page: PageId,
        element: ElementId,
        edit: ElementChange,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ElementChange {
    Frame(Frame),
    Visible(bool),
    Opacity(f32),
    Source(Option<SourceText>),
    Translation(Option<String>),
    Style(TextStyle),
    Layout(TextLayout),
    Image { blob: BlobId, natural_size: Size },
    ImageName(String),
}

#[derive(Default)]
struct Footprint {
    pages: HashSet<PageId>,
    page_fields: HashSet<(PageId, PageField)>,
    elements: HashSet<(PageId, ElementId)>,
    element_fields: HashSet<(PageId, ElementId, ElementField)>,
}

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
enum PageField {
    Move,
    Name,
    Source,
    Asset(PageAsset),
}

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
enum ElementField {
    Move,
    Frame,
    Visible,
    Opacity,
    Source,
    Translation,
    Style,
    Layout,
    Image,
    Name,
}

impl Footprint {
    fn of(commands: &[Command]) -> Self {
        let mut footprint = Self::default();
        for command in commands {
            match command {
                Command::InsertPage { page, .. } => {
                    footprint.pages.insert(page.id);
                }
                Command::DeletePage(page) => {
                    footprint.pages.insert(*page);
                }
                Command::MovePage { page, .. } => {
                    footprint.page_fields.insert((*page, PageField::Move));
                }
                Command::RenamePage { page, .. } => {
                    footprint.page_fields.insert((*page, PageField::Name));
                }
                Command::ReplaceSource { page, .. } => {
                    footprint.page_fields.insert((*page, PageField::Source));
                }
                Command::SetPageAsset { page, asset, .. } => {
                    footprint
                        .page_fields
                        .insert((*page, PageField::Asset(*asset)));
                }
                Command::InsertElement { page, element, .. } => {
                    footprint.elements.insert((*page, element.id));
                }
                Command::DeleteElement { page, element } => {
                    footprint.elements.insert((*page, *element));
                }
                Command::MoveElement { page, element, .. } => {
                    footprint
                        .element_fields
                        .insert((*page, *element, ElementField::Move));
                }
                Command::EditElement {
                    page,
                    element,
                    edit,
                } => {
                    footprint
                        .element_fields
                        .insert((*page, *element, edit.field()));
                }
            }
        }
        footprint
    }

    fn overlaps(&self, other: &Self) -> bool {
        if self.pages.iter().any(|page| other.touches_page(*page))
            || other.pages.iter().any(|page| self.touches_page(*page))
            || self
                .page_fields
                .iter()
                .any(|field| other.page_fields.contains(field))
            || self
                .elements
                .iter()
                .any(|element| other.touches_element(*element))
            || other
                .elements
                .iter()
                .any(|element| self.touches_element(*element))
        {
            return true;
        }
        self.element_fields
            .iter()
            .any(|field| other.element_fields.contains(field))
    }

    fn touches_page(&self, page: PageId) -> bool {
        self.pages.contains(&page)
            || self.page_fields.iter().any(|(id, _)| *id == page)
            || self.elements.iter().any(|(id, _)| *id == page)
            || self.element_fields.iter().any(|(id, _, _)| *id == page)
    }

    fn touches_element(&self, element: (PageId, ElementId)) -> bool {
        self.elements.contains(&element)
            || self
                .element_fields
                .iter()
                .any(|(page, id, _)| (*page, *id) == element)
    }
}

impl ElementChange {
    const fn field(&self) -> ElementField {
        match self {
            Self::Frame(_) => ElementField::Frame,
            Self::Visible(_) => ElementField::Visible,
            Self::Opacity(_) => ElementField::Opacity,
            Self::Source(_) => ElementField::Source,
            Self::Translation(_) => ElementField::Translation,
            Self::Style(_) => ElementField::Style,
            Self::Layout(_) => ElementField::Layout,
            Self::Image { .. } => ElementField::Image,
            Self::ImageName(_) => ElementField::Name,
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StoredBatch {
    pub changes: Vec<StoredChange>,
}

impl StoredBatch {
    pub(crate) fn reversed(&self) -> Self {
        Self {
            changes: self
                .changes
                .iter()
                .rev()
                .map(StoredChange::reversed)
                .collect(),
        }
    }

    pub(crate) fn blob_ids(&self, output: &mut HashSet<BlobId>) {
        for change in &self.changes {
            change.blob_ids(output);
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PositionedPage {
    pub index: usize,
    pub page: Page,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PositionedElement {
    pub index: usize,
    pub element: Element,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StoredChange {
    Page {
        before: Option<PositionedPage>,
        after: Option<PositionedPage>,
    },
    MovePage {
        page: PageId,
        before: usize,
        after: usize,
    },
    PageName {
        page: PageId,
        before: String,
        after: String,
    },
    PageSource {
        page: PageId,
        before: BlobId,
        after: BlobId,
    },
    PageAsset {
        page: PageId,
        asset: PageAsset,
        before: Option<BlobId>,
        after: Option<BlobId>,
    },
    Element {
        page: PageId,
        before: Option<PositionedElement>,
        after: Option<PositionedElement>,
    },
}

impl StoredChange {
    pub(crate) fn reversed(&self) -> Self {
        match self.clone() {
            Self::Page { before, after } => Self::Page {
                before: after,
                after: before,
            },
            Self::MovePage {
                page,
                before,
                after,
            } => Self::MovePage {
                page,
                before: after,
                after: before,
            },
            Self::PageName {
                page,
                before,
                after,
            } => Self::PageName {
                page,
                before: after,
                after: before,
            },
            Self::PageSource {
                page,
                before,
                after,
            } => Self::PageSource {
                page,
                before: after,
                after: before,
            },
            Self::PageAsset {
                page,
                asset,
                before,
                after,
            } => Self::PageAsset {
                page,
                asset,
                before: after,
                after: before,
            },
            Self::Element {
                page,
                before,
                after,
            } => Self::Element {
                page,
                before: after,
                after: before,
            },
        }
    }

    fn blob_ids(&self, output: &mut HashSet<BlobId>) {
        match self {
            Self::Page { before, after } => {
                for positioned in [before, after].into_iter().flatten() {
                    positioned.page.blob_ids(output);
                }
            }
            Self::PageSource { before, after, .. } => {
                output.extend([*before, *after]);
            }
            Self::PageAsset { before, after, .. } => {
                output.extend([*before, *after].into_iter().flatten());
            }
            Self::Element { before, after, .. } => {
                for positioned in [before, after].into_iter().flatten() {
                    if let crate::ElementKind::Image(image) = &positioned.element.kind {
                        output.insert(image.blob);
                    }
                }
            }
            Self::MovePage { .. } | Self::PageName { .. } => {}
        }
    }
}
