use anyhow::Result;
use koharu_core::{
    AppState, Document, DocumentSummary, ProjectPageStages, ProjectStageState, ProjectStageStatus,
    ProjectSummary,
};
use once_cell::sync::Lazy;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

use crate::projects;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum ChangedField {
    #[strum(serialize = "name")]
    Name,
    #[strum(serialize = "textBlocks")]
    TextBlocks,
    #[strum(serialize = "segment")]
    Segment,
    #[strum(serialize = "brushLayer")]
    BrushLayer,
    #[strum(serialize = "inpainted")]
    Inpainted,
    #[strum(serialize = "rendered")]
    Rendered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStage {
    Detect,
    Ocr,
    Inpaint,
    Translate,
    Render,
}

#[derive(Debug, Clone)]
pub enum StateEvent {
    DocumentsChanged,
    DocumentChanged {
        document_id: String,
        revision: u64,
        changed: Vec<String>,
    },
}

static STATE_TX: Lazy<broadcast::Sender<StateEvent>> = Lazy::new(|| broadcast::channel(256).0);

pub fn subscribe() -> broadcast::Receiver<StateEvent> {
    STATE_TX.subscribe()
}

fn emit(event: StateEvent) {
    let _ = STATE_TX.send(event);
}

fn serialize_changed_fields(changed: &[ChangedField]) -> Vec<String> {
    changed.iter().map(ToString::to_string).collect()
}

fn current_project(
    state: &koharu_core::State,
) -> Result<&koharu_core::ProjectSessionState> {
    state
        .current_project
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No project is currently open"))
}

fn current_project_mut(
    state: &mut koharu_core::State,
) -> Result<&mut koharu_core::ProjectSessionState> {
    state
        .current_project
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("No project is currently open"))
}

fn current_project_snapshot(
    state: &koharu_core::State,
) -> Result<koharu_core::ProjectSessionState> {
    let project = current_project(state)?;
    Ok(koharu_core::ProjectSessionState {
        root: project.root.clone(),
        summary: project.summary.clone(),
        pages: project.pages.clone(),
        current_document_id: project.current_document_id.clone(),
        loaded_documents: Default::default(),
    })
}

fn page_id_at_index(project: &koharu_core::ProjectSessionState, index: usize) -> Result<String> {
    project
        .pages
        .get(index)
        .map(|page| page.summary.id.clone())
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))
}

fn stage_mut<'a>(
    stages: &'a mut ProjectPageStages,
    stage: ProjectStage,
) -> &'a mut ProjectStageState {
    match stage {
        ProjectStage::Detect => &mut stages.detect,
        ProjectStage::Ocr => &mut stages.ocr,
        ProjectStage::Inpaint => &mut stages.inpaint,
        ProjectStage::Translate => &mut stages.translate,
        ProjectStage::Render => &mut stages.render,
    }
}

fn invalidate_document(document: &mut Document, changed: &[ChangedField], stages: &mut ProjectPageStages) {
    let changed_text_blocks = changed.contains(&ChangedField::TextBlocks);
    let changed_segment = changed.contains(&ChangedField::Segment);
    let changed_brush = changed.contains(&ChangedField::BrushLayer);
    let changed_inpainted = changed.contains(&ChangedField::Inpainted);
    let changed_rendered = changed.contains(&ChangedField::Rendered);

    if changed_segment {
        document.inpainted = None;
        document.rendered = None;
        stages.inpaint.status = ProjectStageStatus::Stale;
        stages.inpaint.error = None;
        stages.render.status = ProjectStageStatus::Stale;
        stages.render.error = None;
    }

    if changed_brush {
        document.rendered = None;
        stages.render.status = ProjectStageStatus::Stale;
        stages.render.error = None;
    }

    if changed_text_blocks && !changed_rendered {
        document.rendered = None;
        stages.render.status = ProjectStageStatus::Stale;
        stages.render.error = None;
    }

    if changed_inpainted {
        stages.inpaint.status = ProjectStageStatus::Ready;
        stages.inpaint.error = None;
        document.rendered = None;
        stages.render.status = ProjectStageStatus::Stale;
        stages.render.error = None;
    }

    if changed_rendered {
        stages.render.status = ProjectStageStatus::Ready;
        stages.render.error = None;
    }
}

fn normalize_stage_success(stages: &mut ProjectPageStages, stage: ProjectStage) {
    let target = stage_mut(stages, stage);
    target.status = ProjectStageStatus::Ready;
    target.error = None;

    match stage {
        ProjectStage::Detect => {
            stages.ocr.status = ProjectStageStatus::Stale;
            stages.ocr.error = None;
            stages.translate.status = ProjectStageStatus::Stale;
            stages.translate.error = None;
            stages.inpaint.status = ProjectStageStatus::Stale;
            stages.inpaint.error = None;
            stages.render.status = ProjectStageStatus::Stale;
            stages.render.error = None;
        }
        ProjectStage::Ocr => {
            stages.translate.status = ProjectStageStatus::Stale;
            stages.translate.error = None;
            stages.render.status = ProjectStageStatus::Stale;
            stages.render.error = None;
        }
        ProjectStage::Inpaint => {
            stages.render.status = ProjectStageStatus::Stale;
            stages.render.error = None;
        }
        ProjectStage::Translate => {
            stages.render.status = ProjectStageStatus::Stale;
            stages.render.error = None;
        }
        ProjectStage::Render => {}
    }
}

fn touch_project_summary(project: &mut koharu_core::ProjectSessionState) {
    project.summary.updated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    project.summary.current_document_id = project.current_document_id.clone();
}

pub async fn restore_last_project(state: &AppState) -> Result<bool> {
    let maybe_session = projects::load_last_project()?;
    let restored = maybe_session.is_some();
    if restored {
        let mut guard = state.write().await;
        guard.current_project = maybe_session;
        drop(guard);
        emit(StateEvent::DocumentsChanged);
    }
    Ok(restored)
}

pub async fn list_doc_summaries(state: &AppState) -> Vec<DocumentSummary> {
    state
        .read()
        .await
        .current_project
        .as_ref()
        .map(|project| {
            project
                .pages
                .iter()
                .map(|page| DocumentSummary::from(&page.summary))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn current_project_summary(state: &AppState) -> Option<ProjectSummary> {
    state
        .read()
        .await
        .current_project
        .as_ref()
        .map(|project| project.summary.clone())
}

pub async fn current_document_id(state: &AppState) -> Option<String> {
    state
        .read()
        .await
        .current_project
        .as_ref()
        .and_then(|project| project.current_document_id.clone())
}

pub async fn read_doc(state: &AppState, index: usize) -> Result<Document> {
    let mut project = {
        let guard = state.read().await;
        current_project_snapshot(&guard)?
    };
    let document_id = page_id_at_index(&project, index)?;
    projects::load_document(&mut project, &document_id)
}

pub async fn list_docs(state: &AppState) -> Vec<Document> {
    let mut project = {
        let guard = state.read().await;
        let Ok(project) = current_project_snapshot(&guard) else {
            return Vec::new();
        };
        project
    };

    let ids = project
        .pages
        .iter()
        .map(|page| page.summary.id.clone())
        .collect::<Vec<_>>();
    let mut documents = Vec::with_capacity(ids.len());
    for document_id in ids {
        match projects::load_document(&mut project, &document_id) {
            Ok(document) => documents.push(document),
            Err(err) => tracing::warn!(?err, document_id, "failed to load project page"),
        }
    }
    documents
}

pub async fn read_thumbnail(state: &AppState, index: usize) -> Result<Vec<u8>> {
    let project = {
        let guard = state.read().await;
        current_project_snapshot(&guard)?
    };
    let document_id = page_id_at_index(&project, index)?;
    projects::read_thumbnail(&project, &document_id)
}

pub async fn doc_count(state: &AppState) -> usize {
    state
        .read()
        .await
        .current_project
        .as_ref()
        .map(|project| project.pages.len())
        .unwrap_or(0)
}

pub async fn find_doc_index(state: &AppState, document_id: &str) -> Result<usize> {
    let guard = state.read().await;
    let project = current_project(&guard)?;
    project
        .pages
        .iter()
        .position(|page| page.summary.id == document_id)
        .ok_or_else(|| anyhow::anyhow!("Document not found: {document_id}"))
}

pub async fn replace_docs_from_files(state: &AppState, files: Vec<koharu_core::FileEntry>) -> Result<usize> {
    let session = projects::create_project(files)?;
    let count = session.pages.len();
    let mut guard = state.write().await;
    guard.current_project = Some(session);
    drop(guard);
    emit(StateEvent::DocumentsChanged);
    Ok(count)
}

pub async fn append_docs_from_files(state: &AppState, files: Vec<koharu_core::FileEntry>) -> Result<usize> {
    let mut guard = state.write().await;
    let count = match guard.current_project.as_mut() {
        Some(project) => projects::append_files(project, files)?,
        None => {
            let session = projects::create_project(files)?;
            let count = session.pages.len();
            guard.current_project = Some(session);
            count
        }
    };
    drop(guard);
    emit(StateEvent::DocumentsChanged);
    Ok(count)
}

pub async fn open_project(state: &AppState, project_id: &str) -> Result<usize> {
    let session = projects::open_project(project_id)?;
    let count = session.pages.len();
    let mut guard = state.write().await;
    guard.current_project = Some(session);
    drop(guard);
    emit(StateEvent::DocumentsChanged);
    Ok(count)
}

pub async fn list_projects(recent_only: bool) -> Result<Vec<ProjectSummary>> {
    projects::list_projects(recent_only)
}

pub async fn save_current_project(state: &AppState) -> Result<Option<ProjectSummary>> {
    let mut guard = state.write().await;
    let Some(project) = guard.current_project.as_mut() else {
        return Ok(None);
    };
    touch_project_summary(project);
    projects::save_session(project)?;
    Ok(Some(project.summary.clone()))
}

pub async fn set_current_document(state: &AppState, document_id: Option<String>) -> Result<()> {
    let mut guard = state.write().await;
    let project = current_project_mut(&mut guard)?;
    projects::set_current_document(project, document_id)?;
    Ok(())
}

pub async fn update_doc(
    state: &AppState,
    index: usize,
    mut document: Document,
    changed: &[ChangedField],
) -> Result<()> {
    document.prepare_for_store();
    document.bump_revision();
    let document_id = document.id.clone();
    let revision = document.revision;

    let mut guard = state.write().await;
    let project = current_project_mut(&mut guard)?;
    let page = project
        .pages
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    invalidate_document(&mut document, changed, &mut page.summary.stages);
    projects::save_document(project, &document)?;
    drop(guard);

    emit(StateEvent::DocumentChanged {
        document_id,
        revision,
        changed: serialize_changed_fields(changed),
    });
    Ok(())
}

pub async fn mutate_doc<T, F>(
    state: &AppState,
    index: usize,
    changed: &[ChangedField],
    mutator: F,
) -> Result<T>
where
    F: FnOnce(&mut Document) -> Result<T>,
{
    let mut guard = state.write().await;
    let project = current_project_mut(&mut guard)?;
    let document_id = page_id_at_index(project, index)?;
    let page_stages = project
        .pages
        .get(index)
        .map(|page| page.summary.stages.clone())
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    let mut document = projects::load_document(project, &document_id)?;
    let result = mutator(&mut document)?;
    document.prepare_for_store();
    document.bump_revision();
    let revision = document.revision;

    if let Some(page) = project.pages.get_mut(index) {
        page.summary.stages = page_stages;
        invalidate_document(&mut document, changed, &mut page.summary.stages);
    }
    projects::save_document(project, &document)?;
    drop(guard);

    emit(StateEvent::DocumentChanged {
        document_id,
        revision,
        changed: serialize_changed_fields(changed),
    });
    Ok(result)
}

pub async fn mark_stage_success(
    state: &AppState,
    index: usize,
    stage: ProjectStage,
) -> Result<()> {
    let mut guard = state.write().await;
    let project = current_project_mut(&mut guard)?;
    let page = project
        .pages
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    normalize_stage_success(&mut page.summary.stages, stage);
    touch_project_summary(project);
    projects::save_session(project)?;
    drop(guard);
    emit(StateEvent::DocumentsChanged);
    Ok(())
}

pub async fn mark_stage_failure(
    state: &AppState,
    index: usize,
    stage: ProjectStage,
    error: impl Into<String>,
) -> Result<()> {
    let mut guard = state.write().await;
    let project = current_project_mut(&mut guard)?;
    let page = project
        .pages
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    let target = stage_mut(&mut page.summary.stages, stage);
    target.status = ProjectStageStatus::Failed;
    target.error = Some(error.into());
    touch_project_summary(project);
    projects::save_session(project)?;
    drop(guard);
    emit(StateEvent::DocumentsChanged);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use koharu_core::{FileEntry, State};
    use std::{io::Cursor, sync::Arc, thread, time::Duration};
    use tokio::sync::RwLock;

    fn sample_file_entry(name: &str) -> FileEntry {
        let image = RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]));
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("png encode should work");
        FileEntry {
            name: name.to_string(),
            data: cursor.into_inner(),
        }
    }

    #[test]
    fn stage_failures_persist_and_touch_updated_at() {
        crate::projects::with_test_app_data_root(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime should build");

            runtime.block_on(async {
                let session = crate::projects::create_project(vec![sample_file_entry("page.png")])
                    .expect("project should exist");
                let project_id = session.summary.id.clone();
                let document_id = session
                    .current_document_id
                    .clone()
                    .expect("project should have a current page");
                let state: AppState = Arc::new(RwLock::new(State {
                    current_project: Some(session),
                }));

                let before = current_project_summary(&state)
                    .await
                    .expect("project summary should exist")
                    .updated_at_ms;
                thread::sleep(Duration::from_millis(2));
                mark_stage_failure(&state, 0, ProjectStage::Render, "cuda failed")
                    .await
                    .expect("stage failure should save");

                let after = current_project_summary(&state)
                    .await
                    .expect("project summary should exist")
                    .updated_at_ms;
                assert!(after > before);

                let reopened =
                    crate::projects::open_project(&project_id).expect("project should reopen");
                let page = reopened
                    .pages
                    .iter()
                    .find(|page| page.summary.id == document_id)
                    .expect("saved page should exist");
                assert_eq!(page.summary.stages.render.status, ProjectStageStatus::Failed);
                assert_eq!(page.summary.stages.render.error.as_deref(), Some("cuda failed"));
            });
        });
    }

    #[test]
    fn stage_success_persists_normalized_statuses() {
        crate::projects::with_test_app_data_root(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime should build");

            runtime.block_on(async {
                let session = crate::projects::create_project(vec![sample_file_entry("page.png")])
                    .expect("project should exist");
                let project_id = session.summary.id.clone();
                let document_id = session
                    .current_document_id
                    .clone()
                    .expect("project should have a current page");
                let state: AppState = Arc::new(RwLock::new(State {
                    current_project: Some(session),
                }));

                mark_stage_success(&state, 0, ProjectStage::Detect)
                    .await
                    .expect("stage success should save");

                let reopened =
                    crate::projects::open_project(&project_id).expect("project should reopen");
                let page = reopened
                    .pages
                    .iter()
                    .find(|page| page.summary.id == document_id)
                    .expect("saved page should exist");
                assert_eq!(page.summary.stages.detect.status, ProjectStageStatus::Ready);
                assert_eq!(page.summary.stages.ocr.status, ProjectStageStatus::Stale);
                assert_eq!(page.summary.stages.render.status, ProjectStageStatus::Stale);
            });
        });
    }
}
