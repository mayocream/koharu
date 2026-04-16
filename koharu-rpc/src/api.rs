use axum::{
    Json,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{
        HeaderValue, StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use koharu_app::{AppResources, config as app_config, edit, engine, io, llm, pipeline};
use koharu_core::{
    CreateTextBlock, Document, DocumentDetail, DocumentSummary, DownloadState, ExportLayer,
    ExportResult, FontFaceInfo, JobState, LlmCatalog, LlmLoadRequest, LlmState, MaskRegionRequest,
    MetaInfo, PipelineJobRequest, RenderRequest, ReorderRequest, TextBlock, TextBlockDetail,
    TextBlockInput, TextBlockPatch, TranslateRequest,
};
use koharu_psd::{PsdExportOptions, TextLayerMode};
use serde::{Deserialize, Serialize};
use utoipa::IntoParams;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{shared::SharedState, tracker::Tracker};

const MAX_BODY_SIZE: usize = 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct ApiState {
    pub resources: SharedState,
    pub tracker: Tracker,
}

impl ApiState {
    fn resources(&self) -> ApiResult<AppResources> {
        self.resources.get().ok_or_else(|| {
            ApiError::service_unavailable(anyhow::anyhow!("Resources not initialized"))
        })
    }
}

pub fn api() -> (axum::Router<ApiState>, utoipa::openapi::OpenApi) {
    OpenApiRouter::default()
        .routes(routes!(list_documents, import_documents))
        .routes(routes!(reorder_documents))
        .routes(routes!(get_document))
        .routes(routes!(update_document_style))
        .routes(routes!(get_blob))
        .routes(routes!(get_document_thumbnail))
        .routes(routes!(detect_document))
        .routes(routes!(recognize_document))
        .routes(routes!(inpaint_document))
        .routes(routes!(render_document))
        .routes(routes!(translate_document))
        .routes(routes!(update_mask))
        .routes(routes!(update_brush_layer))
        .routes(routes!(inpaint_region))
        .routes(routes!(create_text_block, put_text_blocks))
        .routes(routes!(patch_text_block, delete_text_block))
        .routes(routes!(export_document))
        .routes(routes!(batch_export))
        .routes(routes!(get_llm, load_llm, unload_llm))
        .routes(routes!(get_llm_catalog))
        .routes(routes!(start_pipeline))
        .routes(routes!(list_jobs))
        .routes(routes!(get_job, cancel_job))
        .routes(routes!(list_downloads))
        .routes(routes!(get_meta))
        .routes(routes!(list_fonts))
        .routes(routes!(get_google_fonts_catalog))
        .routes(routes!(fetch_google_font, get_google_font_file))
        .routes(routes!(get_config, update_config))
        .routes(routes!(get_engine_catalog))
        .split_for_parts()
}

pub fn router(resources: SharedState, tracker: Tracker) -> axum::Router {
    let state = ApiState { resources, tracker };
    let (router, _) = api();
    router
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status: status.as_u16(),
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    fn service_unavailable(error: anyhow::Error) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string())
    }

    fn internal(error: anyhow::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        let message = error.to_string();
        if message.contains("not found") || message.contains("out of range") {
            Self::not_found(message)
        } else {
            Self::bad_request(message)
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

#[derive(Debug, utoipa::ToSchema)]
#[allow(dead_code)]
struct MultipartUpload {
    #[schema(value_type = Vec<String>, format = Binary)]
    files: Vec<Vec<u8>>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct ImportQuery {
    mode: Option<koharu_core::ImportMode>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct ExportQuery {
    layer: Option<ExportLayer>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct ExportBatchRequest {
    layer: Option<ExportLayer>,
}

// ---------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/config",
    operation_id = "getConfig",
    tag = "system",
    responses(
        (status = 200, body = inline(app_config::AppConfig)),
        (status = 503, body = ApiError),
    ),
)]
async fn get_config() -> ApiResult<Json<app_config::AppConfig>> {
    let config = app_config::load().map_err(ApiError::internal)?;
    Ok(Json(config))
}

#[utoipa::path(
    put,
    path = "/config",
    operation_id = "updateConfig",
    tag = "system",
    request_body = inline(app_config::AppConfig),
    responses(
        (status = 200, body = inline(app_config::AppConfig)),
        (status = 400, body = ApiError),
    ),
)]
async fn update_config(
    State(state): State<ApiState>,
    Json(config): Json<app_config::AppConfig>,
) -> ApiResult<Json<app_config::AppConfig>> {
    app_config::sync_secrets(&config).map_err(ApiError::from)?;
    app_config::save(&config).map_err(ApiError::internal)?;
    let reloaded = app_config::load().map_err(ApiError::internal)?;
    if let Ok(resources) = state.resources() {
        *resources.config.write().await = reloaded.clone();
        resources.registry.clear().await;
    }
    Ok(Json(reloaded))
}

#[utoipa::path(
    get,
    path = "/engines",
    operation_id = "getEngineCatalog",
    tag = "system",
    responses(
        (status = 200, body = inline(koharu_core::EngineCatalog)),
    ),
)]
async fn get_engine_catalog() -> Json<koharu_core::EngineCatalog> {
    Json(engine::catalog())
}

#[utoipa::path(
    get,
    path = "/meta",
    operation_id = "getMeta",
    tag = "system",
    responses(
        (status = 200, body = MetaInfo),
        (status = 503, body = ApiError),
    ),
)]
async fn get_meta(State(state): State<ApiState>) -> ApiResult<Json<MetaInfo>> {
    let resources = state.resources()?;
    let device = io::device(resources.clone()).await?;
    Ok(Json(MetaInfo {
        version: resources.version.to_string(),
        ml_device: device.ml_device,
    }))
}

#[utoipa::path(
    get,
    path = "/fonts",
    operation_id = "listFonts",
    tag = "system",
    responses(
        (status = 200, body = Vec<FontFaceInfo>),
        (status = 503, body = ApiError),
    ),
)]
async fn list_fonts(State(state): State<ApiState>) -> ApiResult<Json<Vec<FontFaceInfo>>> {
    let resources = state.resources()?;
    let renderer = engine::get_renderer(&resources)
        .await
        .map_err(ApiError::from)?;
    let fonts = renderer.available_fonts().map_err(ApiError::from)?;
    Ok(Json(fonts))
}

// ---------------------------------------------------------------------------
// Google Fonts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct GoogleFontCatalogResponse {
    fonts: Vec<GoogleFontCatalogEntry>,
    recommended: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct GoogleFontCatalogEntry {
    family: String,
    category: String,
    subsets: Vec<String>,
    cached: bool,
}

#[utoipa::path(
    get,
    path = "/fonts/google/catalog",
    operation_id = "getGoogleFontsCatalog",
    tag = "system",
    responses(
        (status = 200, body = GoogleFontCatalogResponse),
        (status = 503, body = ApiError),
    ),
)]
async fn get_google_fonts_catalog(
    State(state): State<ApiState>,
) -> ApiResult<Json<GoogleFontCatalogResponse>> {
    let resources = state.resources()?;
    let renderer = engine::get_renderer(&resources)
        .await
        .map_err(ApiError::from)?;
    let service = &renderer.google_fonts;
    let catalog = service.catalog();

    let mut fonts = Vec::with_capacity(catalog.fonts.len());
    for entry in &catalog.fonts {
        fonts.push(GoogleFontCatalogEntry {
            family: entry.family.clone(),
            category: entry.category.clone(),
            subsets: entry.subsets.clone(),
            cached: service.is_cached(&entry.family).await,
        });
    }

    let recommended = service
        .recommended_families()
        .iter()
        .map(|s| s.to_string())
        .collect();

    Ok(Json(GoogleFontCatalogResponse { fonts, recommended }))
}

#[utoipa::path(
    post,
    path = "/fonts/google/{family}/fetch",
    operation_id = "fetchGoogleFont",
    tag = "system",
    params(
        ("family" = String, Path, description = "Font family name"),
    ),
    responses(
        (status = 200, body = FontFaceInfo),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn fetch_google_font(
    State(state): State<ApiState>,
    Path(family): Path<String>,
) -> ApiResult<Json<FontFaceInfo>> {
    let resources = state.resources()?;
    let renderer = engine::get_renderer(&resources)
        .await
        .map_err(ApiError::from)?;
    let service = &renderer.google_fonts;

    let entry = service.find_entry(&family).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            format!("font family not found: {family}"),
        )
    })?;

    let http = resources.runtime.http_client();
    service
        .fetch_family(&family, &http)
        .await
        .map_err(ApiError::from)?;

    let category = Some(entry.category.clone());

    Ok(Json(FontFaceInfo {
        family_name: family.clone(),
        post_script_name: family,
        source: koharu_core::FontSource::Google,
        category,
        cached: true,
    }))
}

#[utoipa::path(
    get,
    path = "/fonts/google/{family}/file",
    operation_id = "getGoogleFontFile",
    tag = "system",
    params(
        ("family" = String, Path, description = "Font family name"),
    ),
    responses(
        (status = 200, description = "Font file bytes", content_type = "font/ttf"),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn get_google_font_file(
    State(state): State<ApiState>,
    Path(family): Path<String>,
) -> Result<Response, ApiError> {
    let resources = state.resources()?;
    let renderer = engine::get_renderer(&resources)
        .await
        .map_err(ApiError::from)?;
    let service = &renderer.google_fonts;

    let data = service
        .read_cached_file(&family)
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                format!("font not cached: {family}. Call fetch first."),
            )
        })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("font/ttf"))
        .body(Body::from(data))
        .unwrap())
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/documents",
    operation_id = "listDocuments",
    tag = "documents",
    responses(
        (status = 200, body = Vec<DocumentSummary>),
        (status = 503, body = ApiError),
    ),
)]
async fn list_documents(State(state): State<ApiState>) -> ApiResult<Json<Vec<DocumentSummary>>> {
    let resources = state.resources()?;
    let documents = resources.storage.list_pages().await;
    Ok(Json(documents))
}

#[utoipa::path(
    put,
    path = "/documents/order",
    operation_id = "reorderDocuments",
    tag = "documents",
    request_body(content = ReorderRequest, content_type = "application/json"),
    responses(
        (status = 200, body = Vec<DocumentSummary>),
        (status = 400, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn reorder_documents(
    State(state): State<ApiState>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<Json<Vec<DocumentSummary>>> {
    let resources = state.resources()?;
    resources
        .storage
        .reorder_pages(&body.ids)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let documents = resources.storage.list_pages().await;
    Ok(Json(documents))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateDocumentStyleRequest {
    #[serde(default)]
    pub default_font: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/documents/{document_id}/style",
    operation_id = "updateDocumentStyle",
    tag = "documents",
    params(
        ("document_id" = String, Path, description = "Document ID"),
    ),
    request_body = UpdateDocumentStyleRequest,
    responses(
        (status = 200, body = UpdateDocumentStyleRequest),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn update_document_style(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<UpdateDocumentStyleRequest>,
) -> ApiResult<Json<UpdateDocumentStyleRequest>> {
    let resources = state.resources()?;
    resources
        .storage
        .update_page(&document_id, |doc| {
            let style = doc.style.get_or_insert_with(Default::default);
            style.default_font = request.default_font.clone();
        })
        .await
        .map_err(ApiError::from)?;
    Ok(Json(request))
}

#[utoipa::path(
    get,
    path = "/documents/{document_id}",
    operation_id = "getDocument",
    tag = "documents",
    params(("document_id" = String, Path,)),
    responses(
        (status = 200, body = DocumentDetail),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all, fields(document_id = %document_id))]
async fn get_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<Json<DocumentDetail>> {
    let resources = state.resources()?;
    let doc = find_document(&resources, &document_id).await?;

    let text_blocks = doc
        .text_blocks
        .iter()
        .map(koharu_core::TextBlockDetail::from)
        .collect();

    let detail = DocumentDetail {
        id: doc.id,
        name: doc.name,
        width: doc.width,
        height: doc.height,
        text_blocks,
        image: doc.source.hash().to_string(),
        segment: doc.segment.as_ref().map(|r| r.hash().to_string()),
        inpainted: doc.inpainted.as_ref().map(|r| r.hash().to_string()),
        brush_layer: doc.brush_layer.as_ref().map(|r| r.hash().to_string()),
        rendered: doc.rendered.as_ref().map(|r| r.hash().to_string()),
        style: doc.style.clone(),
    };

    Ok(Json(detail))
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct ThumbnailQuery {
    size: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/documents/{document_id}/thumbnail",
    operation_id = "getDocumentThumbnail",
    tag = "documents",
    params(("document_id" = String, Path,), ThumbnailQuery),
    responses(
        (status = 200, content_type = "image/webp", body = inline(String)),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn get_document_thumbnail(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Query(query): Query<ThumbnailQuery>,
) -> ApiResult<Response> {
    let size = query.size.unwrap_or(200).min(800);
    let resources = state.resources()?;
    let doc = find_document(&resources, &document_id).await?;
    let source_ref = doc.rendered.as_ref().unwrap_or(&doc.source);
    let source_img = resources
        .storage
        .images
        .load(source_ref)
        .map_err(ApiError::internal)?;
    let thumbnail = source_img.thumbnail(size, size);
    let bytes =
        koharu_app::utils::encode_image_dynamic(&thumbnail, "webp").map_err(ApiError::internal)?;
    Ok(binary_response(bytes, "image/webp", None))
}

#[utoipa::path(
    post,
    path = "/documents",
    operation_id = "importDocuments",
    tag = "documents",
    params(ImportQuery),
    request_body(content_type = "multipart/form-data", content = inline(MultipartUpload)),
    responses(
        (status = 200, body = koharu_core::ImportResult),
        (status = 400, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn import_documents(
    State(state): State<ApiState>,
    Query(query): Query<ImportQuery>,
    mut multipart: Multipart,
) -> ApiResult<Json<koharu_core::ImportResult>> {
    let resources = state.resources()?;
    let mut uploaded_files = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?
    {
        let filename = field
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "upload.bin".to_string());
        let data = field
            .bytes()
            .await
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        uploaded_files.push((filename, data.to_vec()));
    }

    if uploaded_files.is_empty() {
        return Err(ApiError::bad_request("No files uploaded"));
    }

    let files = uploaded_files
        .into_iter()
        .map(|(name, data)| koharu_core::FileEntry { name, data })
        .collect();

    let payload = koharu_core::OpenDocumentsPayload { files };
    match query.mode.unwrap_or(koharu_core::ImportMode::Replace) {
        koharu_core::ImportMode::Replace => {
            io::open_documents(resources.clone(), payload).await?;
        }
        koharu_core::ImportMode::Append => {
            io::add_documents(resources.clone(), payload).await?;
        }
    }

    let documents = resources.storage.list_pages().await;

    Ok(Json(koharu_core::ImportResult {
        total_count: documents.len(),
        documents,
    }))
}

// ---------------------------------------------------------------------------
// Processing
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/documents/{document_id}/detect",
    operation_id = "detectDocument",
    tag = "processing",
    params(("document_id" = String, Path,)),
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn detect_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let pipeline = resources.config.read().await.pipeline.clone();
    let mut detect_engines = vec![pipeline.detector.clone()];
    if let Ok(det) = engine::Registry::find(&pipeline.detector)
        && !det.produces.contains(&engine::Artifact::Segment)
    {
        detect_engines.push(pipeline.segmenter.clone());
    }
    detect_engines.push("yuzumarker-font-detection".to_string());
    let refs: Vec<&str> = detect_engines.iter().map(|s| s.as_str()).collect();
    engine::run_many(&refs, &resources, &document_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/documents/{document_id}/recognize",
    operation_id = "recognizeDocument",
    tag = "processing",
    params(("document_id" = String, Path,)),
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn recognize_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let ocr = resources.config.read().await.pipeline.ocr.clone();
    engine::run_one(&ocr, &resources, &document_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/documents/{document_id}/inpaint",
    operation_id = "inpaintDocument",
    tag = "processing",
    params(("document_id" = String, Path,)),
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn inpaint_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let inpainter = resources.config.read().await.pipeline.inpainter.clone();
    engine::run_one(&inpainter, &resources, &document_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/documents/{document_id}/render",
    operation_id = "renderDocument",
    tag = "processing",
    params(("document_id" = String, Path,)),
    request_body = RenderRequest,
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn render_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<RenderRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let document = find_document(&resources, &document_id).await?;
    let text_block_index = request
        .text_block_id
        .as_deref()
        .map(|id| find_text_block_index(&document, id))
        .transpose()?;

    engine::render_document(
        &resources,
        &document_id,
        text_block_index,
        request.shader_effect,
        request.shader_stroke,
        None,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/documents/{document_id}/translate",
    operation_id = "translateDocument",
    tag = "processing",
    params(("document_id" = String, Path,)),
    request_body = TranslateRequest,
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn translate_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<TranslateRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let document = find_document(&resources, &document_id).await?;
    let text_block_index = request
        .text_block_id
        .as_deref()
        .map(|id| find_text_block_index(&document, id))
        .transpose()?;

    llm::llm_generate(
        resources,
        &document_id,
        text_block_index,
        request.language.as_deref(),
        request.system_prompt.as_deref(),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Regions
// ---------------------------------------------------------------------------

#[utoipa::path(
    put,
    path = "/documents/{document_id}/mask",
    operation_id = "updateMask",
    tag = "regions",
    params(("document_id" = String, Path,)),
    request_body = MaskRegionRequest,
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn update_mask(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<MaskRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    edit::update_inpaint_mask(resources, &document_id, &request.data, request.region).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put,
    path = "/documents/{document_id}/brush-layer",
    operation_id = "updateBrushLayer",
    tag = "regions",
    params(("document_id" = String, Path,)),
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn update_brush_layer(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_core::BrushRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    edit::update_brush_layer(resources, &document_id, &request.data, request.region).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/documents/{document_id}/inpaint-region",
    operation_id = "inpaintRegion",
    tag = "regions",
    params(("document_id" = String, Path,)),
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn inpaint_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_core::InpaintRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    edit::inpaint_partial(resources, &document_id, request.region).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Text Blocks
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/documents/{document_id}/text-blocks",
    operation_id = "createTextBlock",
    tag = "text-blocks",
    params(("document_id" = String, Path,)),
    request_body = CreateTextBlock,
    responses(
        (status = 200, body = TextBlockDetail),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn create_text_block(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<CreateTextBlock>,
) -> ApiResult<Json<TextBlockDetail>> {
    let resources = state.resources()?;

    let block = TextBlock {
        x: request.x,
        y: request.y,
        width: request.width,
        height: request.height,
        confidence: 1.0,
        ..Default::default()
    };

    let mut detail = None;
    resources
        .storage
        .update_page(&document_id, |page| {
            page.text_blocks.push(block);
            detail = page.text_blocks.last().map(TextBlockDetail::from);
        })
        .await
        .map_err(ApiError::from)?;

    detail
        .map(Json)
        .ok_or_else(|| ApiError::internal(anyhow::anyhow!("Failed to append text block")))
}

#[utoipa::path(
    put,
    path = "/documents/{document_id}/text-blocks",
    operation_id = "putTextBlocks",
    tag = "text-blocks",
    params(("document_id" = String, Path,)),
    request_body = Vec<TextBlockInput>,
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn put_text_blocks(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(inputs): Json<Vec<TextBlockInput>>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;

    let mut any_changed = false;

    resources
        .storage
        .update_page(&document_id, |doc| {
            // Build a set of incoming IDs for deletion detection
            let incoming_ids: std::collections::HashSet<&str> = inputs
                .iter()
                .filter_map(|input| input.id.as_deref())
                .collect();

            // Delete blocks not present in the incoming array
            let before_len = doc.text_blocks.len();
            doc.text_blocks
                .retain(|block| incoming_ids.contains(block.id.as_str()));
            if doc.text_blocks.len() != before_len {
                any_changed = true;
            }

            for input in &inputs {
                if let Some(ref id) = input.id {
                    // Update existing block
                    if let Some(block) = doc.text_blocks.iter_mut().find(|b| &b.id == id) {
                        let patch = TextBlockPatch {
                            text: input.text.clone(),
                            translation: input.translation.clone(),
                            x: Some(input.x),
                            y: Some(input.y),
                            width: Some(input.width),
                            height: Some(input.height),
                            style: input.style.clone(),
                        };
                        let had_render = block.rendered.is_some();
                        apply_text_block_patch(block, patch);
                        if had_render && block.rendered.is_none() {
                            any_changed = true;
                        }
                    }
                } else {
                    // Create new block
                    let block = TextBlock {
                        x: input.x,
                        y: input.y,
                        width: input.width,
                        height: input.height,
                        text: input.text.clone(),
                        translation: input.translation.clone(),
                        style: input.style.clone(),
                        confidence: 1.0,
                        ..Default::default()
                    };

                    doc.text_blocks.push(block);
                    any_changed = true;
                }
            }
        })
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    patch,
    path = "/documents/{document_id}/text-blocks/{text_block_id}",
    operation_id = "patchTextBlock",
    tag = "text-blocks",
    params(
        ("document_id" = String, Path,),
        ("text_block_id" = String, Path,),
    ),
    request_body = TextBlockPatch,
    responses(
        (status = 200, body = TextBlockDetail),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn patch_text_block(
    State(state): State<ApiState>,
    Path((document_id, text_block_id)): Path<(String, String)>,
    Json(request): Json<TextBlockPatch>,
) -> ApiResult<Json<TextBlockDetail>> {
    let resources = state.resources()?;
    let mut detail = None;
    let mut needs_render = false;
    let mut block_index = None;
    resources
        .storage
        .update_page(&document_id, |doc| {
            if let Some((idx, block)) = doc
                .text_blocks
                .iter_mut()
                .enumerate()
                .find(|(_, b)| b.id == text_block_id)
            {
                needs_render = apply_text_block_patch(block, request);
                detail = Some(TextBlockDetail::from(&*block));
                block_index = Some(idx);
            }
        })
        .await
        .map_err(ApiError::from)?;

    if needs_render && let Some(idx) = block_index {
        engine::render_document(&resources, &document_id, Some(idx), None, None, None)
            .await
            .map_err(ApiError::internal)?;
    }

    detail
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("Text block not found: {text_block_id}")))
}

#[utoipa::path(
    delete,
    path = "/documents/{document_id}/text-blocks/{text_block_id}",
    operation_id = "deleteTextBlock",
    tag = "text-blocks",
    params(
        ("document_id" = String, Path,),
        ("text_block_id" = String, Path,),
    ),
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn delete_text_block(
    State(state): State<ApiState>,
    Path((document_id, text_block_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let mut found = false;
    resources
        .storage
        .update_page(&document_id, |doc| {
            if let Some(idx) = doc.text_blocks.iter().position(|b| b.id == text_block_id) {
                doc.text_blocks.remove(idx);
                found = true;
            }
        })
        .await
        .map_err(ApiError::from)?;

    if !found {
        return Err(ApiError::not_found(format!(
            "Text block not found: {text_block_id}"
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// LLM
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/llm/catalog",
    operation_id = "getLlmCatalog",
    tag = "llm",
    responses(
        (status = 200, body = LlmCatalog),
        (status = 503, body = ApiError),
    ),
)]
async fn get_llm_catalog(State(state): State<ApiState>) -> ApiResult<Json<LlmCatalog>> {
    let resources = state.resources()?;
    Ok(Json(llm::llm_catalog(resources).await?))
}

#[utoipa::path(
    get,
    path = "/llm",
    operation_id = "getLlm",
    tag = "llm",
    responses(
        (status = 200, body = LlmState),
        (status = 503, body = ApiError),
    ),
)]
async fn get_llm(State(state): State<ApiState>) -> ApiResult<Json<LlmState>> {
    let resources = state.resources()?;
    Ok(Json(resources.llm.snapshot().await))
}

#[utoipa::path(
    put,
    path = "/llm",
    operation_id = "loadLlm",
    tag = "llm",
    request_body = LlmLoadRequest,
    responses(
        (status = 200, body = LlmState),
        (status = 400, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn load_llm(
    State(state): State<ApiState>,
    Json(request): Json<LlmLoadRequest>,
) -> ApiResult<Json<LlmState>> {
    let resources = state.resources()?;
    llm::llm_load(resources.clone(), request).await?;
    Ok(Json(resources.llm.snapshot().await))
}

#[utoipa::path(
    delete,
    path = "/llm",
    operation_id = "unloadLlm",
    tag = "llm",
    responses(
        (status = 200, body = LlmState),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn unload_llm(State(state): State<ApiState>) -> ApiResult<Json<LlmState>> {
    let resources = state.resources()?;
    llm::llm_offload(resources.clone()).await?;
    Ok(Json(resources.llm.snapshot().await))
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/jobs/pipeline",
    operation_id = "startPipeline",
    tag = "jobs",
    request_body = PipelineJobRequest,
    responses(
        (status = 200, body = JobState),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn start_pipeline(
    State(state): State<ApiState>,
    Json(request): Json<PipelineJobRequest>,
) -> ApiResult<Json<JobState>> {
    let resources = state.resources()?;
    // Validate document_id exists if provided
    if let Some(document_id) = request.document_id.as_deref() {
        resources
            .storage
            .page(document_id)
            .await
            .map_err(|_| ApiError::not_found(format!("Document not found: {document_id}")))?;
    }
    let job_id = pipeline::process(
        resources.clone(),
        koharu_core::ProcessRequest {
            document_id: request.document_id.clone(),
            llm: request.llm.clone(),
            language: request.language,
            system_prompt: request.system_prompt,
            shader_effect: request.shader_effect,
            shader_stroke: request.shader_stroke,
            default_font: request.default_font,
        },
        state.tracker.jobs(),
    )
    .await?;

    let job = state
        .tracker
        .get_job(&job_id)
        .await
        .ok_or_else(|| ApiError::internal(anyhow::anyhow!("Job not found after creation")))?;

    Ok(Json(job))
}

#[utoipa::path(
    get,
    path = "/jobs",
    operation_id = "listJobs",
    tag = "jobs",
    responses(
        (status = 200, body = Vec<JobState>),
    ),
)]
async fn list_jobs(State(state): State<ApiState>) -> Json<Vec<JobState>> {
    Json(state.tracker.list_jobs().await)
}

#[utoipa::path(
    get,
    path = "/jobs/{job_id}",
    operation_id = "getJob",
    tag = "jobs",
    params(("job_id" = String, Path,)),
    responses(
        (status = 200, body = JobState),
        (status = 404, body = ApiError),
    ),
)]
async fn get_job(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> ApiResult<Json<JobState>> {
    state
        .tracker
        .get_job(&job_id)
        .await
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("Job not found: {job_id}")))
}

#[utoipa::path(
    delete,
    path = "/jobs/{job_id}",
    operation_id = "cancelJob",
    tag = "jobs",
    params(("job_id" = String, Path,)),
    responses(
        (status = 204),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn cancel_job(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let guard = resources.pipeline.read().await;
    let handle = guard
        .as_ref()
        .ok_or_else(|| ApiError::not_found("Pipeline job not found"))?;
    if handle.id != job_id {
        return Err(ApiError::not_found(format!(
            "Pipeline job not found: {job_id}"
        )));
    }
    handle
        .cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/downloads",
    operation_id = "listDownloads",
    tag = "downloads",
    responses(
        (status = 200, body = Vec<DownloadState>),
    ),
)]
async fn list_downloads(State(state): State<ApiState>) -> Json<Vec<DownloadState>> {
    Json(state.tracker.list_downloads().await)
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/documents/{document_id}/export/{format}",
    operation_id = "exportDocument",
    tag = "exports",
    params(
        ("document_id" = String, Path,),
        ("format" = String, Path,),
        ExportQuery,
    ),
    responses(
        (status = 200, content_type = "application/octet-stream", body = inline(String)),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn export_document(
    State(state): State<ApiState>,
    Path((document_id, format)): Path<(String, String)>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let document = find_document(&resources, &document_id).await?;

    if format == "psd" {
        let source_img = resources
            .storage
            .images
            .load(&document.source)
            .map_err(ApiError::internal)?;
        let segment_img = document
            .segment
            .as_ref()
            .map(|r| resources.storage.images.load(r))
            .transpose()
            .map_err(ApiError::internal)?;
        let inpainted_img = document
            .inpainted
            .as_ref()
            .map(|r| resources.storage.images.load(r))
            .transpose()
            .map_err(ApiError::internal)?;
        let rendered_img = document
            .rendered
            .as_ref()
            .map(|r| resources.storage.images.load(r))
            .transpose()
            .map_err(ApiError::internal)?;
        let brush_layer_img = document
            .brush_layer
            .as_ref()
            .map(|r| resources.storage.images.load(r))
            .transpose()
            .map_err(ApiError::internal)?;

        // Pre-resolve text block rendered images
        let block_rendered: std::collections::HashMap<koharu_core::BlobRef, image::DynamicImage> =
            document
                .text_blocks
                .iter()
                .filter_map(|b| b.rendered.as_ref())
                .filter_map(|r| {
                    resources
                        .storage
                        .images
                        .load(r)
                        .ok()
                        .map(|img| (r.clone(), img))
                })
                .collect();

        let resolved = koharu_psd::ResolvedDocument {
            document: &document,
            source: &source_img,
            segment: segment_img.as_ref(),
            inpainted: inpainted_img.as_ref(),
            rendered: rendered_img.as_ref(),
            brush_layer: brush_layer_img.as_ref(),
            block_images: &block_rendered,
        };

        let data = koharu_psd::export_document(
            &resolved,
            &PsdExportOptions {
                text_layer_mode: TextLayerMode::Editable,
                ..PsdExportOptions::default()
            },
        )
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
        return Ok(binary_response(
            data,
            "image/vnd.adobe.photoshop",
            Some(format!("{}_koharu.psd", document.name)),
        ));
    }

    let layer = query.layer.unwrap_or(ExportLayer::Rendered);
    let (blob_ref, filename) = export_layer_ref(&document, layer)?;
    let data = if resources.storage.images.is_raw_rgba(blob_ref) {
        let img = resources
            .storage
            .images
            .load(blob_ref)
            .map_err(ApiError::internal)?;
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| ApiError::internal(e.into()))?;
        buf.into_inner()
    } else {
        resources
            .storage
            .images
            .load_bytes(blob_ref)
            .map_err(ApiError::internal)?
    };
    let mime = if data.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "image/webp"
    };
    Ok(binary_response(data, mime, Some(filename)))
}

#[utoipa::path(
    post,
    path = "/exports",
    operation_id = "batchExport",
    tag = "exports",
    request_body = ExportBatchRequest,
    responses(
        (status = 200, body = ExportResult),
        (status = 503, body = ApiError),
    ),
)]
#[tracing::instrument(level = "info", skip_all)]
async fn batch_export(
    State(state): State<ApiState>,
    Json(request): Json<ExportBatchRequest>,
) -> ApiResult<Json<ExportResult>> {
    let resources = state.resources()?;
    let count = match request.layer.unwrap_or(ExportLayer::Rendered) {
        ExportLayer::Rendered => io::export_all_rendered(resources).await?,
        ExportLayer::Inpainted => io::export_all_inpainted(resources).await?,
    };
    Ok(Json(ExportResult { count }))
}

async fn find_document(resources: &AppResources, document_id: &str) -> ApiResult<Document> {
    resources
        .storage
        .page(document_id)
        .await
        .map_err(|_| ApiError::not_found(format!("Document not found: {document_id}")))
}

fn find_text_block_index(document: &Document, text_block_id: &str) -> ApiResult<usize> {
    document
        .text_blocks
        .iter()
        .position(|block| block.id == text_block_id)
        .ok_or_else(|| ApiError::not_found(format!("Text block not found: {text_block_id}")))
}

fn binary_response(data: Vec<u8>, content_type: &str, filename: Option<String>) -> Response {
    let mut response = Response::new(Body::from(data));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_str(content_type).unwrap());
    if let Some(filename) = filename
        && let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
    {
        response.headers_mut().insert(CONTENT_DISPOSITION, value);
    }
    response
}

fn export_layer_ref(
    document: &Document,
    layer: ExportLayer,
) -> ApiResult<(&koharu_core::BlobRef, String)> {
    match layer {
        ExportLayer::Rendered => {
            let r = document
                .rendered
                .as_ref()
                .ok_or_else(|| ApiError::not_found("No rendered image found"))?;
            Ok((r, format!("{}_koharu.png", document.name)))
        }
        ExportLayer::Inpainted => {
            let r = document
                .inpainted
                .as_ref()
                .ok_or_else(|| ApiError::not_found("No inpainted image found"))?;
            Ok((r, format!("{}_inpainted.png", document.name)))
        }
    }
}

#[utoipa::path(
    get,
    path = "/blobs/{hash}",
    operation_id = "getBlob",
    tag = "blobs",
    params(("hash" = String, Path,)),
    responses(
        (status = 200, content_type = "application/octet-stream", body = inline(Vec<u8>)),
        (status = 404, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn get_blob(State(state): State<ApiState>, Path(hash): Path<String>) -> ApiResult<Response> {
    let resources = state.resources()?;
    let blob_ref = koharu_core::BlobRef::new(hash);
    let data = resources
        .storage
        .images
        .load_bytes(&blob_ref)
        .map_err(|_| ApiError::not_found("Blob not found"))?;
    Ok(binary_response(data, "application/octet-stream", None))
}

/// Returns `true` if the patch invalidated the rendered sprite.
fn apply_text_block_patch(block: &mut TextBlock, patch: TextBlockPatch) -> bool {
    let previous_width = block.width;
    let previous_height = block.height;
    let mut invalidate_render = false;

    if let Some(text) = patch.text {
        block.text = Some(text);
        invalidate_render = true;
    }
    if let Some(translation) = patch.translation {
        block.translation = Some(translation);
        invalidate_render = true;
    }
    if let Some(x) = patch.x {
        block.x = x;
        invalidate_render = true;
    }
    if let Some(y) = patch.y {
        block.y = y;
        invalidate_render = true;
    }
    if let Some(width) = patch.width {
        block.width = width;
        invalidate_render = true;
    }
    if let Some(height) = patch.height {
        block.height = height;
        invalidate_render = true;
    }
    if let Some(style) = patch.style {
        block.style = Some(style);
        invalidate_render = true;
    }

    if (previous_width - block.width).abs() > f32::EPSILON
        || (previous_height - block.height).abs() > f32::EPSILON
    {
        block.lock_layout_box = true;
    }
    invalidate_render
}
