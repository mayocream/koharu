use std::sync::Arc;

use crate::{
    Applied, CanvasSize, CommandBatch, Error, NodeBuilder, NodeId, Page, PageId, PagePosition,
    Parent, Position, Result, Scene, Session, TextLayout, TextStyle, Transform,
};

pub struct Edit<'a> {
    session: &'a mut Session,
    batch: CommandBatch,
}

impl<'a> Edit<'a> {
    pub(crate) fn new(session: &'a mut Session) -> Self {
        let batch = CommandBatch::new(session.revision());
        Self { session, batch }
    }

    pub fn create_page(&mut self, page: Page) -> Result<PageId> {
        self.batch.create_page(page)
    }

    pub fn move_page(&mut self, page: PageId, position: PagePosition) -> Result<()> {
        self.batch.move_page(page, position)
    }

    pub fn remove_page(&mut self, page: PageId) -> Result<()> {
        self.batch.remove_page(page)
    }

    pub fn page(&mut self, page: PageId) -> Result<PageEdit<'_>> {
        if !self.session.scene().contains_page(page) && !self.batch.creates_page(page) {
            return Err(Error::PageNotFound(page));
        }
        Ok(PageEdit {
            scene: self.session.scene(),
            batch: &mut self.batch,
            page,
        })
    }

    pub fn commit(self) -> Result<Applied> {
        self.session.apply(self.batch)
    }
}

pub struct PageEdit<'a> {
    scene: &'a Scene,
    batch: &'a mut CommandBatch,
    page: PageId,
}

impl<'a> PageEdit<'a> {
    pub fn create(&mut self, builder: NodeBuilder) -> Result<NodeId> {
        self.create_at(Position::Top, builder)
    }

    pub fn create_at(&mut self, position: Position, builder: NodeBuilder) -> Result<NodeId> {
        self.batch
            .create(Parent::Page(self.page), position, builder)
    }

    pub fn rename(&mut self, name: impl Into<String>) -> Result<()> {
        self.batch.rename_page(self.page, name)
    }

    pub fn resize(&mut self, size: CanvasSize) -> Result<()> {
        self.batch.resize_page(self.page, size)
    }

    pub fn move_to(&mut self, position: PagePosition) -> Result<()> {
        self.batch.move_page(self.page, position)
    }

    pub fn remove(self) -> Result<()> {
        self.batch.remove_page(self.page)
    }

    pub fn node(self, node: NodeId) -> Result<NodeEdit<'a>> {
        self.ensure_node(node)?;
        Ok(NodeEdit {
            batch: self.batch,
            node,
        })
    }

    pub fn text(self, node: NodeId) -> Result<TextEdit<'a>> {
        self.ensure_node(node)?;
        if let Ok(committed) = self.scene.node(node)
            && !matches!(committed.kind(), crate::NodeKind::Text(_))
        {
            return Err(Error::WrongNodeKind {
                node,
                expected: "text",
                actual: committed.kind().name(),
            });
        }
        Ok(TextEdit {
            batch: self.batch,
            node,
        })
    }

    pub fn container(self, node: NodeId) -> Result<ContainerEdit<'a>> {
        self.ensure_node(node)?;
        if let Ok(committed) = self.scene.node(node)
            && !committed.is_container()
        {
            return Err(Error::WrongNodeKind {
                node,
                expected: "group or mask container",
                actual: committed.kind().name(),
            });
        }
        Ok(ContainerEdit {
            scene: self.scene,
            batch: self.batch,
            node,
        })
    }

    fn ensure_node(&self, node: NodeId) -> Result<()> {
        if self.scene.contains_node(node) || self.batch.creates_node(node) {
            Ok(())
        } else {
            Err(Error::NodeNotFound(node))
        }
    }
}

pub struct ContainerEdit<'a> {
    scene: &'a Scene,
    batch: &'a mut CommandBatch,
    node: NodeId,
}

impl ContainerEdit<'_> {
    pub fn create(&mut self, builder: NodeBuilder) -> Result<NodeId> {
        self.create_at(Position::Top, builder)
    }

    pub fn create_at(&mut self, position: Position, builder: NodeBuilder) -> Result<NodeId> {
        self.batch
            .create(Parent::Node(self.node), position, builder)
    }

    pub fn node(&mut self, node: NodeId) -> Result<NodeEdit<'_>> {
        if !self.scene.contains_node(node) && !self.batch.creates_node(node) {
            return Err(Error::NodeNotFound(node));
        }
        Ok(NodeEdit {
            batch: self.batch,
            node,
        })
    }
}

pub struct NodeEdit<'a> {
    batch: &'a mut CommandBatch,
    node: NodeId,
}

impl NodeEdit<'_> {
    pub fn set_name(&mut self, name: impl Into<String>) -> Result<()> {
        self.batch.set_name(self.node, Some(name.into()))
    }

    pub fn clear_name(&mut self) -> Result<()> {
        self.batch.set_name(self.node, None)
    }

    pub fn set_visible(&mut self, visible: bool) -> Result<()> {
        self.batch.set_visible(self.node, visible)
    }

    pub fn set_opacity(&mut self, opacity: f32) -> Result<()> {
        self.batch.set_opacity(self.node, opacity)
    }

    pub fn set_transform(&mut self, transform: Transform) -> Result<()> {
        self.batch.set_transform(self.node, transform)
    }

    pub fn set_image(&mut self, bytes: impl Into<Arc<[u8]>>) -> Result<()> {
        self.batch.set_image(self.node, bytes)
    }

    pub fn set_mask(&mut self, bytes: impl Into<Arc<[u8]>>) -> Result<()> {
        self.batch.set_mask(self.node, bytes)
    }

    pub fn place_above(&mut self, anchor: NodeId) -> Result<()> {
        self.batch.place_above(self.node, anchor)
    }

    pub fn place_below(&mut self, anchor: NodeId) -> Result<()> {
        self.batch.place_below(self.node, anchor)
    }

    pub fn move_to(&mut self, parent: Parent, position: Position) -> Result<()> {
        self.batch.move_node(self.node, parent, position)
    }

    pub fn remove(&mut self) -> Result<()> {
        self.batch.remove_node(self.node)
    }
}

pub struct TextEdit<'a> {
    batch: &'a mut CommandBatch,
    node: NodeId,
}

impl TextEdit<'_> {
    pub fn set_text(&mut self, text: impl Into<String>) -> Result<()> {
        self.batch.set_text(self.node, text)
    }

    pub fn set_style(&mut self, style: TextStyle) -> Result<()> {
        self.batch.set_text_style(self.node, style)
    }

    pub fn set_layout(&mut self, layout: TextLayout) -> Result<()> {
        self.batch.set_text_layout(self.node, layout)
    }
}
