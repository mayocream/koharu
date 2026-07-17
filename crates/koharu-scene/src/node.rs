use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{BlobId, NodeId, PixelSize, TextLayout, TextStyle, Transform};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub(crate) id: NodeId,
    pub(crate) name: Option<String>,
    pub(crate) visible: bool,
    pub(crate) opacity: f32,
    pub(crate) transform: Transform,
    pub(crate) kind: NodeKind,
}

impl Node {
    #[must_use]
    pub const fn id(&self) -> NodeId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    #[must_use]
    pub const fn opacity(&self) -> f32 {
        self.opacity
    }

    #[must_use]
    pub const fn transform(&self) -> Transform {
        self.transform
    }

    #[must_use]
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }

    #[must_use]
    pub const fn is_container(&self) -> bool {
        self.kind.is_container()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Group,
    Mask(MaskNode),
    Image(ImageNode),
    Text(Box<TextNode>),
}

impl NodeKind {
    #[must_use]
    pub const fn is_container(&self) -> bool {
        matches!(self, Self::Group | Self::Mask(_))
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Mask(_) => "mask",
            Self::Image(_) => "image",
            Self::Text(_) => "text",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageNode {
    pub(crate) blob: BlobId,
    pub(crate) natural_size: PixelSize,
}

impl ImageNode {
    #[must_use]
    pub const fn blob(&self) -> BlobId {
        self.blob
    }

    #[must_use]
    pub const fn natural_size(&self) -> PixelSize {
        self.natural_size
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaskNode {
    pub(crate) blob: BlobId,
    pub(crate) natural_size: PixelSize,
}

impl MaskNode {
    #[must_use]
    pub const fn blob(&self) -> BlobId {
        self.blob
    }

    #[must_use]
    pub const fn natural_size(&self) -> PixelSize {
        self.natural_size
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextNode {
    pub(crate) text: String,
    pub(crate) style: TextStyle,
    pub(crate) layout: TextLayout,
}

impl TextNode {
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn style(&self) -> &TextStyle {
        &self.style
    }

    #[must_use]
    pub const fn layout(&self) -> &TextLayout {
        &self.layout
    }
}

/// Construction value consumed by [`crate::CommandBatch`].
///
/// Image bytes live here only until the builder is added to a command batch;
/// committed nodes never retain them.
#[derive(Clone, Debug)]
pub struct NodeBuilder {
    pub(crate) id: NodeId,
    pub(crate) name: Option<String>,
    pub(crate) visible: bool,
    pub(crate) opacity: f32,
    pub(crate) transform: Transform,
    pub(crate) kind: BuilderKind,
}

#[derive(Clone, Debug)]
pub(crate) enum BuilderKind {
    Group,
    Mask(Arc<[u8]>),
    Image(Arc<[u8]>),
    Text(Box<TextNode>),
}

impl NodeBuilder {
    fn new(kind: BuilderKind) -> Self {
        Self {
            id: NodeId::new(),
            name: None,
            visible: true,
            opacity: 1.0,
            transform: Transform::IDENTITY,
            kind,
        }
    }

    #[must_use]
    pub const fn id(&self) -> NodeId {
        self.id
    }

    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[must_use]
    pub const fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    #[must_use]
    pub const fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    #[must_use]
    pub const fn at(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }
}

#[must_use]
pub fn group() -> NodeBuilder {
    NodeBuilder::new(BuilderKind::Group)
}

#[must_use]
pub fn image(bytes: impl Into<Arc<[u8]>>) -> NodeBuilder {
    NodeBuilder::new(BuilderKind::Image(bytes.into()))
}

#[must_use]
pub fn mask(bytes: impl Into<Arc<[u8]>>) -> NodeBuilder {
    NodeBuilder::new(BuilderKind::Mask(bytes.into()))
}

#[must_use]
pub fn text(text: impl Into<String>, style: TextStyle, layout: TextLayout) -> NodeBuilder {
    NodeBuilder::new(BuilderKind::Text(Box::new(TextNode {
        text: text.into(),
        style,
        layout,
    })))
}
