//! Project lifecycle routes. Every project lives under the managed
//! `{data.path}/projects/` directory; clients never supply filesystem
//! paths. A project's `id` is the `.khrproj/` directory basename.
//!
//! - `GET    /projects` — list managed projects
//! - `POST   /projects` — create a new project (`{name}`), server allocates path
//! - `POST   /projects/import` — extract a `.khr` archive into a fresh dir + open
//! - `PUT    /projects/current` — open a managed project by `id`
//! - `DELETE /projects/current` — close current session
//! - `POST   /projects/current/export` — export current; returns bytes

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use koharu_app::pipeline::support::text_nodes;
use koharu_app::{projects as project_dirs, utils};
use koharu_core::{
    ImageRole, NodeDataPatch, NodeId, NodePatch, Op, PageId, ProjectSummary, Scene, TextDataPatch,
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
        .routes(routes!(import_script))
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
    /// One '.txt' per page.
    Script,
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
        ExportFormat::Script => export_script(&session, req.pages.as_deref(), &project_name).await,
    }
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

async fn export_script(
    session: &std::sync::Arc<koharu_app::ProjectSession>,
    pages: Option<&[PageId]>,
    project_name: &str,
) -> ApiResult<Response> {
    let page_ids = resolve_page_ids(session, pages)?;
    if page_ids.is_empty() {
        return Err(ApiError::bad_request("no pages in selection"));
    }

    let scene = session.scene.read();

    let mut page_blocks: Vec<(PageId, Vec<(NodeId, String)>)> = Vec::new();

    for &page_id in &page_ids {
        let targets = collect_translation_targets_from(&scene, page_id);
        if targets.is_empty() {
            continue;
        }
        page_blocks.push((page_id, targets));
    }

    if page_blocks.is_empty() {
        return Err(ApiError::bad_request("no pages with text blocks to export"));
    }

    let single_page = page_blocks.len() == 1;

    let mut body = String::new();

    for (page_id, blocks) in page_blocks.iter() {
        if !body.is_empty() {
            body.push('\n');
        }

        if !single_page {
            let page_index = scene
                .pages
                .get_index_of(page_id)
                .map(|i| i + 1)
                .unwrap_or(page_blocks.len());
            body.push_str(&format!("Page {}", page_index));
        }

        if !body.is_empty() {
            body.push('\n');
        }

        let formatted = utils::format_sources(
            &blocks
                .iter()
                .map(|block| block.1.clone())
                .collect::<Vec<String>>(),
        );
        body.push_str(&formatted);
    }

    let file = ("script.txt".to_string(), body.into_bytes());

    files_to_response(vec![file], project_name, "txt")
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportScriptRequest {
    pub body: String,
    #[serde(default)]
    pub page_id: Option<PageId>,
}

#[utoipa::path(
    post,
    path = "/projects/current/import-script",
    request_body = ImportScriptRequest,
    responses((status = 200, description = "Translations applied"),
              (status = 400, description = "Parse error or invalid request"))
)]
async fn import_script(
    State(app): State<AppState>,
    Json(req): Json<ImportScriptRequest>,
) -> ApiResult<()> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;

    let scene = session.scene_snapshot();

    let entries = parse_script_body(&req.body, req.page_id, &scene)?;
    if entries.is_empty() {
        return Err(ApiError::bad_request("no valid entries found in script"));
    }

    // Build UpdateNode ops for each (page, node, translation) entry
    let mut ops = Vec::new();
    for (page_id, node_id, translation) in entries {
        ops.push(Op::UpdateNode {
            page: page_id,
            id: node_id,
            patch: NodePatch {
                data: Some(NodeDataPatch::Text(TextDataPatch {
                    translation: Some(Some(translation)),
                    ..Default::default()
                })),
                transform: None,
                visible: None,
            },
            prev: NodePatch::default(),
        });
    }

    let batch = Op::Batch {
        ops,
        label: "Import script translations".into(),
    };

    session.apply(batch).map_err(|e| ApiError::internal(e))?;

    Ok(())
}

/// Parse the script body and produce (page_id, node_id, translation_text) tuples.
fn parse_script_body(
    body: &str,
    page_id: Option<PageId>,
    scene: &Scene,
) -> ApiResult<Vec<(PageId, NodeId, String)>> {
    let mut entries = Vec::new();

    if let Some(page_id) = page_id {
        let targets = collect_translation_targets_from(scene, page_id);
        if let Some(translation_texts) = utils::parse_tagged_blocks(body, targets.len())? {
            for ((node_id, _), translation) in targets.into_iter().zip(translation_texts) {
                entries.push((page_id, node_id, translation));
            }
        } else {
            return Err(ApiError::bad_request(
                "script file has no translation lines but page has text nodes",
            ));
        }
    } else {
        let sections = split_into_page_sections(body);
        for (page_number, section_lines) in sections {
            if page_number < 1 {
                return Err(ApiError::bad_request("page numbers must be >= 1"));
            }
            let (page_id, _) = scene.pages.get_index(page_number - 1).ok_or_else(|| {
                ApiError::bad_request(format!("page {} not found in project", page_number))
            })?;

            let targets = collect_translation_targets_from(scene, *page_id);
            if let Some(translation_texts) =
                utils::parse_tagged_blocks(&section_lines, targets.len())?
            {
                for ((node_id, _), translation) in targets.into_iter().zip(translation_texts) {
                    entries.push((*page_id, node_id, translation));
                }
            }
        }
    }

    Ok(entries)
}

/// Split multi-page script body into `(page_number, page_body)` pairs.
/// Lines starting with "Page " and containing a page number are treated as section headers.
fn split_into_page_sections(body: &str) -> Vec<(usize, String)> {
    let mut sections: Vec<(usize, String)> = Vec::new();
    let mut current_page: Option<usize> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    for line in body.lines() {
        let parsed_page = line
            .strip_prefix("Page ")
            .and_then(|rest| rest.trim().parse::<usize>().ok());

        if let Some(page) = parsed_page {
            if let Some(prev_page) = current_page.take() {
                let section_body = current_lines.join("\n");
                if !section_body.trim().is_empty() {
                    sections.push((prev_page, section_body));
                }
            }

            current_lines.clear();
            current_page = Some(page);
        } else {
            current_lines.push(line);
        }
    }

    if let Some(page) = current_page {
        let section_body = current_lines.join("\n");
        if !section_body.trim().is_empty() {
            sections.push((page, section_body));
        }
    }

    sections
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
            "txt" => "text/plain",
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

/// Collect all the non-empty text blocks from the specified page in the scene
fn collect_translation_targets_from(scene: &Scene, page: PageId) -> Vec<(NodeId, String)> {
    text_nodes(scene, page)
        .into_iter()
        .filter_map(|(id, _, text_data)| {
            let text = text_data.text.as_ref()?;
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| (id, text.clone()))
        })
        .collect()
}
