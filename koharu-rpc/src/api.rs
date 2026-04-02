use std::io::Cursor;

use anyhow::Context;
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
use image::ImageFormat;
use koharu_app::{
    AppResources, operations,
    state_tx::{self, ChangedField},
};
use koharu_core::{
    ApiKeyResponse, ApiKeyValue, BootstrapConfig, CreateTextBlock, Document, DocumentDetail,
    DocumentSummary, DownloadState, ExportLayer, ExportResult, FontFaceInfo, InpaintRegion,
    JobState, JobStatus, LlmLoadRequest, LlmModelInfo, LlmPingRequest, LlmPingResponse,
    MaskRegionRequest, MetaInfo, PipelineJobRequest, Region, RenderRequest, TextBlock,
    TextBlockDetail, TextBlockInput, TextBlockPatch, TranslateRequest,
};
use koharu_psd::{PsdExportOptions, TextLayerMode};
use serde::{Deserialize, Serialize};
use utoipa::IntoParams;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    shared::{SharedState, get_resources},
    tracker::Tracker,
};

const MAX_BODY_SIZE: usize = 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct ApiState {
    pub resources: SharedState,
    pub tracker: Tracker,
}

impl ApiState {
    fn resources(&self) -> ApiResult<AppResources> {
        get_resources(&self.resources).map_err(ApiError::service_unavailable)
    }
}

pub fn api() -> (axum::Router<ApiState>, utoipa::openapi::OpenApi) {
    OpenApiRouter::default()
        .routes(routes!(list_documents, import_documents))
        .routes(routes!(get_document))
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
        .routes(routes!(list_llm_models))
        .routes(routes!(check_llm_health))
        .routes(routes!(get_api_key, set_api_key))
        .routes(routes!(start_pipeline))
        .routes(routes!(list_jobs))
        .routes(routes!(get_job, cancel_job))
        .routes(routes!(list_downloads))
        .routes(routes!(get_meta))
        .routes(routes!(list_fonts))
        .routes(routes!(get_config, update_config))
        .routes(routes!(initialize))
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

    fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
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
        let status =
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
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
struct LlmModelsQuery {
    language: Option<String>,
    openai_compatible_base_url: Option<String>,
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
        (status = 200, body = BootstrapConfig),
        (status = 503, body = ApiError),
    ),
)]
async fn get_config(State(state): State<ApiState>) -> ApiResult<Json<BootstrapConfig>> {
    let config = state.resources.get_config().map_err(ApiError::internal)?;
    Ok(Json(config))
}

#[utoipa::path(
    put,
    path = "/config",
    operation_id = "updateConfig",
    tag = "system",
    request_body = BootstrapConfig,
    responses(
        (status = 200, body = BootstrapConfig),
        (status = 400, body = ApiError),
    ),
)]
async fn update_config(
    State(state): State<ApiState>,
    Json(config): Json<BootstrapConfig>,
) -> ApiResult<Json<BootstrapConfig>> {
    let saved = state.resources.put_config(config).map_err(ApiError::from)?;
    Ok(Json(saved))
}

#[utoipa::path(
    post,
    path = "/initialize",
    operation_id = "initialize",
    tag = "system",
    responses(
        (status = 204),
        (status = 409, body = ApiError),
        (status = 500, body = ApiError),
    ),
)]
async fn initialize(State(state): State<ApiState>) -> ApiResult<StatusCode> {
    match state.resources.initialize().await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(error) => {
            let message = error.to_string();
            if message.contains("already in progress") {
                Err(ApiError::conflict(message))
            } else {
                Err(ApiError::internal(error))
            }
        }
    }
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
    let device = operations::device(resources.clone()).await?;
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
    let fonts = operations::list_font_families(resources)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(fonts))
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
    let guard = resources.state.read().await;
    let documents = guard.documents.iter().map(DocumentSummary::from).collect();
    Ok(Json(documents))
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
async fn get_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<Json<DocumentDetail>> {
    let resources = state.resources()?;
    let guard = resources.state.read().await;
    let doc = guard
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or_else(|| ApiError::not_found("Document not found"))?;

    let image = serde_bytes::ByteBuf::from(encode_webp(&doc.image)?);
    let segment = doc.segment.as_ref().map(|s| encode_webp(s).map(serde_bytes::ByteBuf::from)).transpose()?;
    let inpainted = doc.inpainted.as_ref().map(|s| encode_webp(s).map(serde_bytes::ByteBuf::from)).transpose()?;
    let brush_layer = doc.brush_layer.as_ref().map(|s| encode_webp(s).map(serde_bytes::ByteBuf::from)).transpose()?;
    let rendered = doc.rendered.as_ref().map(|s| encode_webp(s).map(serde_bytes::ByteBuf::from)).transpose()?;

    let text_blocks = doc.text_blocks.iter().map(koharu_core::TextBlockDetail::from).collect();

    let detail = DocumentDetail {
        id: doc.id.clone(),
        path: doc.path.to_string_lossy().to_string(),
        name: doc.name.clone(),
        width: doc.width,
        height: doc.height,
        revision: doc.revision,
        text_blocks,
        image,
        segment,
        inpainted,
        brush_layer,
        rendered,
    };

    drop(guard);
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
    let guard = resources.state.read().await;
    let doc = guard
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or_else(|| ApiError::not_found("Document not found"))?;
    let source = doc.rendered.as_ref().unwrap_or(&doc.image);
    let thumbnail = source.thumbnail(size, size);
    let bytes = encode_webp(&thumbnail.into())?;
    drop(guard);
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
async fn import_documents(
    State(state): State<ApiState>,
    Query(query): Query<ImportQuery>,
    mut multipart: Multipart,
) -> ApiResult<Json<koharu_core::ImportResult>> {
    let resources = state.resources()?;
    let mut files = Vec::new();

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
        files.push(koharu_core::FileEntry {
            name: filename,
            data: data.to_vec(),
        });
    }

    if files.is_empty() {
        return Err(ApiError::bad_request("No files uploaded"));
    }

    let payload = koharu_core::OpenDocumentsPayload { files };
    match query.mode.unwrap_or(koharu_core::ImportMode::Replace) {
        koharu_core::ImportMode::Replace => {
            operations::open_documents(resources.clone(), payload).await?;
        }
        koharu_core::ImportMode::Append => {
            operations::add_documents(resources.clone(), payload).await?;
        }
    }

    let documents = state_tx::list_docs(&resources.state)
        .await
        .iter()
        .map(DocumentSummary::from)
        .collect::<Vec<_>>();

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
async fn detect_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::detect(resources, koharu_core::IndexPayload { index }).await?;
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
async fn recognize_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::ocr(resources, koharu_core::IndexPayload { index }).await?;
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
async fn inpaint_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::inpaint(resources, koharu_core::IndexPayload { index }).await?;
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
async fn render_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<RenderRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, document) = find_document(&resources, &document_id).await?;
    let text_block_index = request
        .text_block_id
        .as_deref()
        .map(|id| find_text_block_index(&document, id))
        .transpose()?;

    operations::render(
        resources,
        koharu_core::RenderPayload {
            index,
            text_block_index,
            shader_effect: request.shader_effect,
            shader_stroke: request.shader_stroke,
            font_family: request.font_family,
        },
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
async fn translate_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<TranslateRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, document) = find_document(&resources, &document_id).await?;
    let text_block_index = request
        .text_block_id
        .as_deref()
        .map(|id| find_text_block_index(&document, id))
        .transpose()?;

    operations::llm_generate(
        resources,
        koharu_core::LlmGeneratePayload {
            index,
            text_block_index,
            language: request.language,
        },
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
async fn update_mask(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<MaskRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::update_inpaint_mask(
        resources,
        koharu_core::UpdateInpaintMaskPayload {
            index,
            mask: request.data,
            region: request.region.map(to_inpaint_region),
        },
    )
    .await?;
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
async fn update_brush_layer(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_core::BrushRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::update_brush_layer(
        resources,
        koharu_core::UpdateBrushLayerPayload {
            index,
            patch: request.data,
            region: to_inpaint_region(request.region),
        },
    )
    .await?;
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
async fn inpaint_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_core::InpaintRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::inpaint_partial(
        resources,
        koharu_core::InpaintPartialPayload {
            index,
            region: to_inpaint_region(request.region),
        },
    )
    .await?;
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
async fn create_text_block(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<CreateTextBlock>,
) -> ApiResult<Json<TextBlockDetail>> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;

    let detail = state_tx::mutate_doc(
        &resources.state,
        index,
        &[ChangedField::TextBlocks],
        |document| {
            let mut block = TextBlock {
                x: request.x,
                y: request.y,
                width: request.width,
                height: request.height,
                confidence: 1.0,
                ..Default::default()
            };
            block.set_layout_seed(block.x, block.y, block.width, block.height);
            document.text_blocks.push(block);
            let block = document
                .text_blocks
                .last()
                .ok_or_else(|| anyhow::anyhow!("Failed to append text block"))?;
            Ok(TextBlockDetail::from(block))
        },
    )
    .await?;

    Ok(Json(detail))
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
async fn put_text_blocks(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(inputs): Json<Vec<TextBlockInput>>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;

    let any_content_changed = state_tx::mutate_doc(
        &resources.state,
        index,
        &[ChangedField::TextBlocks],
        |document| {
            let mut any_changed = false;

            // Build a set of incoming IDs for deletion detection
            let incoming_ids: std::collections::HashSet<&str> = inputs
                .iter()
                .filter_map(|input| input.id.as_deref())
                .collect();

            // Delete blocks not present in the incoming array
            let before_len = document.text_blocks.len();
            document
                .text_blocks
                .retain(|block| incoming_ids.contains(block.id.as_str()));
            if document.text_blocks.len() != before_len {
                any_changed = true;
            }

            for input in &inputs {
                if let Some(ref id) = input.id {
                    // Update existing block
                    if let Some(block) = document
                        .text_blocks
                        .iter_mut()
                        .find(|b| &b.id == id)
                    {
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
                        // Content changed if the render was invalidated
                        if had_render && block.rendered.is_none() {
                            any_changed = true;
                        }
                    }
                } else {
                    // Create new block
                    let mut block = TextBlock {
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
                    block.set_layout_seed(block.x, block.y, block.width, block.height);
                    document.text_blocks.push(block);
                    any_changed = true;
                }
            }

            Ok(any_changed)
        },
    )
    .await?;

    // Auto-render if any block content/geometry changed
    if any_content_changed {
        let _ = operations::render(
            resources,
            koharu_core::RenderPayload {
                index,
                text_block_index: None,
                shader_effect: None,
                shader_stroke: None,
                font_family: None,
            },
        )
        .await;
    }

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
async fn patch_text_block(
    State(state): State<ApiState>,
    Path((document_id, text_block_id)): Path<(String, String)>,
    Json(request): Json<TextBlockPatch>,
) -> ApiResult<Json<TextBlockDetail>> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;

    let detail = state_tx::mutate_doc(
        &resources.state,
        index,
        &[ChangedField::TextBlocks],
        |document| {
            let block = document
                .text_blocks
                .iter_mut()
                .find(|block| block.id == text_block_id)
                .ok_or_else(|| anyhow::anyhow!("Text block not found: {text_block_id}"))?;
            apply_text_block_patch(block, request.clone());
            Ok(TextBlockDetail::from(&*block))
        },
    )
    .await?;

    Ok(Json(detail))
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
async fn delete_text_block(
    State(state): State<ApiState>,
    Path((document_id, text_block_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;

    state_tx::mutate_doc(
        &resources.state,
        index,
        &[ChangedField::TextBlocks],
        |document| {
            let block_index = document
                .text_blocks
                .iter()
                .position(|block| block.id == text_block_id)
                .ok_or_else(|| anyhow::anyhow!("Text block not found: {text_block_id}"))?;
            document.text_blocks.remove(block_index);
            Ok(())
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// LLM
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/llm/models",
    operation_id = "listLlmModels",
    tag = "llm",
    params(LlmModelsQuery),
    responses(
        (status = 200, body = Vec<LlmModelInfo>),
        (status = 503, body = ApiError),
    ),
)]
async fn list_llm_models(
    State(state): State<ApiState>,
    Query(query): Query<LlmModelsQuery>,
) -> ApiResult<Json<Vec<LlmModelInfo>>> {
    let resources = state.resources()?;
    let models = operations::llm_list(
        resources,
        koharu_core::LlmListPayload {
            language: query.language,
            openai_compatible_base_url: query.openai_compatible_base_url,
        },
    )
    .await?
    .into_iter()
    .map(|model| LlmModelInfo {
        id: model.id,
        languages: model.languages,
        source: model.source.to_string(),
    })
    .collect();
    Ok(Json(models))
}

#[utoipa::path(
    get,
    path = "/llm",
    operation_id = "getLlm",
    tag = "llm",
    responses(
        (status = 200, body = koharu_core::LlmState),
        (status = 503, body = ApiError),
    ),
)]
async fn get_llm(State(state): State<ApiState>) -> ApiResult<Json<koharu_core::LlmState>> {
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
        (status = 200, body = koharu_core::LlmState),
        (status = 400, body = ApiError),
        (status = 503, body = ApiError),
    ),
)]
async fn load_llm(
    State(state): State<ApiState>,
    Json(request): Json<LlmLoadRequest>,
) -> ApiResult<Json<koharu_core::LlmState>> {
    let resources = state.resources()?;
    operations::llm_load(
        resources.clone(),
        koharu_core::LlmLoadPayload {
            id: request.id,
            api_key: request.api_key,
            base_url: request.base_url,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            custom_system_prompt: request.custom_system_prompt,
        },
    )
    .await?;
    Ok(Json(resources.llm.snapshot().await))
}

#[utoipa::path(
    delete,
    path = "/llm",
    operation_id = "unloadLlm",
    tag = "llm",
    responses(
        (status = 200, body = koharu_core::LlmState),
        (status = 503, body = ApiError),
    ),
)]
async fn unload_llm(State(state): State<ApiState>) -> ApiResult<Json<koharu_core::LlmState>> {
    let resources = state.resources()?;
    operations::llm_offload(resources.clone()).await?;
    Ok(Json(resources.llm.snapshot().await))
}

#[utoipa::path(
    post,
    path = "/llm/health",
    operation_id = "checkLlmHealth",
    tag = "llm",
    request_body = LlmPingRequest,
    responses(
        (status = 200, body = LlmPingResponse),
    ),
)]
async fn check_llm_health(
    State(state): State<ApiState>,
    Json(request): Json<LlmPingRequest>,
) -> ApiResult<Json<LlmPingResponse>> {
    match operations::llm_ping(
        state.resources.runtime().http_client(),
        &request.base_url,
        request.api_key.as_deref(),
    )
    .await
    {
        Ok(result) => Ok(Json(LlmPingResponse {
            ok: true,
            models: result.models,
            latency_ms: Some(result.latency_ms),
            error: None,
        })),
        Err(err) => Ok(Json(LlmPingResponse {
            ok: false,
            models: vec![],
            latency_ms: None,
            error: Some(err.to_string()),
        })),
    }
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/providers/{provider}/api-key",
    operation_id = "getApiKey",
    tag = "providers",
    params(("provider" = String, Path,)),
    responses(
        (status = 200, body = ApiKeyResponse),
        (status = 503, body = ApiError),
    ),
)]
async fn get_api_key(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
) -> ApiResult<Json<ApiKeyResponse>> {
    let resources = state.resources()?;
    let result =
        operations::get_api_key(resources, koharu_core::ApiKeyGetPayload { provider }).await?;
    Ok(Json(ApiKeyResponse {
        api_key: result.api_key,
    }))
}

#[utoipa::path(
    put,
    path = "/providers/{provider}/api-key",
    operation_id = "setApiKey",
    tag = "providers",
    params(("provider" = String, Path,)),
    request_body = ApiKeyValue,
    responses(
        (status = 204),
        (status = 503, body = ApiError),
    ),
)]
async fn set_api_key(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
    Json(request): Json<ApiKeyValue>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    operations::set_api_key(
        resources,
        koharu_core::ApiKeySetPayload {
            provider,
            api_key: request.api_key,
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
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
async fn start_pipeline(
    State(state): State<ApiState>,
    Json(request): Json<PipelineJobRequest>,
) -> ApiResult<Json<JobState>> {
    let resources = state.resources()?;
    let index = if let Some(document_id) = request.document_id.as_deref() {
        Some(state_tx::find_doc_index(&resources.state, document_id).await?)
    } else {
        None
    };
    let total_documents = match index {
        Some(_) => 1,
        None => state_tx::doc_count(&resources.state).await,
    };

    let job_id = operations::process(
        resources.clone(),
        koharu_core::ProcessRequest {
            index,
            llm_model_id: request.llm_model_id,
            llm_api_key: request.llm_api_key,
            llm_base_url: request.llm_base_url,
            llm_temperature: request.llm_temperature,
            llm_max_tokens: request.llm_max_tokens,
            llm_custom_system_prompt: request.llm_custom_system_prompt,
            language: request.language,
            shader_effect: request.shader_effect,
            shader_stroke: request.shader_stroke,
            font_family: request.font_family,
        },
    )
    .await?;

    let job = JobState {
        id: job_id,
        kind: "pipeline".to_string(),
        status: JobStatus::Running,
        step: None,
        current_document: 0,
        total_documents,
        current_step_index: 0,
        total_steps: koharu_core::PipelineStep::ALL.len(),
        overall_percent: 0,
        error: None,
    };
    state.tracker.publish_job(job.clone()).await;

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
async fn export_document(
    State(state): State<ApiState>,
    Path((document_id, format)): Path<(String, String)>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let (_, document) = find_document(&resources, &document_id).await?;

    if format == "psd" {
        let data = koharu_psd::export_document(&document, &app_psd_export_options())
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        return Ok(binary_response(
            data,
            "image/vnd.adobe.photoshop",
            Some(psd_export_filename(&document)),
        ));
    }

    let layer = query.layer.unwrap_or(ExportLayer::Rendered);
    let (image, filename) = export_target(&document, layer)?;
    let ext = document
        .path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let data = encode_image(image, &ext)?;
    let content_type = mime_from_ext(&ext);
    Ok(binary_response(data, content_type, Some(filename)))
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
async fn batch_export(
    State(state): State<ApiState>,
    Json(request): Json<ExportBatchRequest>,
) -> ApiResult<Json<ExportResult>> {
    let resources = state.resources()?;
    let count = match request.layer.unwrap_or(ExportLayer::Rendered) {
        ExportLayer::Rendered => operations::export_all_rendered(resources).await?,
        ExportLayer::Inpainted => operations::export_all_inpainted(resources).await?,
    };
    Ok(Json(ExportResult { count }))
}

async fn find_document(
    resources: &AppResources,
    document_id: &str,
) -> ApiResult<(usize, Document)> {
    let index = state_tx::find_doc_index(&resources.state, document_id)
        .await
        .map_err(ApiError::from)?;
    let document = state_tx::read_doc(&resources.state, index)
        .await
        .map_err(ApiError::from)?;
    Ok((index, document))
}

fn find_text_block_index(document: &Document, text_block_id: &str) -> ApiResult<usize> {
    document
        .text_blocks
        .iter()
        .position(|block| block.id == text_block_id)
        .ok_or_else(|| ApiError::not_found(format!("Text block not found: {text_block_id}")))
}

fn encode_webp(image: &koharu_core::SerializableDynamicImage) -> ApiResult<Vec<u8>> {
    encode_image(image, "webp")
}

fn encode_image(image: &koharu_core::SerializableDynamicImage, ext: &str) -> ApiResult<Vec<u8>> {
    let format = ImageFormat::from_extension(ext).unwrap_or(ImageFormat::Jpeg);
    let mut cursor = Cursor::new(Vec::new());
    image
        .0
        .write_to(&mut cursor, format)
        .with_context(|| format!("failed to encode image as {ext}"))
        .map_err(ApiError::internal)?;
    Ok(cursor.into_inner())
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

fn mime_from_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn export_target(
    document: &Document,
    layer: ExportLayer,
) -> ApiResult<(&koharu_core::SerializableDynamicImage, String)> {
    let ext = document
        .path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();

    match layer {
        ExportLayer::Rendered => {
            let image = document
                .rendered
                .as_ref()
                .ok_or_else(|| ApiError::not_found("No rendered image found"))?;
            Ok((image, format!("{}_koharu.{ext}", document.name)))
        }
        ExportLayer::Inpainted => {
            let image = document
                .inpainted
                .as_ref()
                .ok_or_else(|| ApiError::not_found("No inpainted image found"))?;
            Ok((image, format!("{}_inpainted.{ext}", document.name)))
        }
    }
}

fn psd_export_filename(document: &Document) -> String {
    format!("{}_koharu.psd", document.name)
}

fn app_psd_export_options() -> PsdExportOptions {
    PsdExportOptions {
        text_layer_mode: TextLayerMode::Editable,
        ..PsdExportOptions::default()
    }
}

fn to_inpaint_region(region: Region) -> InpaintRegion {
    InpaintRegion {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
    }
}

fn apply_text_block_patch(block: &mut TextBlock, patch: TextBlockPatch) {
    let previous_width = block.width;
    let previous_height = block.height;
    let mut geometry_changed = false;
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
        if (block.x - x).abs() > f32::EPSILON {
            geometry_changed = true;
        }
        block.x = x;
        invalidate_render = true;
    }
    if let Some(y) = patch.y {
        if (block.y - y).abs() > f32::EPSILON {
            geometry_changed = true;
        }
        block.y = y;
        invalidate_render = true;
    }
    if let Some(width) = patch.width {
        if (block.width - width).abs() > f32::EPSILON {
            geometry_changed = true;
        }
        block.width = width;
        invalidate_render = true;
    }
    if let Some(height) = patch.height {
        if (block.height - height).abs() > f32::EPSILON {
            geometry_changed = true;
        }
        block.height = height;
        invalidate_render = true;
    }
    if let Some(style) = patch.style {
        block.style = Some(style);
        invalidate_render = true;
    }

    if geometry_changed {
        block.set_layout_seed(block.x, block.y, block.width, block.height);
    }
    if (previous_width - block.width).abs() > f32::EPSILON
        || (previous_height - block.height).abs() > f32::EPSILON
    {
        block.lock_layout_box = true;
    }
    if invalidate_render {
        block.rendered = None;
        block.rendered_direction = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{app_psd_export_options, apply_text_block_patch, psd_export_filename};
    use koharu_core::{Document, TextAlign, TextBlock, TextBlockPatch, TextDirection, TextStyle};
    use koharu_psd::TextLayerMode;

    #[test]
    fn text_block_patch_updates_geometry_and_clears_rendered() {
        let mut block = TextBlock {
            width: 100.0,
            height: 50.0,
            rendered_direction: Some(TextDirection::Vertical),
            rendered: Some(image::DynamicImage::new_rgba8(1, 1).into()),
            ..Default::default()
        };

        apply_text_block_patch(
            &mut block,
            TextBlockPatch {
                text: None,
                translation: Some("hello".to_string()),
                x: Some(12.0),
                y: Some(24.0),
                width: Some(80.0),
                height: Some(40.0),
                style: Some(TextStyle {
                    font_families: vec!["Noto Sans".to_string()],
                    font_size: Some(16.0),
                    color: [255, 255, 255, 255],
                    effect: None,
                    stroke: None,
                    text_align: Some(TextAlign::Center),
                }),
            },
        );

        assert_eq!(block.translation.as_deref(), Some("hello"));
        assert_eq!(block.x, 12.0);
        assert_eq!(block.y, 24.0);
        assert_eq!(block.width, 80.0);
        assert_eq!(block.height, 40.0);
        assert!(block.lock_layout_box);
        assert!(block.rendered.is_none());
        assert!(block.rendered_direction.is_none());
    }

    #[test]
    fn psd_export_filename_uses_koharu_suffix() {
        let document = Document {
            name: "chapter-01".to_string(),
            ..Default::default()
        };

        assert_eq!(psd_export_filename(&document), "chapter-01_koharu.psd");
    }

    #[test]
    fn app_psd_export_uses_editable_text_layers() {
        assert_eq!(
            app_psd_export_options().text_layer_mode,
            TextLayerMode::Editable
        );
    }
}
