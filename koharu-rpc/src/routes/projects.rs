//! Project lifecycle routes. Every project lives under the managed
//! `{data.path}/projects/` directory; clients never supply filesystem
//! paths. A project's `id` is the `.khrproj/` directory basename.
//!
//! - `GET    /projects` — list managed projects
//! - `POST   /projects` — create a new project (`{name}`), server allocates path
//! - `POST   /projects/import` — extract a `.khr` archive into a fresh dir + open
//! - `PUT    /projects/current` — open a managed project by `id`
//! - `DELETE /projects/current` — close current session
//! - `POST /projects/current/export` — export current; returns bytes
//! - `POST /projects/current/import-translations` — read a JSON document
//!   of per-page texts and write them to the translation slot of the
//!   matching text nodes. Defensive JSON parse.

use anyhow::Context as _;
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use koharu_app::projects as project_dirs;
use koharu_core::{
    ImageRole, NodeDataPatch, NodeId, NodePatch, Op, PageId, ProjectSummary, TextDataPatch,
};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(list_projects))
        .routes(routes!(create_project))
        .routes(routes!(import_project))
        .routes(routes!(put_current_project))
        .routes(routes!(delete_current_project))
        .routes(routes!(delete_project))
        .routes(routes!(export_current_project))
        .routes(routes!(import_translations))
}

// ---------------------------------------------------------------------------
// GET /projects
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListProjectsResponse {
    pub projects: Vec<ProjectSummary>,
}

#[utoipa::path(
    get,
    path = "/projects",
    responses((status = 200, body = ListProjectsResponse))
)]
async fn list_projects(State(app): State<AppState>) -> ApiResult<Json<ListProjectsResponse>> {
    let config = (**app.config.load()).clone();
    let projects = project_dirs::list_projects(&config).map_err(ApiError::internal)?;
    Ok(Json(ListProjectsResponse { projects }))
}

// ---------------------------------------------------------------------------
// POST /projects — create a new project from a display name
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
}

#[utoipa::path(
    post,
    path = "/projects",
    request_body = CreateProjectRequest,
    responses((status = 200, body = ProjectSummary))
)]
async fn create_project(
    State(app): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> ApiResult<Json<ProjectSummary>> {
    let trimmed = req.name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request("name must not be empty"));
    }
    let config = (**app.config.load()).clone();
    let path = project_dirs::allocate_named(&config, trimmed).map_err(ApiError::internal)?;
    // `allocate_named` atomically created the directory so concurrent
    // callers can't collide. Session::create wants an empty-or-missing dir
    // and writes the scaffold — remove so it can populate.
    std::fs::remove_dir(path.as_std_path())
        .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?;
    let session = app
        .open_project(path, Some(trimmed.to_string()))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(koharu_app::app::project_summary(&session)))
}

// ---------------------------------------------------------------------------
// PUT /projects/current — open a managed project by id
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectRequest {
    /// `.khrproj/` directory basename (no extension). Must exist under the
    /// managed projects directory.
    pub id: String,
}

#[utoipa::path(
    put,
    path = "/projects/current",
    request_body = OpenProjectRequest,
    responses((status = 200, body = ProjectSummary))
)]
async fn put_current_project(
    State(app): State<AppState>,
    Json(req): Json<OpenProjectRequest>,
) -> ApiResult<Json<ProjectSummary>> {
    let config = (**app.config.load()).clone();
    let path = project_dirs::project_path(&config, &req.id)
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    if !path.exists() {
        return Err(ApiError::not_found(format!("project {}", req.id)));
    }
    let session = app
        .open_project(path, None)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(koharu_app::app::project_summary(&session)))
}

#[utoipa::path(delete, path = "/projects/current", responses((status = 204)))]
async fn delete_current_project(State(app): State<AppState>) -> ApiResult<axum::http::StatusCode> {
    app.close_project().await.map_err(ApiError::internal)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// DELETE /projects/{id} — delete a managed project recursively
// ---------------------------------------------------------------------------

#[utoipa::path(
    delete,
    path = "/projects/{id}",
    params(
        ("id" = String, Path, description = "Project ID to delete")
    ),
    responses(
        (status = 204, description = "Project successfully deleted"),
        (status = 400, description = "Invalid project ID"),
        (status = 404, description = "Project not found"),
        (status = 500, description = "Internal filesystem error")
    )
)]
async fn delete_project(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    let config = (**app.config.load()).clone();
    let path = project_dirs::project_path(&config, &id)
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;

    if !path.exists() {
        return Err(ApiError::not_found(format!("project {}", id)));
    }

    // If the active session is the project we are deleting, close it first to release lock files
    if app
        .current_session()
        .is_some_and(|session| session.dir == path)
    {
        app.close_project().await.map_err(ApiError::internal)?;
    }

    // Recursively delete the project directory from disk
    tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(path.as_std_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    })
    .await
    .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
    .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// POST /projects/import — extract an archive into a fresh allocated dir
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/projects/import",
    request_body(content_type = "application/zip"),
    responses((status = 200, body = ProjectSummary))
)]
async fn import_project(
    State(app): State<AppState>,
    body: Bytes,
) -> ApiResult<Json<ProjectSummary>> {
    if body.is_empty() {
        return Err(ApiError::bad_request("empty archive body"));
    }
    let config = (**app.config.load()).clone();
    let dest =
        project_dirs::allocate_imported(&config, Some("imported")).map_err(ApiError::internal)?;
    // Atomic-created dir must be removed so `import_khr_bytes` can do its
    // own exists-check + populate.
    std::fs::remove_dir(dest.as_std_path())
        .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?;

    let body_vec = body.to_vec();
    let dest_c = dest.clone();
    tokio::task::spawn_blocking(move || koharu_app::archive::import_khr_bytes(&body_vec, &dest_c))
        .await
        .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
        .map_err(ApiError::internal)?;

    let session = app
        .open_project(dest, None)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(koharu_app::app::project_summary(&session)))
}

// ---------------------------------------------------------------------------
// Export — returns bytes (zip when the format produces >1 file)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportProjectRequest {
    pub format: ExportFormat,
    /// Optional subset of pages; defaults to every page.
    #[serde(default)]
    pub pages: Option<Vec<PageId>>,
    /// Optional global font override (from UI preferences).
    #[serde(default)]
    pub default_font: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Whole project as a `.khr` archive (always a single zip).
    Khr,
    /// One `.psd` per page.
    Psd,
    /// One `.png` per page (the Rendered layer).
    Rendered,
    /// One `.png` per page (the Inpainted layer).
    Inpainted,
    /// Single combined JSON with OCR'd source text for all pages.
    SourceTexts,
    /// Single combined JSON with the current translation text for all pages.
    Translations,
}

#[utoipa::path(
    post,
    path = "/projects/current/export",
    request_body = ExportProjectRequest,
    responses((
        status = 200,
        content_type = "application/octet-stream",
        description = "Export bytes. Content-Type is `application/zip` when the format produces multiple files."
    ))
)]
async fn export_current_project(
    State(app): State<AppState>,
    Json(req): Json<ExportProjectRequest>,
) -> ApiResult<Response> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;

    let s_for_compact = session.clone();
    tokio::task::spawn_blocking(move || s_for_compact.compact())
        .await
        .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
        .map_err(ApiError::internal)?;

    let project_name = session.scene.read().project.name.clone();

    match req.format {
        ExportFormat::Khr => {
            let src = session.dir.clone();
            let bytes =
                tokio::task::spawn_blocking(move || koharu_app::archive::export_khr_bytes(&src))
                    .await
                    .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
                    .map_err(ApiError::internal)?;
            Ok(bytes_response(
                bytes,
                &sanitize(&project_name, "project"),
                "khr",
                "application/octet-stream",
            ))
        }
        ExportFormat::Psd => {
            let page_ids = resolve_page_ids(&session, req.pages.as_deref())?;
            if page_ids.is_empty() {
                return Err(ApiError::bad_request("no pages in selection"));
            }
            let session_c = session.clone();
            let page_ids_c = page_ids.clone();
            let renderer_c = app.renderer.clone();
            let default_font_c = req.default_font.clone();
            let files = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                let mut out = Vec::with_capacity(page_ids_c.len());
                for (i, id) in page_ids_c.iter().enumerate() {
                    let bytes = crate::psd_export::psd_bytes_for_page(
                        &session_c,
                        &renderer_c,
                        default_font_c.clone(),
                        *id,
                    )?;
                    out.push((format!("page-{:03}-{id}.psd", i + 1), bytes));
                }
                Ok(out)
            })
            .await
            .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
            .map_err(ApiError::internal)?;
            Ok(files_to_response(files, &project_name, "psd")?)
        }
        ExportFormat::Rendered => {
            export_image_role(
                &session,
                req.pages.as_deref(),
                ImageRole::Rendered,
                &project_name,
            )
            .await
        }
        ExportFormat::Inpainted => {
            export_image_role(
                &session,
                req.pages.as_deref(),
                ImageRole::Inpainted,
                &project_name,
            )
            .await
        }
        ExportFormat::SourceTexts => {
            export_texts(
                &session,
                req.pages.as_deref(),
                &project_name,
                "source-texts",
                TextSlot::Source,
            )
            .await
        }
        ExportFormat::Translations => {
            export_texts(
                &session,
                req.pages.as_deref(),
                &project_name,
                "translations",
                TextSlot::Translation,
            )
            .await
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct TextsPage {
    /// 1-indexed page number (matches array position in `pages`).
    page: usize,
    texts: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct TextsExport {
    pages: Vec<TextsPage>,
}

/// Which field of a Text node to read on export. Both exports share the
/// same `{ pages: [{ page, texts: [...] }] }` shape; the only difference
/// is which string lands in `texts[]`. Symmetric with `import-translations`.
#[derive(Debug, Clone, Copy)]
enum TextSlot {
    Source,
    Translation,
}

fn text_node_text(data: &koharu_core::scene::TextData, slot: TextSlot) -> Option<&str> {
    let raw = match slot {
        TextSlot::Source => data.text.as_deref(),
        TextSlot::Translation => data.translation.as_deref(),
    };
    raw.map(str::trim).filter(|s| !s.is_empty())
}

async fn export_texts(
    session: &std::sync::Arc<koharu_app::ProjectSession>,
    pages: Option<&[PageId]>,
    project_name: &str,
    filename_suffix: &str,
    slot: TextSlot,
) -> ApiResult<Response> {
    let page_ids = resolve_page_ids(session, pages)?;
    if page_ids.is_empty() {
        return Err(ApiError::bad_request("no pages in selection"));
    }
    let session_c = session.clone();
    let page_ids_c = page_ids.clone();
    let payload = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let scene = session_c.scene.read();
        let mut pages_out: Vec<TextsPage> = Vec::with_capacity(page_ids_c.len());
        for (i, id) in page_ids_c.iter().enumerate() {
            let page = scene
                .pages
                .get(id)
                .context(format!("page {id} not found"))?;
            let mut texts: Vec<String> = Vec::new();
            for (_node_id, node) in page.nodes.iter() {
                if let koharu_core::scene::NodeKind::Text(text_data) = &node.kind
                    && let Some(s) = text_node_text(text_data, slot)
                {
                    texts.push(s.to_string());
                }
            }
            pages_out.push(TextsPage { page: i + 1, texts });
        }
        let export = TextsExport { pages: pages_out };
        let bytes = serde_json::to_vec_pretty(&export)?;
        Ok(bytes)
    })
    .await
    .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
    .map_err(ApiError::internal)?;

    let base = sanitize(project_name, "export");
    let filename = format!("{base}-{filename_suffix}.json");
    Ok(bytes_response_with_filename(
        payload,
        &filename,
        "application/json",
    ))
}

async fn export_image_role(
    session: &std::sync::Arc<koharu_app::ProjectSession>,
    pages: Option<&[PageId]>,
    role: ImageRole,
    project_name: &str,
) -> ApiResult<Response> {
    let page_ids = resolve_page_ids(session, pages)?;
    if page_ids.is_empty() {
        return Err(ApiError::bad_request("no pages in selection"));
    }
    let session_c = session.clone();
    let page_ids_c = page_ids.clone();
    let files = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let mut out: Vec<(String, Vec<u8>)> = Vec::new();
        for (i, id) in page_ids_c.iter().enumerate() {
            if let Some(bytes) = crate::psd_export::png_bytes_for_page(&session_c, *id, role)? {
                out.push((format!("page-{:03}-{id}.png", i + 1), bytes));
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
    .map_err(ApiError::internal)?;

    if files.is_empty() {
        return Err(ApiError::bad_request(
            "no pages have the requested layer populated",
        ));
    }
    files_to_response(files, project_name, role_ext(role))
}

// ---------------------------------------------------------------------------
// Import translations — write per-page texts into each text node's
// translation slot. The payload is user-supplied JSON; we
// defensively extract the JSON object from markdown fences or
// surrounding prose so hand-edited or model-generated payloads
// both work.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportTranslationsRequest {
    /// User-supplied JSON text. May be a raw object, wrapped in a
    /// markdown ```json fence, or surrounded by prose. The server
    /// extracts the first JSON object it can find.
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportTranslationsSkip {
    pub page: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportTranslationsResponse {
    /// Number of pages whose translations were applied.
    pub applied: usize,
    /// Per-page reasons for pages that were skipped (missing from response,
    /// length mismatch, etc.). Not an error — partial success is normal.
    pub skipped: Vec<ImportTranslationsSkip>,
    /// Top-level parse errors (e.g. could not extract JSON). Empty on success.
    pub errors: Vec<String>,
}

/// Schema for the import payload. Every field is `#[serde(default)]` so the
/// parser tolerates partial/garbled input gracefully.
#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct TranslationsResponse {
    #[serde(default)]
    pages: Vec<TranslatedPage>,
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct TranslatedPage {
    #[serde(default)]
    page: usize,
    #[serde(default)]
    texts: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/projects/current/import-translations",
    request_body = ImportTranslationsRequest,
    responses((status = 200, body = ImportTranslationsResponse))
)]
async fn import_translations(
    State(app): State<AppState>,
    Json(req): Json<ImportTranslationsRequest>,
) -> ApiResult<Json<ImportTranslationsResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;

    let json_text = match extract_json(&req.payload) {
        Some(s) => s.to_string(),
        None => {
            return Ok(Json(ImportTranslationsResponse {
                applied: 0,
                skipped: Vec::new(),
                errors: vec!["could not locate a JSON object in the payload".to_string()],
            }));
        }
    };

    let parsed: TranslationsResponse = match serde_json::from_str(&json_text) {
        Ok(v) => v,
        Err(e) => {
            return Ok(Json(ImportTranslationsResponse {
                applied: 0,
                skipped: Vec::new(),
                errors: vec![format!("failed to parse JSON: {e}")],
            }));
        }
    };

    // Build the batch of `UpdateNode` ops on a blocking thread (scene lock
    // + scene iteration), then apply via the shared history pipeline so the
    // changes land in one undoable entry.
    let session_c = session.clone();
    let build = tokio::task::spawn_blocking(
        move || -> anyhow::Result<(Vec<Op>, ImportTranslationsResponse)> {
            let scene = session_c.scene.read();
            let mut ops: Vec<Op> = Vec::new();
            let mut applied = 0usize;
            let mut skipped: Vec<ImportTranslationsSkip> = Vec::new();

            for (page_idx, (page_id, page)) in scene.pages.iter().enumerate() {
                let page_num = page_idx + 1;

                // Replicate the exporter's collection rule: iterate nodes in
                // insertion order, keep only Text nodes with non-empty trimmed text.
                let text_node_ids: Vec<NodeId> = page
                    .nodes
                    .iter()
                    .filter_map(|(node_id, node)| match &node.kind {
                        koharu_core::scene::NodeKind::Text(td) => {
                            text_node_text(td, TextSlot::Source).map(|_| *node_id)
                        }
                        _ => None,
                    })
                    .collect();

                if text_node_ids.is_empty() {
                    continue;
                }

                let Some(entry) = parsed.pages.iter().find(|p| p.page == page_num) else {
                    skipped.push(ImportTranslationsSkip {
                        page: page_num,
                        reason: "page missing from payload".to_string(),
                    });
                    continue;
                };

                if entry.texts.len() != text_node_ids.len() {
                    skipped.push(ImportTranslationsSkip {
                        page: page_num,
                        reason: format!(
                            "length mismatch: expected {}, got {}",
                            text_node_ids.len(),
                            entry.texts.len()
                        ),
                    });
                    continue;
                }

                for (node_id, text) in text_node_ids.iter().zip(entry.texts.iter()) {
                    ops.push(Op::UpdateNode {
                        page: *page_id,
                        id: *node_id,
                        patch: NodePatch {
                            data: Some(NodeDataPatch::Text(TextDataPatch {
                                translation: Some(Some(text.clone())),
                                ..Default::default()
                            })),
                            transform: None,
                            visible: None,
                        },
                        prev: NodePatch::default(),
                    });
                }
                applied += 1;
            }

            let summary = ImportTranslationsResponse {
                applied,
                skipped,
                errors: Vec::new(),
            };
            Ok((ops, summary))
        },
    );

    let (ops, mut summary) = build
        .await
        .map_err(|e| ApiError::internal(anyhow::Error::new(e)))?
        .map_err(ApiError::internal)?;

    if !ops.is_empty() {
        let op = if ops.len() == 1 {
            ops.into_iter().next().unwrap()
        } else {
            Op::Batch {
                ops,
                label: "Import translations".to_string(),
            }
        };
        if let Err(e) = app.apply(op) {
            summary.errors.push(format!("failed to apply: {e:#}"));
        }
    }

    Ok(Json(summary))
}

/// Locate a JSON object in arbitrary text input. Tries:
/// 1. a ```json ... ``` fenced block
/// 2. a generic ``` ... ``` fenced block
/// 3. the substring from the first `{` to the last `}`
fn extract_json(input: &str) -> Option<&str> {
    if let Some(start) = input.find("```json") {
        let after = &input[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    if let Some(start) = input.find("```") {
        let after = &input[start + 3..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    if let (Some(open), Some(close)) = (input.find('{'), input.rfind('}'))
        && close > open
    {
        return Some(input[open..=close].trim());
    }
    None
}

fn resolve_page_ids(
    session: &koharu_app::ProjectSession,
    requested: Option<&[PageId]>,
) -> ApiResult<Vec<PageId>> {
    let scene = session.scene.read();
    match requested {
        None => Ok(scene.pages.keys().copied().collect()),
        Some(ids) => {
            for id in ids {
                if !scene.pages.contains_key(id) {
                    return Err(ApiError::not_found(format!("page {id}")));
                }
            }
            Ok(ids.to_vec())
        }
    }
}

fn role_ext(role: ImageRole) -> &'static str {
    match role {
        ImageRole::Rendered => "png",
        ImageRole::Inpainted => "png",
        ImageRole::Source => "png",
        ImageRole::Custom => "png",
    }
}

fn files_to_response(
    mut files: Vec<(String, Vec<u8>)>,
    project_name: &str,
    ext: &str,
) -> ApiResult<Response> {
    if files.len() == 1 {
        let (fname, bytes) = files.remove(0);
        let content_type = match ext {
            "psd" => "image/vnd.adobe.photoshop",
            "png" => "image/png",
            "khr" => "application/octet-stream",
            _ => "application/octet-stream",
        };
        return Ok(bytes_response_with_filename(bytes, &fname, content_type));
    }
    let zip_bytes = koharu_app::archive::zip_files_to_bytes(&files).map_err(ApiError::internal)?;
    let base = sanitize(project_name, "export");
    let filename = format!("{base}-{ext}.zip");
    Ok(bytes_response_with_filename(
        zip_bytes,
        &filename,
        "application/zip",
    ))
}

fn bytes_response(bytes: Vec<u8>, base: &str, ext: &str, content_type: &str) -> Response {
    let filename = format!("{base}.{ext}");
    bytes_response_with_filename(bytes, &filename, content_type)
}

fn bytes_response_with_filename(bytes: Vec<u8>, filename: &str, content_type: &str) -> Response {
    let cd = format!("attachment; filename=\"{filename}\"");
    let mut resp = Response::new(Body::from(bytes));
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if let Ok(v) = HeaderValue::from_str(&cd) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    resp.into_response()
}

fn sanitize(name: &str, fallback: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if s.is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::extract_json;

    #[test]
    fn extracts_from_json_fence() {
        let input = "Here you go:\n```json\n{\"pages\":[]}\n```\nDone.";
        assert_eq!(extract_json(input), Some("{\"pages\":[]}"));
    }

    #[test]
    fn extracts_from_generic_fence() {
        let input = "```\n{\"pages\":[]}\n```";
        assert_eq!(extract_json(input), Some("{\"pages\":[]}"));
    }

    #[test]
    fn extracts_first_brace_to_last_brace_as_fallback() {
        let input = "Some prose. {\"pages\":[{\"page\":1,\"texts\":[]}]} trailing.";
        assert_eq!(
            extract_json(input),
            Some("{\"pages\":[{\"page\":1,\"texts\":[]}]}")
        );
    }

    #[test]
    fn returns_none_when_no_braces() {
        assert_eq!(extract_json("no json here"), None);
    }
}
