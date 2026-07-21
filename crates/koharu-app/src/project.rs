use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use koharu_scene::{ChangeSet, Command, Commands, ElementChange, PageId, Revision, Session};

use crate::protocol::{
    AppCommand, AppErrorCode, PageDelta, PageSummary, ProjectDelta, ProjectHeader,
};

/// The platform-independent state of an open project.
pub struct Project {
    session: Session,
    path: PathBuf,
    visible_page: Option<PageId>,
    undo: Vec<Vec<Revision>>,
    redo: Vec<Vec<Revision>>,
}

impl Project {
    pub fn create(path: PathBuf) -> Result<Self> {
        let session = Session::create(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self::new(session, path))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let session =
            Session::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        Ok(Self::new(session, path))
    }

    #[must_use]
    pub fn new(session: Session, path: PathBuf) -> Self {
        let visible_page = session.project().pages.first().map(|page| page.id);
        Self {
            session,
            path,
            visible_page,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    #[must_use]
    pub const fn session(&self) -> &Session {
        &self.session
    }

    pub const fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn visible_page(&self) -> Option<PageId> {
        self.visible_page
    }

    pub fn show_page(&mut self, page: PageId) -> Result<()> {
        self.session.page(page)?;
        self.visible_page = Some(page);
        Ok(())
    }

    /// Select the first remaining page when the visible page was deleted.
    pub fn reconcile_visible_page(&mut self) {
        if self
            .visible_page
            .is_some_and(|page| self.session.project().page(page).is_some())
        {
            return;
        }
        self.visible_page = self.session.project().pages.first().map(|page| page.id);
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.session.revision()
    }

    pub fn require_base(&self, base: Revision) -> Result<()> {
        let current = self.revision();
        if base != current {
            return Err(failure(
                AppErrorCode::StaleRevision,
                format!("stale scene revision {base}; current revision is {current}"),
            ));
        }
        Ok(())
    }

    pub fn apply(&mut self, command: AppCommand) -> Result<ChangeSet> {
        let commands = build_commands(&self.session, command)?;
        self.apply_commands(commands)
    }

    pub fn apply_commands(&mut self, commands: Commands) -> Result<ChangeSet> {
        let changes = self.session.apply(commands)?;
        self.record_change(&changes);
        Ok(changes)
    }

    pub fn refresh(&mut self) -> Result<ChangeSet> {
        Ok(self.session.refresh()?)
    }

    pub fn undo(&mut self, base: Revision) -> Result<ChangeSet> {
        self.require_base(base)?;
        let group = self.undo.pop().ok_or_else(|| anyhow!("nothing to undo"))?;
        let changes = match self.session.revert(group.iter().copied()) {
            Ok(changes) => changes,
            Err(error) => {
                self.undo.push(group);
                return Err(error.into());
            }
        };
        self.redo.push(vec![changes.to]);
        Ok(changes)
    }

    pub fn redo(&mut self, base: Revision) -> Result<ChangeSet> {
        self.require_base(base)?;
        let group = self.redo.pop().ok_or_else(|| anyhow!("nothing to redo"))?;
        let changes = match self.session.revert(group.iter().copied()) {
            Ok(changes) => changes,
            Err(error) => {
                self.redo.push(group);
                return Err(error.into());
            }
        };
        self.undo.push(vec![changes.to]);
        Ok(changes)
    }

    pub fn record_revisions(&mut self, revisions: Vec<Revision>) {
        if revisions.is_empty() {
            return;
        }
        self.undo.push(revisions);
        self.redo.clear();
    }

    #[must_use]
    pub fn header(&self) -> ProjectHeader {
        ProjectHeader {
            id: self.session.id(),
            name: project_name(&self.path),
            visible_page: self.visible_page,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        }
    }

    pub fn delta(&self, changes: &ChangeSet) -> Result<ProjectDelta> {
        let project = self.session.project();
        let pages = changes
            .pages
            .iter()
            .filter_map(|id| project.page(*id))
            .map(PageSummary::from_page)
            .collect();
        let deleted_pages = changes
            .pages
            .iter()
            .copied()
            .filter(|id| project.page(*id).is_none())
            .collect();
        let visible_page = self
            .visible_page
            .filter(|visible| {
                changes.pages.contains(visible)
                    || changes.elements.iter().any(|element| {
                        project
                            .page(*visible)
                            .is_some_and(|page| page.element(*element).is_some())
                    })
            })
            .and_then(|visible| project.page(visible))
            .map(|page| PageDelta {
                id: page.id,
                name: page.name.clone(),
                size: page.size,
                source: page.source.to_string(),
                assets: (&page.assets).into(),
                element_order: page.elements.iter().map(|element| element.id).collect(),
                elements: changes
                    .elements
                    .iter()
                    .filter_map(|id| page.element(*id).cloned())
                    .collect(),
                deleted_elements: changes
                    .elements
                    .iter()
                    .copied()
                    .filter(|id| page.element(*id).is_none())
                    .collect(),
            });
        Ok(ProjectDelta {
            from: changes.from,
            revision: changes.to,
            name: project_name(&self.path),
            page_order: project.pages.iter().map(|page| page.id).collect(),
            pages,
            deleted_pages,
            visible_page,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        })
    }

    fn record_change(&mut self, changes: &ChangeSet) {
        if changes.to == changes.from {
            return;
        }
        self.undo.push(vec![changes.to]);
        self.redo.clear();
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct AppFailure {
    code: AppErrorCode,
    message: String,
}

pub fn failure(code: AppErrorCode, message: impl std::fmt::Display) -> anyhow::Error {
    AppFailure {
        code,
        message: message.to_string(),
    }
    .into()
}

#[must_use]
pub fn classify_error(error: &anyhow::Error) -> AppErrorCode {
    if let Some(error) = error.downcast_ref::<AppFailure>() {
        return error.code;
    }
    if let Some(error) = error.downcast_ref::<koharu_scene::Error>() {
        return match error {
            koharu_scene::Error::Io(_) | koharu_scene::Error::Sql(_) => AppErrorCode::IoFailed,
            koharu_scene::Error::PageNotFound(_)
            | koharu_scene::Error::ElementNotFound(_)
            | koharu_scene::Error::HistoryNotFound(_) => AppErrorCode::NotFound,
            koharu_scene::Error::RevisionConflict { .. } => AppErrorCode::StaleRevision,
            koharu_scene::Error::Invalid(_)
            | koharu_scene::Error::ElementKind(_)
            | koharu_scene::Error::CommandConflict
            | koharu_scene::Error::HistoryConflict(_) => AppErrorCode::InvalidInput,
            _ => AppErrorCode::IoFailed,
        };
    }
    if error.downcast_ref::<std::io::Error>().is_some() {
        AppErrorCode::IoFailed
    } else {
        AppErrorCode::Internal
    }
}

#[must_use]
pub fn project_name(path: &Path) -> String {
    path.file_stem()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

fn build_commands(session: &Session, command: AppCommand) -> Result<Commands> {
    let mut commands = session.commands();
    match command {
        AppCommand::RenamePage { page, name } => commands.push(Command::RenamePage { page, name }),
        AppCommand::DeletePage { page } => commands.push(Command::DeletePage(page)),
        AppCommand::DeletePages { pages } => {
            for page in pages {
                commands.push(Command::DeletePage(page));
            }
        }
        AppCommand::MovePage { page, index } => commands.push(Command::MovePage { page, index }),
        AppCommand::AddText { page, frame } => {
            commands.add_text(page, frame);
        }
        AppCommand::SetTranslation {
            page,
            element,
            translation,
        } => commands.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Translation(translation),
        }),
        AppCommand::SetTextStyle {
            page,
            element,
            style,
        } => commands.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Style(style),
        }),
        AppCommand::SetTextLayout {
            page,
            element,
            layout,
        } => commands.push(Command::EditElement {
            page,
            element,
            edit: ElementChange::Layout(layout),
        }),
        AppCommand::SetTextStyles { page, elements } => {
            for value in elements {
                commands.push(Command::EditElement {
                    page,
                    element: value.element,
                    edit: ElementChange::Style(value.style),
                });
            }
        }
        AppCommand::SetTextLayouts { page, elements } => {
            for value in elements {
                commands.push(Command::EditElement {
                    page,
                    element: value.element,
                    edit: ElementChange::Layout(value.layout),
                });
            }
        }
        AppCommand::SetElementFrames { elements } => {
            for value in elements {
                commands.push(Command::EditElement {
                    page: value.page,
                    element: value.element,
                    edit: ElementChange::Frame(value.frame),
                });
            }
        }
        AppCommand::SetElementOpacity {
            page,
            elements,
            opacity,
        } => {
            for element in elements {
                commands.push(Command::EditElement {
                    page,
                    element,
                    edit: ElementChange::Opacity(opacity),
                });
            }
        }
        AppCommand::SetElementVisibility {
            page,
            elements,
            visible,
        } => {
            for element in elements {
                commands.push(Command::EditElement {
                    page,
                    element,
                    edit: ElementChange::Visible(visible),
                });
            }
        }
        AppCommand::DeleteElements { page, elements } => {
            for element in elements {
                commands.push(Command::DeleteElement { page, element });
            }
        }
        AppCommand::MoveElement {
            page,
            element,
            index,
        } => commands.push(Command::MoveElement {
            page,
            element,
            index,
        }),
        _ => bail!("command is not a scene edit"),
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_scene::{ElementKind, Frame, TextStyle};

    #[test]
    fn scene_edits_and_history_are_headless() {
        let mut project = memory_project();
        let page = project.visible_page().unwrap();

        let changes = project
            .apply(AppCommand::AddText {
                page,
                frame: Frame::new(1.0, 2.0, 30.0, 40.0),
            })
            .unwrap();
        assert_eq!(changes.elements.len(), 1);
        assert!(project.header().can_undo);

        let undone = project.undo(changes.to).unwrap();
        assert!(project.session().page(page).unwrap().elements.is_empty());
        assert!(project.header().can_redo);

        project.redo(undone.to).unwrap();
        assert_eq!(project.session().page(page).unwrap().elements.len(), 1);
    }

    #[test]
    fn bulk_styles_share_one_scene_revision() {
        let mut project = memory_project();
        let page = project.visible_page().unwrap();
        let mut commands = project.session().commands();
        let first = commands.add_text(page, Frame::new(0.0, 0.0, 20.0, 20.0));
        let second = commands.add_text(page, Frame::new(30.0, 0.0, 20.0, 20.0));
        project.apply_commands(commands).unwrap();
        let before = project.revision();

        let mut first_style = TextStyle::default();
        first_style.font_size = 24.0;
        let mut second_style = TextStyle::default();
        second_style.font_size = 30.0;
        let changes = project
            .apply(AppCommand::SetTextStyles {
                page,
                elements: vec![
                    crate::protocol::ElementTextStyle {
                        element: first,
                        style: first_style,
                    },
                    crate::protocol::ElementTextStyle {
                        element: second,
                        style: second_style,
                    },
                ],
            })
            .unwrap();

        assert_eq!(changes.to.get(), before.get() + 1);
        let sizes = project
            .session()
            .page(page)
            .unwrap()
            .elements
            .iter()
            .map(|element| match &element.kind {
                ElementKind::Text(text) => text.style.font_size,
                ElementKind::Image(_) | ElementKind::Region(_) => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(sizes, [24.0, 30.0]);
    }

    #[test]
    fn project_projection_and_errors_are_stable() {
        let mut project = memory_project();
        let page = project.visible_page().unwrap();
        let changes = project
            .apply(AppCommand::RenamePage {
                page,
                name: "Opening".into(),
            })
            .unwrap();
        let delta = project.delta(&changes).unwrap();
        assert_eq!(delta.name, "Volume 1");
        assert_eq!(delta.pages[0].name, "Opening");
        assert_eq!(
            classify_error(&failure(AppErrorCode::Busy, "busy")),
            AppErrorCode::Busy
        );
        assert_eq!(project_name(Path::new("Untitled")), "Untitled");
    }

    #[test]
    fn deleting_pages_is_one_revision_and_reconciles_the_visible_page() {
        let mut project = memory_project();
        let first = project.visible_page().unwrap();
        let mut commands = project.session().commands();
        let second = commands.add_page("second.png", png()).unwrap();
        project.apply_commands(commands).unwrap();
        let before = project.revision();

        let changes = project
            .apply(AppCommand::DeletePages {
                pages: vec![first, second],
            })
            .unwrap();
        assert_eq!(changes.to.get(), before.get() + 1);
        assert!(project.session().project().pages.is_empty());

        project.reconcile_visible_page();
        assert_eq!(project.visible_page(), None);
        assert_eq!(
            classify_error(&project.require_base(before).unwrap_err()),
            AppErrorCode::StaleRevision
        );
    }

    fn memory_project() -> Project {
        let mut session = Session::memory().unwrap();
        let mut commands = session.commands();
        commands.add_page("page.png", png()).unwrap();
        session.apply(commands).unwrap();
        Project::new(session, PathBuf::from("Volume 1.khr"))
    }

    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ]
    }
}
