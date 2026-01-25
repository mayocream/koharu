use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use koharu_renderer::renderer::TextShaderEffect;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

use crate::{
    api_crs,
    app::AppResources,
    llm, ml,
    operations::{self, DocumentInput, ExportedDocument, InpaintRegion},
    renderer::Renderer,
    state::{AppState, Document, TextBlock},
    version,
};

#[derive(Clone)]
pub struct ApiState {
    resources: AppResources,
}

impl ApiState {
    pub fn app_state(&self) -> &AppState {
        &self.resources.state
    }

    pub fn ml(&self) -> &Arc<ml::Model> {
        &self.resources.ml
    }

    pub fn llm(&self) -> &Arc<llm::Model> {
        &self.resources.llm
    }

    pub fn renderer(&self) -> &Arc<Renderer> {
        &self.resources.renderer
    }
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl From<crate::result::CommandError> for ApiError {
    fn from(err: crate::result::CommandError) -> Self {
        Self::bad_request(err.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal(err.to_string())
    }
}

impl From<axum::extract::multipart::MultipartError> for ApiError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        Self::bad_request(err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug, Deserialize)]
struct IndexPayload {
    index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InpaintMaskPayload {
    index: usize,
    mask: Vec<u8>,
    region: Option<InpaintRegion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrushPayload {
    index: usize,
    patch: Vec<u8>,
    region: InpaintRegion,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InpaintPartialPayload {
    index: usize,
    region: InpaintRegion,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenderPayload {
    index: usize,
    text_block_index: Option<usize>,
    shader_effect: Option<TextShaderEffect>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextBlocksPayload {
    index: usize,
    text_blocks: Vec<TextBlock>,
}

#[derive(Debug, Deserialize)]
struct LlmLoadPayload {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmGeneratePayload {
    index: usize,
    text_block_index: Option<usize>,
    language: Option<String>,
}

#[derive(Embed)]
#[folder = "$CARGO_WORKSPACE_DIR/ui/out"]
#[allow_missing = true]
struct EmbeddedUi;

pub async fn serve_embedded(uri: Uri) -> impl IntoResponse {
    let path = uri.path();
    let target = match path {
        "/" => "index.html",
        _ => path.trim_start_matches('/'),
    };

    if let Some(resp) = embedded_response(target) {
        return resp;
    }

    if let Some(resp) = embedded_response("index.html") {
        return resp;
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

fn embedded_response(path: &str) -> Option<Response> {
    let asset = EmbeddedUi::get(path)?;
    let mut response = Response::new(Body::from(asset.data.into_owned()));
    if let Some(ct) = content_type_for(path) {
        response.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    Some(response)
}

fn content_type_for(path: &str) -> Option<HeaderValue> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mime = match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript",
        "mjs" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };

    HeaderValue::from_str(mime).ok()
}

pub async fn serve(bind: String, resources: AppResources) -> Result<()> {
    let state = ApiState { resources };

    let mut router = Router::new()
        .route("/api/app_version", get(app_version).post(app_version))
        .route("/api/get_documents", get(get_documents).post(get_documents))
        .route("/api/open_documents", post(open_documents))
        .route("/api/save_documents", post(save_documents))
        .route("/api/export_document", post(export_document))
        .route("/api/export_all_documents", post(export_all_documents))
        .route("/api/detect", post(detect))
        .route("/api/ocr", post(ocr))
        .route("/api/inpaint", post(inpaint))
        .route("/api/inpaint_partial", post(inpaint_partial))
        .route("/api/render", post(render))
        .route("/api/update_brush_layer", post(update_brush_layer))
        .route("/api/update_inpaint_mask", post(update_inpaint_mask))
        .route("/api/update_text_blocks", post(update_text_blocks))
        .route(
            "/api/list_font_families",
            get(list_font_families).post(list_font_families),
        )
        .route("/api/llm_list", get(llm_list).post(llm_list))
        .route("/api/llm_load", post(llm_load))
        .route("/api/llm_offload", post(llm_offload))
        .route("/api/llm_ready", get(llm_ready).post(llm_ready))
        .route("/api/llm_generate", post(llm_generate))
        .route(
            "/translate/with-form/image/stream",
            post(api_crs::translate_with_form_image_stream),
        )
        .with_state(state)
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .layer(CorsLayer::permissive());

    router = router.fallback(serve_embedded);

    let listener = TcpListener::bind(&bind).await?;
    tracing::info!("Headless server listening on http://{}", bind);
    axum::serve(listener, router.into_make_service()).await?;

    Ok(())
}

async fn app_version() -> impl IntoResponse {
    Json(version::current().to_string())
}

async fn get_documents(State(state): State<ApiState>) -> ApiResult<Json<Vec<Document>>> {
    let docs = operations::get_documents(state.app_state()).await?;
    Ok(Json(docs))
}

async fn open_documents(
    State(state): State<ApiState>,
    mut multipart: Multipart,
) -> ApiResult<Json<Vec<Document>>> {
    let mut inputs = Vec::new();
    while let Some(field) = multipart.next_field().await? {
        let file_name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "document".to_string());
        let data = field.bytes().await?;
        if data.is_empty() {
            continue;
        }
        inputs.push(DocumentInput {
            path: PathBuf::from(file_name),
            bytes: data.to_vec(),
        });
    }

    if inputs.is_empty() {
        return Err(ApiError::bad_request("No files uploaded"));
    }

    let docs = operations::load_documents(inputs).map_err(ApiError::from)?;
    let docs = operations::set_documents(state.app_state(), docs)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(docs))
}

async fn save_documents(State(state): State<ApiState>) -> ApiResult<Response> {
    let filename = operations::default_khr_filename(state.app_state())
        .await
        .ok_or_else(|| ApiError::bad_request("No documents to save"))?;
    let bytes = operations::serialize_state(state.app_state())
        .await
        .map_err(ApiError::from)?;

    attachment_response(&filename, bytes, "application/octet-stream")
}

async fn export_document(
    State(state): State<ApiState>,
    Json(payload): Json<IndexPayload>,
) -> ApiResult<Response> {
    let export = operations::export_document(state.app_state(), payload.index)
        .await
        .map_err(ApiError::from)?;
    let ext = Path::new(&export.filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("bin");
    attachment_response(&export.filename, export.bytes, mime_from_ext(ext))
}

async fn export_all_documents(State(state): State<ApiState>) -> ApiResult<Response> {
    let exports = operations::export_all_documents(state.app_state())
        .await
        .map_err(ApiError::from)?;
    let zip_bytes = zip_exports(exports).map_err(ApiError::from)?;
    attachment_response("export.zip", zip_bytes, "application/zip")
}

async fn detect(
    State(state): State<ApiState>,
    Json(payload): Json<IndexPayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::detect(state.app_state(), state.ml(), payload.index)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn ocr(
    State(state): State<ApiState>,
    Json(payload): Json<IndexPayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::ocr(state.app_state(), state.ml(), payload.index)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn inpaint(
    State(state): State<ApiState>,
    Json(payload): Json<IndexPayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::inpaint(state.app_state(), state.ml(), payload.index)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn update_inpaint_mask(
    State(state): State<ApiState>,
    Json(payload): Json<InpaintMaskPayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::update_inpaint_mask(
        state.app_state(),
        payload.index,
        payload.mask,
        payload.region,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn update_brush_layer(
    State(state): State<ApiState>,
    Json(payload): Json<BrushPayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::update_brush_layer(
        state.app_state(),
        payload.index,
        payload.patch,
        payload.region,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn inpaint_partial(
    State(state): State<ApiState>,
    Json(payload): Json<InpaintPartialPayload>,
) -> ApiResult<Json<Document>> {
    let doc =
        operations::inpaint_partial(state.app_state(), state.ml(), payload.index, payload.region)
            .await
            .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn render(
    State(state): State<ApiState>,
    Json(payload): Json<RenderPayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::render(
        state.app_state(),
        state.renderer(),
        payload.index,
        payload.text_block_index,
        payload.shader_effect,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn update_text_blocks(
    State(state): State<ApiState>,
    Json(payload): Json<TextBlocksPayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::update_text_blocks(state.app_state(), payload.index, payload.text_blocks)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(doc))
}

async fn list_font_families(State(state): State<ApiState>) -> ApiResult<Json<Vec<String>>> {
    let fonts = operations::list_font_families(state.renderer()).map_err(ApiError::from)?;
    Ok(Json(fonts))
}

async fn llm_list(State(state): State<ApiState>) -> ApiResult<Json<Vec<llm::ModelInfo>>> {
    let models = operations::llm_list(state.llm());
    Ok(Json(models))
}

async fn llm_load(
    State(state): State<ApiState>,
    Json(payload): Json<LlmLoadPayload>,
) -> ApiResult<StatusCode> {
    operations::llm_load(state.llm(), payload.id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn llm_offload(State(state): State<ApiState>) -> ApiResult<StatusCode> {
    operations::llm_offload(state.llm())
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn llm_ready(State(state): State<ApiState>) -> ApiResult<Json<bool>> {
    let ready = operations::llm_ready(state.llm())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(ready))
}

async fn llm_generate(
    State(state): State<ApiState>,
    Json(payload): Json<LlmGeneratePayload>,
) -> ApiResult<Json<Document>> {
    let doc = operations::llm_generate(
        state.app_state(),
        state.llm(),
        payload.index,
        payload.text_block_index,
        payload.language,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(doc))
}

fn attachment_response(filename: &str, bytes: Vec<u8>, content_type: &str) -> ApiResult<Response> {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).map_err(|err| ApiError::internal(err.to_string()))?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|err| ApiError::internal(err.to_string()))?,
    );
    Ok(response)
}

fn mime_from_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn zip_exports(exports: Vec<ExportedDocument>) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let cursor = Cursor::new(&mut buf);
    let mut writer = zip::ZipWriter::new(cursor);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for export in exports {
        writer.start_file(export.filename, options)?;
        writer.write_all(&export.bytes)?;
    }

    writer.finish()?;
    Ok(buf)
}
