use std::sync::Arc;

use crate::{
    ChangeSet, Command, Commands, ElementChange, ElementId, ElementKind, Error, Frame, PageAsset,
    PageId, Result, Session, SourceText, TextLayout, TextStyle,
};

pub struct Edit<'session> {
    session: &'session mut Session,
    commands: Commands,
}

pub struct PageEdit<'edit, 'session> {
    edit: &'edit mut Edit<'session>,
    page: PageId,
}

pub struct TextEdit<'edit, 'session> {
    edit: &'edit mut Edit<'session>,
    page: PageId,
    element: ElementId,
}

pub struct ImageEdit<'edit, 'session> {
    edit: &'edit mut Edit<'session>,
    page: PageId,
    element: ElementId,
}

impl Session {
    pub fn edit(&mut self) -> Edit<'_> {
        let commands = self.commands();
        Edit {
            session: self,
            commands,
        }
    }
}

impl<'session> Edit<'session> {
    pub fn add_page(
        &mut self,
        name: impl Into<String>,
        source: impl Into<Arc<[u8]>>,
    ) -> Result<PageId> {
        self.commands.add_page(name, source)
    }

    pub fn page(&mut self, page: PageId) -> Result<PageEdit<'_, 'session>> {
        if !self.page_exists(page) {
            return Err(Error::PageNotFound(page));
        }
        Ok(PageEdit { edit: self, page })
    }

    pub fn commit(self) -> Result<ChangeSet> {
        self.session.apply(self.commands)
    }

    fn page_exists(&self, page: PageId) -> bool {
        let mut exists = self.session.project().page(page).is_some();
        for command in self.commands.as_slice() {
            match command {
                Command::InsertPage { page: inserted, .. } if inserted.id == page => exists = true,
                Command::DeletePage(deleted) if *deleted == page => exists = false,
                _ => {}
            }
        }
        exists
    }

    fn element_kind(&self, page: PageId, element: ElementId) -> Option<bool> {
        let mut is_text = self
            .session
            .project()
            .page(page)
            .and_then(|page| page.element(element))
            .map(|element| matches!(element.kind, ElementKind::Text(_)));
        for command in self.commands.as_slice() {
            match command {
                Command::InsertElement {
                    page: target,
                    element: inserted,
                    ..
                } if *target == page && inserted.id == element => {
                    is_text = Some(matches!(inserted.kind, ElementKind::Text(_)));
                }
                Command::DeleteElement {
                    page: target,
                    element: deleted,
                } if *target == page && *deleted == element => {
                    is_text = None;
                }
                _ => {}
            }
        }
        is_text
    }
}

impl<'edit, 'session> PageEdit<'edit, 'session> {
    pub fn rename(&mut self, name: impl Into<String>) -> &mut Self {
        self.edit.commands.push(Command::RenamePage {
            page: self.page,
            name: name.into(),
        });
        self
    }

    pub fn remove(&mut self) -> &mut Self {
        self.edit.commands.push(Command::DeletePage(self.page));
        self
    }

    pub fn move_to(&mut self, index: usize) -> &mut Self {
        self.edit.commands.push(Command::MovePage {
            page: self.page,
            index,
        });
        self
    }

    pub fn replace_source(&mut self, bytes: impl Into<Arc<[u8]>>) -> Result<&mut Self> {
        self.edit.commands.replace_source(self.page, bytes)?;
        Ok(self)
    }

    pub fn set_asset(
        &mut self,
        asset: PageAsset,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<&mut Self> {
        self.edit
            .commands
            .set_asset(self.page, asset, Some(bytes))?;
        Ok(self)
    }

    pub fn clear_asset(&mut self, asset: PageAsset) -> &mut Self {
        self.edit.commands.push(Command::SetPageAsset {
            page: self.page,
            asset,
            blob: None,
        });
        self
    }

    pub fn add_text(&mut self, frame: Frame) -> ElementId {
        self.edit.commands.add_text(self.page, frame)
    }

    pub fn add_image(
        &mut self,
        frame: Frame,
        name: impl Into<String>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<ElementId> {
        self.edit.commands.add_image(self.page, frame, name, bytes)
    }

    pub fn text(self, element: ElementId) -> Result<TextEdit<'edit, 'session>> {
        match self.edit.element_kind(self.page, element) {
            Some(true) => Ok(TextEdit {
                edit: self.edit,
                page: self.page,
                element,
            }),
            Some(false) => Err(Error::ElementKind(element)),
            None => Err(Error::ElementNotFound(element)),
        }
    }

    pub fn image(self, element: ElementId) -> Result<ImageEdit<'edit, 'session>> {
        match self.edit.element_kind(self.page, element) {
            Some(false) => Ok(ImageEdit {
                edit: self.edit,
                page: self.page,
                element,
            }),
            Some(true) => Err(Error::ElementKind(element)),
            None => Err(Error::ElementNotFound(element)),
        }
    }
}

macro_rules! common_element_methods {
    () => {
        pub fn set_frame(&mut self, frame: Frame) -> &mut Self {
            self.change(ElementChange::Frame(frame))
        }

        pub fn set_visible(&mut self, visible: bool) -> &mut Self {
            self.change(ElementChange::Visible(visible))
        }

        pub fn set_opacity(&mut self, opacity: f32) -> &mut Self {
            self.change(ElementChange::Opacity(opacity))
        }

        pub fn move_to(&mut self, index: usize) -> &mut Self {
            self.edit.commands.push(Command::MoveElement {
                page: self.page,
                element: self.element,
                index,
            });
            self
        }

        pub fn remove(&mut self) -> &mut Self {
            self.edit.commands.push(Command::DeleteElement {
                page: self.page,
                element: self.element,
            });
            self
        }

        fn change(&mut self, change: ElementChange) -> &mut Self {
            self.edit.commands.push(Command::EditElement {
                page: self.page,
                element: self.element,
                edit: change,
            });
            self
        }
    };
}

impl TextEdit<'_, '_> {
    common_element_methods!();

    pub fn set_source(&mut self, source: Option<SourceText>) -> &mut Self {
        self.change(ElementChange::Source(source))
    }

    pub fn set_translation(&mut self, translation: Option<impl Into<String>>) -> &mut Self {
        self.change(ElementChange::Translation(translation.map(Into::into)))
    }

    pub fn set_style(&mut self, style: TextStyle) -> &mut Self {
        self.change(ElementChange::Style(style))
    }

    pub fn set_layout(&mut self, layout: TextLayout) -> &mut Self {
        self.change(ElementChange::Layout(layout))
    }
}

impl ImageEdit<'_, '_> {
    common_element_methods!();

    pub fn replace(&mut self, bytes: impl Into<Arc<[u8]>>) -> Result<&mut Self> {
        self.edit
            .commands
            .replace_image(self.page, self.element, bytes)?;
        Ok(self)
    }

    pub fn rename(&mut self, name: impl Into<String>) -> &mut Self {
        self.change(ElementChange::ImageName(name.into()))
    }
}
