//! Platform-independent ownership of one open project.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, anyhow, bail};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, Authored, Children, EntityChange, EntityId, Geometry,
    LanguageTag, Origin, PageDraft, Point, RemovePolicy, Revision, SceneChangeSet, SceneComponent,
    SceneSession, SceneSnapshot, SourceText, Translation, Typography, Visibility,
};

use crate::{
    projection::{page_summary, page_view},
    protocol::{
        AppCommand, AppErrorCode, Frame, PageSummary, PageView, ProjectDelta, ProjectHeader,
    },
};

/// Application state for one project. Scene data lives only in `SceneSession`;
/// selection of the visible page and user-facing history grouping live here.
pub struct Project {
    session: SceneSession,
    path: PathBuf,
    visible_page: Option<EntityId>,
    known_pages: HashSet<EntityId>,
    known_entity_pages: HashMap<EntityId, EntityId>,
    undo: Vec<Vec<Revision>>,
    redo: Vec<Vec<Revision>>,
}

impl Project {
    pub fn create(path: PathBuf) -> Result<Self> {
        let session = SceneSession::create(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self::new(session, path))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let session = SceneSession::open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        Ok(Self::new(session, path))
    }

    #[must_use]
    pub fn new(session: SceneSession, path: PathBuf) -> Self {
        let snapshot = session.snapshot();
        let known_pages = snapshot
            .pages()
            .map(|page| page.id())
            .collect::<HashSet<_>>();
        let known_entity_pages = entity_pages(&snapshot);
        let visible_page = snapshot.pages().next().map(|page| page.id());
        Self {
            session,
            path,
            visible_page,
            known_pages,
            known_entity_pages,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    #[must_use]
    pub const fn session(&self) -> &SceneSession {
        &self.session
    }

    pub const fn session_mut(&mut self) -> &mut SceneSession {
        &mut self.session
    }

    #[must_use]
    pub fn snapshot(&self) -> SceneSnapshot {
        self.session.snapshot()
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn visible_page(&self) -> Option<EntityId> {
        self.visible_page
    }

    pub fn show_page(&mut self, page: EntityId) -> Result<()> {
        self.snapshot().page(page)?;
        self.visible_page = Some(page);
        Ok(())
    }

    pub fn reconcile_visible_page(&mut self) {
        let snapshot = self.snapshot();
        if self
            .visible_page
            .is_some_and(|page| snapshot.page(page).is_ok())
        {
            return;
        }
        self.visible_page = snapshot.pages().next().map(|page| page.id());
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        self.snapshot().revision()
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

    pub fn apply(&mut self, command: AppCommand) -> Result<SceneChangeSet> {
        let snapshot = self.snapshot();
        let patch = match command {
            AppCommand::RenamePage { page, label } => {
                let current = snapshot.page(page)?.page()?;
                snapshot.patch(|edit| {
                    edit.set_page(page, PageDraft::new(label, current.width, current.height))
                })?
            }
            AppCommand::DeletePages { pages } => {
                let pages = unique_roots(&snapshot, pages)?;
                snapshot.patch(|edit| {
                    for page in pages {
                        edit.remove_entity(page, RemovePolicy::Cascade)?;
                    }
                    Ok(())
                })?
            }
            AppCommand::MovePage { page, index } => {
                let siblings = snapshot.pages().map(|page| page.id()).collect::<Vec<_>>();
                let at = placement(&siblings, page, index);
                snapshot.patch(|edit| edit.move_entity(page, None, at))?
            }
            AppCommand::AddText { page, frame } => {
                let geometry = geometry_from_frame(frame)?;
                snapshot.patch(|edit| {
                    let entity = edit.add_entity(page, At::End)?;
                    edit.set(entity, "default", &geometry)?;
                    edit.set_source_text(
                        entity,
                        SourceText {
                            text: Authored::user(String::new()),
                            language: None,
                        },
                    )?;
                    edit.set(
                        entity,
                        "default",
                        &Typography {
                            origin: Origin::User,
                            preferred_font: None,
                            size: None,
                            alignment: Some(koharu_scene::TextAlignment::Center),
                            writing_mode: None,
                            extensions: Default::default(),
                        },
                    )
                })?
            }
            AppCommand::SetTranslation {
                entity,
                locale,
                text,
            } => {
                let locale = LanguageTag::new(locale)?;
                snapshot.patch(|edit| match text {
                    Some(text) => edit.set_translation(
                        entity,
                        &locale,
                        Translation {
                            text: Authored::user(text),
                        },
                    ),
                    None => edit.remove::<Translation>(entity, locale.as_str()),
                })?
            }
            AppCommand::SetTypography { entities } => snapshot.patch(|edit| {
                for value in entities {
                    let mut typography = snapshot
                        .component::<Typography>(value.entity, "default")?
                        .unwrap_or(Typography {
                            origin: Origin::User,
                            preferred_font: None,
                            size: None,
                            alignment: None,
                            writing_mode: None,
                            extensions: Default::default(),
                        });
                    typography.origin = Origin::User;
                    typography.preferred_font = value.typography.preferred_font;
                    typography.size = value.typography.size;
                    typography.alignment = value.typography.alignment;
                    typography.writing_mode = value.typography.writing_mode;
                    edit.set(value.entity, "default", &typography)?;
                }
                Ok(())
            })?,
            AppCommand::SetGeometry { entities } => snapshot.patch(|edit| {
                for value in entities {
                    edit.set(
                        value.entity,
                        "default",
                        &Geometry {
                            origin: Origin::User,
                            points: value
                                .points
                                .into_iter()
                                .map(|point| Point {
                                    x: point.x,
                                    y: point.y,
                                })
                                .collect(),
                        },
                    )?;
                }
                Ok(())
            })?,
            AppCommand::SetVisibility {
                entities,
                visible,
                opacity,
            } => snapshot.patch(|edit| {
                for entity in entities {
                    let mut value = snapshot
                        .component::<Visibility>(entity, "default")?
                        .unwrap_or(Visibility {
                            origin: Origin::User,
                            visible: true,
                            opacity: 1.0,
                        });
                    if let Some(visible) = visible {
                        value.visible = visible;
                    }
                    if let Some(opacity) = opacity {
                        value.opacity = opacity;
                    }
                    value.origin = Origin::User;
                    edit.set(entity, "default", &value)?;
                }
                Ok(())
            })?,
            AppCommand::DeleteEntities { entities } => {
                let entities = unique_roots(&snapshot, entities)?;
                snapshot.patch(|edit| {
                    for entity in entities {
                        if snapshot.page(entity).is_ok() {
                            return Err(koharu_scene::Error::Invalid(
                                "delete pages with DeletePages".to_owned(),
                            ));
                        }
                        edit.remove_entity(entity, RemovePolicy::Cascade)?;
                    }
                    Ok(())
                })?
            }
            AppCommand::MoveEntity {
                entity,
                parent,
                index,
            } => {
                let siblings = snapshot.children(parent)?.collect::<Vec<_>>();
                let at = placement(&siblings, entity, index);
                snapshot.patch(|edit| edit.move_entity(entity, Some(parent), at))?
            }
            AppCommand::Synchronize
            | AppCommand::CreateProject
            | AppCommand::OpenProject
            | AppCommand::CloseProject
            | AppCommand::ImportPages
            | AppCommand::FinishTransform
            | AppCommand::Undo
            | AppCommand::Redo
            | AppCommand::RunPipeline { .. }
            | AppCommand::CancelJob { .. }
            | AppCommand::ExportPages { .. }
            | AppCommand::GetSettings
            | AppCommand::SetSettings { .. }
            | AppCommand::CollectGarbage => bail!("command is not a scene edit"),
        };
        self.commit_user_patch(patch)
    }

    pub fn set_geometries(
        &mut self,
        geometries: impl IntoIterator<Item = (EntityId, Geometry)>,
    ) -> Result<SceneChangeSet> {
        let snapshot = self.snapshot();
        let geometries = geometries.into_iter().collect::<Vec<_>>();
        let patch = snapshot.patch(|edit| {
            for (entity, mut geometry) in geometries {
                geometry.origin = Origin::User;
                edit.set(entity, "default", &geometry)?;
            }
            Ok(())
        })?;
        self.commit_user_patch(patch)
    }

    pub fn set_asset(
        &mut self,
        entity: EntityId,
        role: &str,
        bytes: impl Into<std::sync::Arc<[u8]>>,
        media_type: &str,
        metadata: AssetMetadata,
    ) -> Result<SceneChangeSet> {
        let snapshot = self.snapshot();
        let role = AssetRole::new(role)?;
        let asset = AssetInput::new(bytes, media_type, metadata);
        let patch = snapshot.patch(|edit| edit.set_asset(entity, &role, asset))?;
        self.commit_user_patch(patch)
    }

    pub fn refresh(&mut self) -> Result<SceneChangeSet> {
        Ok(self.session.refresh()?)
    }

    pub fn undo(&mut self, base: Revision) -> Result<SceneChangeSet> {
        self.require_base(base)?;
        let group = self.undo.pop().ok_or_else(|| anyhow!("nothing to undo"))?;
        let commit = match self.session.undo_many(group.iter().copied()) {
            Ok(commit) => commit,
            Err(error) => {
                self.undo.push(group);
                return Err(error.into());
            }
        };
        self.redo.push(vec![commit.revision]);
        Ok(commit.changes)
    }

    pub fn redo(&mut self, base: Revision) -> Result<SceneChangeSet> {
        self.require_base(base)?;
        let group = self.redo.pop().ok_or_else(|| anyhow!("nothing to redo"))?;
        let commit = match self.session.undo_many(group.iter().copied()) {
            Ok(commit) => commit,
            Err(error) => {
                self.redo.push(group);
                return Err(error.into());
            }
        };
        self.undo.push(vec![commit.revision]);
        Ok(commit.changes)
    }

    pub fn record_revisions(&mut self, revisions: Vec<Revision>) {
        if !revisions.is_empty() {
            self.undo.push(revisions);
            self.redo.clear();
        }
    }

    #[must_use]
    pub fn header(&self) -> ProjectHeader {
        ProjectHeader {
            id: self.session.project_id(),
            name: project_name(&self.path),
            visible_page: self.visible_page,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        }
    }

    pub fn page_summaries(&self) -> Result<Vec<PageSummary>> {
        let snapshot = self.snapshot();
        snapshot
            .pages()
            .map(|page| page_summary(&snapshot, page))
            .collect()
    }

    pub fn page_view(&self, page: EntityId, locale: Option<&LanguageTag>) -> Result<PageView> {
        page_view(&self.snapshot(), page, locale)
    }

    pub fn delta(
        &mut self,
        changes: &SceneChangeSet,
        locale: Option<&LanguageTag>,
    ) -> Result<ProjectDelta> {
        let snapshot = self.snapshot();
        let page_order = snapshot.pages().map(|page| page.id()).collect::<Vec<_>>();
        let current = page_order.iter().copied().collect::<HashSet<_>>();
        let mut deleted_pages = self
            .known_pages
            .difference(&current)
            .copied()
            .collect::<Vec<_>>();
        deleted_pages.sort_unstable();
        self.known_pages = current;

        let mut affected_pages = HashSet::new();
        if changes.pages_changed {
            affected_pages.extend(page_order.iter().copied());
        } else {
            for change in &changes.components {
                if let Some(page) = owning_page(&snapshot, change.entity)
                    .or_else(|| self.known_entity_pages.get(&change.entity).copied())
                {
                    affected_pages.insert(page);
                }
            }
            for change in &changes.entities {
                let entity = match change {
                    EntityChange::Inserted(entity) | EntityChange::Removed(entity) => *entity,
                };
                if let Some(page) = owning_page(&snapshot, entity)
                    .or_else(|| self.known_entity_pages.get(&entity).copied())
                {
                    affected_pages.insert(page);
                }
            }
        }
        let pages = snapshot
            .pages()
            .filter(|page| affected_pages.contains(&page.id()))
            .map(|page| page_summary(&snapshot, page))
            .collect::<Result<Vec<_>>>()?;
        if !changes.entities.is_empty()
            || changes
                .components
                .iter()
                .any(|change| change.kind == Children::KIND)
        {
            self.known_entity_pages
                .retain(|_, page| !affected_pages.contains(page));
            for page in snapshot
                .pages()
                .filter(|page| affected_pages.contains(&page.id()))
            {
                self.known_entity_pages.insert(page.id(), page.id());
                self.known_entity_pages.extend(
                    snapshot
                        .descendants(page.id())?
                        .map(|entity| (entity.id(), page.id())),
                );
            }
        }
        let visible_page = self
            .visible_page
            .filter(|page| active_page_changed(&snapshot, *page, changes))
            .map(|page| page_view(&snapshot, page, locale))
            .transpose()?;
        Ok(ProjectDelta {
            from: changes.from,
            revision: changes.to,
            name: project_name(&self.path),
            page_order,
            pages,
            deleted_pages,
            visible_page,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        })
    }

    #[must_use]
    pub fn history_delta(&self) -> ProjectDelta {
        ProjectDelta {
            from: self.revision(),
            revision: self.revision(),
            name: project_name(&self.path),
            page_order: self.snapshot().pages().map(|page| page.id()).collect(),
            pages: Vec::new(),
            deleted_pages: Vec::new(),
            visible_page: None,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        }
    }

    fn commit_user_patch(&mut self, patch: koharu_scene::ScenePatch) -> Result<SceneChangeSet> {
        let commit = self.session.commit(patch)?;
        if commit.changes.to != commit.changes.from {
            self.undo.push(vec![commit.revision]);
            self.redo.clear();
        }
        Ok(commit.changes)
    }
}

fn active_page_changed(snapshot: &SceneSnapshot, page: EntityId, changes: &SceneChangeSet) -> bool {
    if changes.pages_changed || !changes.relations.is_empty() {
        return true;
    }
    let members = snapshot
        .subtree(page)
        .map(|entities| entities.map(|entity| entity.id()).collect::<HashSet<_>>())
        .unwrap_or_default();
    changes
        .components
        .iter()
        .any(|change| members.contains(&change.entity))
        || changes.entities.iter().any(|change| match change {
            EntityChange::Inserted(entity) => members.contains(entity),
            EntityChange::Removed(_) => true,
        })
}

fn entity_pages(snapshot: &SceneSnapshot) -> HashMap<EntityId, EntityId> {
    let mut owners = HashMap::new();
    for page in snapshot.pages() {
        owners.insert(page.id(), page.id());
        if let Ok(descendants) = snapshot.descendants(page.id()) {
            owners.extend(descendants.map(|entity| (entity.id(), page.id())));
        }
    }
    owners
}

fn owning_page(snapshot: &SceneSnapshot, entity: EntityId) -> Option<EntityId> {
    let mut current = entity;
    loop {
        if snapshot.page(current).is_ok() {
            return Some(current);
        }
        current = snapshot.parent(current).ok().flatten()?;
    }
}

fn placement(siblings: &[EntityId], moving: EntityId, index: usize) -> At {
    let remaining = siblings
        .iter()
        .copied()
        .filter(|entity| *entity != moving)
        .collect::<Vec<_>>();
    remaining.get(index).copied().map_or(At::End, At::Before)
}

fn unique_roots(snapshot: &SceneSnapshot, entities: Vec<EntityId>) -> Result<Vec<EntityId>> {
    let selected = entities.into_iter().collect::<HashSet<_>>();
    let mut roots = Vec::new();
    for entity in selected.iter().copied() {
        let mut parent = snapshot.parent(entity)?;
        let mut nested = false;
        while let Some(value) = parent {
            if selected.contains(&value) {
                nested = true;
                break;
            }
            parent = snapshot.parent(value)?;
        }
        if !nested {
            roots.push(entity);
        }
    }
    roots.sort_unstable();
    Ok(roots)
}

fn geometry_from_frame(frame: Frame) -> Result<Geometry> {
    if !frame.x.is_finite()
        || !frame.y.is_finite()
        || !frame.width.is_finite()
        || !frame.height.is_finite()
        || !frame.angle_degrees.is_finite()
        || frame.width <= 0.0
        || frame.height <= 0.0
    {
        bail!("frame must contain finite coordinates and positive dimensions");
    }
    let center_x = f64::from(frame.x + frame.width * 0.5);
    let center_y = f64::from(frame.y + frame.height * 0.5);
    let half_width = f64::from(frame.width) * 0.5;
    let half_height = f64::from(frame.height) * 0.5;
    let angle = f64::from(frame.angle_degrees).to_radians();
    let (sin, cos) = angle.sin_cos();
    Ok(Geometry {
        origin: Origin::User,
        points: [
            (-half_width, -half_height),
            (half_width, -half_height),
            (half_width, half_height),
            (-half_width, half_height),
        ]
        .map(|(x, y)| Point {
            x: center_x + x * cos - y * sin,
            y: center_y + x * sin + y * cos,
        })
        .into(),
    })
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
            koharu_scene::Error::EntityNotFound(_) | koharu_scene::Error::RelationNotFound(_) => {
                AppErrorCode::NotFound
            }
            koharu_scene::Error::Invalid(_)
            | koharu_scene::Error::MultipleParents(_)
            | koharu_scene::Error::HierarchyCycle
            | koharu_scene::Error::NonEmptyEntity(_)
            | koharu_scene::Error::IncidentRelations(_)
            | koharu_scene::Error::Authorship(_)
            | koharu_scene::Error::ReferenceMismatch(_) => AppErrorCode::InvalidInput,
            koharu_scene::Error::Storage(_)
            | koharu_scene::Error::Codec(_)
            | koharu_scene::Error::UnsupportedComponent { .. } => AppErrorCode::IoFailed,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use koharu_scene::{AssetMetadata, AssetRole, At, PageDraft};

    use super::*;

    #[test]
    fn scene_edits_and_grouped_history_are_headless() {
        let mut project = memory_project();
        let page = project.visible_page().unwrap();
        let changes = project
            .apply(AppCommand::AddText {
                page,
                frame: Frame {
                    x: 1.0,
                    y: 2.0,
                    width: 30.0,
                    height: 40.0,
                    angle_degrees: 0.0,
                },
            })
            .unwrap();
        assert_eq!(project.snapshot().descendants(page).unwrap().count(), 1);
        assert!(project.header().can_undo);

        let undone = project.undo(changes.to).unwrap();
        assert_eq!(project.snapshot().descendants(page).unwrap().count(), 0);
        assert!(project.header().can_redo);

        project.redo(undone.to).unwrap();
        assert_eq!(project.snapshot().descendants(page).unwrap().count(), 1);
    }

    #[test]
    fn translations_are_stored_in_explicit_locale_slots() {
        let mut project = memory_project();
        let page = project.visible_page().unwrap();
        project
            .apply(AppCommand::AddText {
                page,
                frame: Frame {
                    width: 20.0,
                    height: 20.0,
                    ..Frame::default()
                },
            })
            .unwrap();
        let entity = project
            .snapshot()
            .descendants(page)
            .unwrap()
            .next()
            .unwrap()
            .id();
        project
            .apply(AppCommand::SetTranslation {
                entity,
                locale: "ar-EG".into(),
                text: Some("مرحبا".into()),
            })
            .unwrap();
        let translation = project
            .snapshot()
            .component::<Translation>(entity, "ar-EG")
            .unwrap()
            .unwrap();
        assert_eq!(translation.text.value, "مرحبا");
    }

    #[test]
    fn deleting_pages_is_atomic_and_reconciles_selection() {
        let mut project = memory_project();
        let first = project.visible_page().unwrap();
        let snapshot = project.snapshot();
        let mut second = None;
        let patch = snapshot
            .patch(|edit| {
                second = Some(edit.add_page(PageDraft::new("second", 1.0, 1.0), At::End)?);
                Ok(())
            })
            .unwrap();
        let commit = project.session_mut().commit(patch).unwrap();
        project.record_revisions(vec![commit.revision]);
        let second = second.unwrap();

        project
            .apply(AppCommand::DeletePages {
                pages: vec![first, second],
            })
            .unwrap();
        project.reconcile_visible_page();
        assert_eq!(project.visible_page(), None);
        assert_eq!(project.snapshot().pages().count(), 0);
    }

    #[test]
    fn project_projection_is_capability_based() {
        let project = memory_project();
        let page = project.visible_page().unwrap();
        let summary = project.page_summaries().unwrap().remove(0);
        assert_eq!(summary.id, page);
        assert!(summary.source.is_some());
        assert_eq!(project_name(Path::new("Untitled")), "Untitled");
    }

    #[test]
    fn deltas_update_each_affected_page_once() {
        let mut project = memory_project();
        let page = project.visible_page().unwrap();
        let changes = project
            .apply(AppCommand::AddText {
                page,
                frame: Frame {
                    width: 20.0,
                    height: 20.0,
                    ..Frame::default()
                },
            })
            .unwrap();
        let delta = project.delta(&changes, None).unwrap();
        assert_eq!(delta.pages.len(), 1);
        assert_eq!(delta.pages[0].entities, 1);

        let entity = project
            .snapshot()
            .descendants(page)
            .unwrap()
            .next()
            .unwrap()
            .id();
        let changes = project
            .apply(AppCommand::DeleteEntities {
                entities: vec![entity],
            })
            .unwrap();
        let delta = project.delta(&changes, None).unwrap();
        assert_eq!(delta.pages.len(), 1);
        assert_eq!(delta.pages[0].entities, 0);
    }

    #[test]
    fn typography_edits_preserve_extension_data() {
        let mut project = memory_project();
        let page = project.visible_page().unwrap();
        project
            .apply(AppCommand::AddText {
                page,
                frame: Frame {
                    width: 20.0,
                    height: 20.0,
                    ..Frame::default()
                },
            })
            .unwrap();
        let entity = project
            .snapshot()
            .descendants(page)
            .unwrap()
            .next()
            .unwrap()
            .id();
        let snapshot = project.snapshot();
        let mut typography = snapshot
            .component::<Typography>(entity, "default")
            .unwrap()
            .unwrap();
        typography
            .extensions
            .insert("example.vendor.setting".into(), "preserved".into());
        let patch = snapshot
            .patch(|edit| edit.set(entity, "default", &typography))
            .unwrap();
        project.session_mut().commit(patch).unwrap();

        project
            .apply(AppCommand::SetTypography {
                entities: vec![crate::protocol::EntityTypography {
                    entity,
                    typography: crate::protocol::TypographyIntent {
                        preferred_font: Some("Noto Sans".into()),
                        size: Some(18.0),
                        alignment: Some(koharu_scene::TextAlignment::Center),
                        writing_mode: Some(koharu_scene::WritingMode::Horizontal),
                    },
                }],
            })
            .unwrap();
        let typography = project
            .snapshot()
            .component::<Typography>(entity, "default")
            .unwrap()
            .unwrap();
        assert_eq!(typography.extensions["example.vendor.setting"], "preserved");
    }

    fn memory_project() -> Project {
        let mut session = SceneSession::memory().unwrap();
        let mut page = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let id = edit.add_page(PageDraft::new("page", 1.0, 1.0), At::End)?;
                edit.set_asset(
                    id,
                    &AssetRole::new("source")?,
                    AssetInput::new(
                        Arc::<[u8]>::from(png()),
                        "image/png",
                        AssetMetadata {
                            width: Some(1),
                            height: Some(1),
                            attributes: Default::default(),
                        },
                    ),
                )?;
                page = Some(id);
                Ok(())
            })
            .unwrap();
        session.commit(patch).unwrap();
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
