use std::{convert::Infallible, io::Cursor, time::Duration};

use anyhow::Context;
use async_stream::stream;
use axum::{
    Json,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{
        HeaderValue, StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use image::ImageFormat;
use koharu_app::{
    AppResources, operations,
    state_tx::{self, ChangedField},
};
use koharu_core::{
    ApiKeyGetPayload, ApiKeyResponse, ApiKeySetPayload, ApiKeyValue, BootstrapConfig,
    CreateTextBlock, Document, DocumentResource, DocumentSummary, ExportLayer, ExportResult,
    FileEntry, FontFaceInfo, ImportMode, IndexPayload, InpaintPartialPayload, InpaintRegion,
    JobState, JobStatus, LlmLoadPayload, LlmLoadRequest, LlmModelInfo, LlmPingRequest,
    LlmPingResponse, MaskRegionRequest, MetaInfo, OpenDocumentsPayload, PipelineJobRequest, Region,
    RenderPayload, RenderRequest, SerializableDynamicImage, TextBlock, TextBlockDetail,
    TextBlockPatch, TranslateRequest, UpdateBrushLayerPayload, UpdateInpaintMaskPayload,
};
use koharu_psd::{PsdExportOptions, TextLayerMode};
use serde::{Deserialize, Serialize};
use utoipa::openapi::{
    SchemaFormat,
    schema::{Array, ArrayBuilder, KnownFormat, Object, ObjectBuilder, Type},
};
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    events::{ApiEvent, EventHub},
    shared::{SharedState, get_resources},
};

const MAX_BODY_SIZE: usize = 1024 * 1024 * 1024;
const API_V1_ROOT: &str = "/api/v1";

#[derive(Clone)]
pub struct ApiState {
    pub resources: SharedState,
    pub events: EventHub,
}

impl ApiState {
    fn resources(&self) -> ApiResult<AppResources> {
        get_resources(&self.resources).map_err(ApiError::service_unavailable)
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Koharu RPC API",
        version = env!("CARGO_PKG_VERSION"),
        description = "OpenAPI description for Koharu's HTTP RPC surface."
    ),
    components(schemas(DocumentLayer, ImportMode)),
    tags(
        (name = "system", description = "Bootstrap and runtime metadata endpoints"),
        (name = "documents", description = "Document import, inspection, editing, and export endpoints"),
        (name = "llm", description = "LLM discovery and runtime control endpoints"),
        (name = "providers", description = "Provider credential endpoints"),
        (name = "jobs", description = "Pipeline job lifecycle endpoints"),
        (name = "events", description = "Server-sent event stream endpoints")
    )
)]
struct ApiDoc;

pub fn openapi_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest(
        "/api/v1",
        OpenApiRouter::default()
            // system
            .routes(routes!(get_system_config, put_system_config))
            .routes(routes!(initialize_system))
            .routes(routes!(get_system_meta))
            .routes(routes!(list_fonts))
            // documents
            .routes(routes!(list_documents, import_documents))
            .routes(routes!(get_document))
            .routes(routes!(get_thumbnail))
            .routes(routes!(get_document_layer))
            .routes(routes!(detect_document))
            .routes(routes!(ocr_document))
            .routes(routes!(inpaint_document))
            .routes(routes!(render_document))
            .routes(routes!(translate_document))
            .routes(routes!(update_mask_region))
            .routes(routes!(update_brush_region))
            .routes(routes!(inpaint_region))
            .routes(routes!(create_text_block))
            .routes(routes!(patch_text_block, delete_text_block))
            .routes(routes!(export_document_image))
            .routes(routes!(export_document_psd))
            .routes(routes!(export_all))
            // llm
            .routes(routes!(list_llm_models))
            .routes(routes!(
                get_llm_runtime,
                load_llm_runtime,
                offload_llm_runtime
            ))
            .routes(routes!(ping_llm))
            // providers
            .routes(routes!(get_api_key, set_api_key))
            // jobs
            .routes(routes!(start_pipeline_job))
            .routes(routes!(cancel_pipeline_job))
            // events
            .routes(routes!(events_stream))
            .layer(DefaultBodyLimit::max(MAX_BODY_SIZE)),
    )
}

pub fn openapi_spec() -> utoipa::openapi::OpenApi {
    let (_, openapi) = openapi_router().split_for_parts();
    openapi
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    status: u16,
    message: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
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
        let body = Json(ErrorResponse {
            status: self.status.as_u16(),
            message: self.message,
        });
        (self.status, body).into_response()
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct ImportDocumentsQuery {
    mode: Option<ImportMode>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct ListLlmModelsQuery {
    language: Option<String>,
    openai_compatible_base_url: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct AssetRevisionQuery {
    revision: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
enum DocumentLayer {
    Original,
    Segment,
    Inpainted,
    Rendered,
    Brush,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ExportAllRequest {
    #[serde(default)]
    layer: Option<ExportLayer>,
}

#[allow(dead_code)]
#[derive(Debug, ToSchema)]
#[schema(value_type = String, format = Binary)]
struct BinaryBody(Vec<u8>);

#[allow(dead_code)]
#[derive(Debug, ToSchema)]
struct ImportDocumentsMultipart {
    #[schema(schema_with = multipart_files_schema)]
    files: Vec<Vec<u8>>,
}

fn binary_string_schema() -> Object {
    ObjectBuilder::new()
        .schema_type(Type::String)
        .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
        .build()
}

fn multipart_files_schema() -> Array {
    ArrayBuilder::new()
        .items(binary_string_schema())
        .min_items(Some(1))
        .build()
}

/// Fetch the persisted bootstrap configuration.
#[utoipa::path(
    get,
    operation_id = "getConfig",
    path = "/config",
    tag = "system",
    responses(
        (status = 200, body = BootstrapConfig),
        (status = 503, body = ErrorResponse)
    )
)]
async fn get_system_config(State(state): State<ApiState>) -> ApiResult<Json<BootstrapConfig>> {
    let config = state.resources.get_config().map_err(ApiError::internal)?;
    Ok(Json(config))
}

/// Persist a new bootstrap configuration.
#[utoipa::path(
    put,
    operation_id = "updateConfig",
    path = "/config",
    tag = "system",
    request_body = BootstrapConfig,
    responses(
        (status = 200, body = BootstrapConfig),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn put_system_config(
    State(state): State<ApiState>,
    Json(config): Json<BootstrapConfig>,
) -> ApiResult<Json<BootstrapConfig>> {
    let saved = state.resources.put_config(config).map_err(ApiError::from)?;
    Ok(Json(saved))
}

/// Trigger initialization of the runtime resources.
#[utoipa::path(
    post,
    operation_id = "initializeSystem",
    path = "/initialization",
    tag = "system",
    responses(
        (status = 204, description = "Initialization started"),
        (status = 409, body = ErrorResponse),
        (status = 500, body = ErrorResponse)
    )
)]
async fn initialize_system(State(state): State<ApiState>) -> ApiResult<StatusCode> {
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

/// Fetch runtime metadata for the active process.
#[utoipa::path(
    get,
    operation_id = "getMeta",
    path = "/meta",
    tag = "system",
    responses(
        (status = 200, body = MetaInfo),
        (status = 503, body = ErrorResponse)
    )
)]
async fn get_system_meta(State(state): State<ApiState>) -> ApiResult<Json<MetaInfo>> {
    let resources = state.resources()?;
    let device = operations::device(resources.clone()).await?;
    Ok(Json(MetaInfo {
        version: resources.version.to_string(),
        ml_device: device.ml_device,
    }))
}

/// List the available font families.
#[utoipa::path(
    get,
    operation_id = "listFonts",
    path = "/fonts",
    tag = "system",
    responses(
        (status = 200, body = [FontFaceInfo]),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn list_fonts(State(state): State<ApiState>) -> ApiResult<Json<Vec<FontFaceInfo>>> {
    let resources = state.resources()?;
    let fonts = operations::list_font_families(resources)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(fonts))
}

/// List all imported documents.
#[utoipa::path(
    get,
    operation_id = "listDocuments",
    path = "/documents",
    tag = "documents",
    responses((status = 200, body = [DocumentSummary]))
)]
async fn list_documents(State(state): State<ApiState>) -> ApiResult<Json<Vec<DocumentSummary>>> {
    let resources = state.resources()?;
    let guard = resources.state.read().await;
    let documents = guard
        .documents
        .iter()
        .map(|document| DocumentSummary::from_document(document, API_V1_ROOT))
        .collect();
    Ok(Json(documents))
}

/// Import one or more documents with multipart upload.
#[utoipa::path(
    post,
    operation_id = "importDocuments",
    path = "/documents",
    tag = "documents",
    params(ImportDocumentsQuery),
    request_body(
        content = inline(ImportDocumentsMultipart),
        content_type = "multipart/form-data"
    ),
    responses(
        (status = 200, body = koharu_core::ImportResult),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn import_documents(
    State(state): State<ApiState>,
    Query(query): Query<ImportDocumentsQuery>,
    mut multipart: Multipart,
) -> ApiResult<Json<koharu_core::ImportResult>> {
    let resources = state.resources()?;
    let mut files = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?
    {
        if !matches!(field.name(), Some("files")) {
            continue;
        }

        let filename = field
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "upload.bin".to_string());
        let data = field
            .bytes()
            .await
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        files.push(FileEntry {
            name: filename,
            data: data.to_vec(),
        });
    }

    if files.is_empty() {
        return Err(ApiError::bad_request(
            "No files uploaded under multipart field 'files'",
        ));
    }

    let payload = OpenDocumentsPayload { files };
    match query.mode.unwrap_or(ImportMode::Replace) {
        ImportMode::Replace => {
            operations::open_documents(resources.clone(), payload).await?;
        }
        ImportMode::Append => {
            operations::add_documents(resources.clone(), payload).await?;
        }
    }

    let documents = state_tx::list_docs(&resources.state)
        .await
        .iter()
        .map(|document| DocumentSummary::from_document(document, API_V1_ROOT))
        .collect::<Vec<_>>();

    Ok(Json(koharu_core::ImportResult {
        total_count: documents.len(),
        documents,
    }))
}

/// Fetch the current document detail.
#[utoipa::path(
    get,
    operation_id = "getDocument",
    path = "/documents/{document_id}",
    tag = "documents",
    responses(
        (status = 200, body = DocumentResource),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn get_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<Json<DocumentResource>> {
    let resources = state.resources()?;
    let guard = resources.state.read().await;
    let doc = guard
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or_else(|| ApiError::not_found("Document not found"))?;
    Ok(Json(DocumentResource::from_document(doc, API_V1_ROOT)))
}

/// Fetch a thumbnail preview for a document.
#[utoipa::path(
    get,
    operation_id = "getDocumentThumbnail",
    path = "/documents/{document_id}/thumbnail",
    tag = "documents",
    params(AssetRevisionQuery),
    responses(
        (
            status = 200,
            description = "Document thumbnail",
            body = inline(BinaryBody),
            content_type = "image/webp"
        ),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn get_thumbnail(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Query(cache): Query<AssetRevisionQuery>,
) -> ApiResult<Response> {
    let _ = cache.revision;
    let resources = state.resources()?;
    let guard = resources.state.read().await;
    let doc = guard
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or_else(|| ApiError::not_found("Document not found"))?;
    let source = doc.rendered.as_ref().unwrap_or(&doc.image);
    let thumbnail = source.thumbnail(200, 200);
    let bytes = encode_webp(&thumbnail.into())?;
    drop(guard);
    Ok(binary_response(bytes, "image/webp", None))
}

/// Fetch a named raster layer from a document.
#[utoipa::path(
    get,
    operation_id = "getDocumentLayer",
    path = "/documents/{document_id}/layers/{layer}",
    tag = "documents",
    params(AssetRevisionQuery),
    responses(
        (
            status = 200,
            description = "Requested document layer",
            body = inline(BinaryBody),
            content_type = "image/webp"
        ),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn get_document_layer(
    State(state): State<ApiState>,
    Path((document_id, layer)): Path<(String, DocumentLayer)>,
    Query(cache): Query<AssetRevisionQuery>,
) -> ApiResult<Response> {
    let _ = cache.revision;
    let resources = state.resources()?;
    let guard = resources.state.read().await;
    let doc = guard
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or_else(|| ApiError::not_found("Document not found"))?;
    let image = document_layer(doc, layer)?;
    let bytes = encode_webp(image)?;
    drop(guard);
    Ok(binary_response(bytes, "image/webp", None))
}

/// Trigger text detection for a document.
#[utoipa::path(
    post,
    operation_id = "detectDocument",
    path = "/documents/{document_id}/detection",
    tag = "documents",
    responses(
        (status = 204, description = "Detection completed"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn detect_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::detect(resources, IndexPayload { index }).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Trigger OCR for a document.
#[utoipa::path(
    post,
    operation_id = "ocrDocument",
    path = "/documents/{document_id}/ocr",
    tag = "documents",
    responses(
        (status = 204, description = "OCR completed"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn ocr_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::ocr(resources, IndexPayload { index }).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Trigger full-document inpainting.
#[utoipa::path(
    post,
    operation_id = "inpaintDocument",
    path = "/documents/{document_id}/inpainting",
    tag = "documents",
    responses(
        (status = 204, description = "Inpainting completed"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn inpaint_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::inpaint(resources, IndexPayload { index }).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Render translated text for a document.
#[utoipa::path(
    post,
    operation_id = "renderDocument",
    path = "/documents/{document_id}/rendering",
    tag = "documents",
    request_body = RenderRequest,
    responses(
        (status = 204, description = "Rendering completed"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
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
        RenderPayload {
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

/// Run translation for a document or a single text block.
#[utoipa::path(
    post,
    operation_id = "translateDocument",
    path = "/documents/{document_id}/translation",
    tag = "documents",
    request_body = TranslateRequest,
    responses(
        (status = 204, description = "Translation completed"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
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

/// Replace the inpainting mask for a document.
#[utoipa::path(
    put,
    operation_id = "updateDocumentInpaintingMask",
    path = "/documents/{document_id}/inpainting/mask",
    tag = "documents",
    request_body = MaskRegionRequest,
    responses(
        (status = 204, description = "Mask updated"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn update_mask_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<MaskRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::update_inpaint_mask(
        resources,
        UpdateInpaintMaskPayload {
            index,
            mask: request.data,
            region: request.region.map(to_inpaint_region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Apply a brush patch to a document.
#[utoipa::path(
    put,
    operation_id = "updateDocumentBrushRegion",
    path = "/documents/{document_id}/brush-layer/region",
    tag = "documents",
    request_body = koharu_core::BrushRegionRequest,
    responses(
        (status = 204, description = "Brush region updated"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn update_brush_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_core::BrushRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::update_brush_layer(
        resources,
        UpdateBrushLayerPayload {
            index,
            patch: request.data,
            region: to_inpaint_region(request.region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Inpaint a specific region of a document.
#[utoipa::path(
    post,
    operation_id = "inpaintDocumentRegion",
    path = "/documents/{document_id}/inpainting/region",
    tag = "documents",
    request_body = koharu_core::InpaintRegionRequest,
    responses(
        (status = 204, description = "Region inpainted"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
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
        InpaintPartialPayload {
            index,
            region: to_inpaint_region(request.region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Create a new text block on a document.
#[utoipa::path(
    post,
    operation_id = "createDocumentTextBlock",
    path = "/documents/{document_id}/text-blocks",
    tag = "documents",
    request_body = CreateTextBlock,
    responses(
        (status = 200, body = TextBlockDetail),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
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

/// Update a text block in place.
#[utoipa::path(
    patch,
    operation_id = "updateDocumentTextBlock",
    path = "/documents/{document_id}/text-blocks/{text_block_id}",
    tag = "documents",
    request_body = TextBlockPatch,
    responses(
        (status = 200, body = TextBlockDetail),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
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

/// Delete a text block from a document.
#[utoipa::path(
    delete,
    operation_id = "deleteDocumentTextBlock",
    path = "/documents/{document_id}/text-blocks/{text_block_id}",
    tag = "documents",
    responses(
        (status = 204, description = "Text block deleted"),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
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

/// Export a document image layer as a file download.
#[utoipa::path(
    get,
    operation_id = "exportDocumentImage",
    path = "/documents/{document_id}/exports/images/{layer}",
    tag = "documents",
    responses(
        (
            status = 200,
            description = "Exported document image",
            body = inline(BinaryBody),
            content_type = "application/octet-stream"
        ),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn export_document_image(
    State(state): State<ApiState>,
    Path((document_id, layer)): Path<(String, ExportLayer)>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let (_, document) = find_document(&resources, &document_id).await?;
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

/// Export a document as a layered PSD.
#[utoipa::path(
    get,
    operation_id = "exportDocumentPsd",
    path = "/documents/{document_id}/exports/psd",
    tag = "documents",
    responses(
        (
            status = 200,
            description = "Exported PSD document",
            body = inline(BinaryBody),
            content_type = "image/vnd.adobe.photoshop"
        ),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn export_document_psd(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let (_, document) = find_document(&resources, &document_id).await?;
    let data = koharu_psd::export_document(&document, &app_psd_export_options())
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(binary_response(
        data,
        "image/vnd.adobe.photoshop",
        Some(psd_export_filename(&document)),
    ))
}

/// Export all documents for the selected output layer.
#[utoipa::path(
    post,
    operation_id = "exportAllDocuments",
    path = "/document-exports",
    tag = "documents",
    request_body = ExportAllRequest,
    responses(
        (status = 200, body = ExportResult),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn export_all(
    State(state): State<ApiState>,
    Json(request): Json<ExportAllRequest>,
) -> ApiResult<Json<ExportResult>> {
    let resources = state.resources()?;
    let count = match request.layer.unwrap_or(ExportLayer::Rendered) {
        ExportLayer::Rendered => operations::export_all_rendered(resources).await?,
        ExportLayer::Inpainted => operations::export_all_inpainted(resources).await?,
    };
    Ok(Json(ExportResult { count }))
}

/// List the LLM models available for the requested language or base URL.
#[utoipa::path(
    get,
    operation_id = "listLlmModels",
    path = "/llm/models",
    tag = "llm",
    params(ListLlmModelsQuery),
    responses(
        (status = 200, body = [LlmModelInfo]),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn list_llm_models(
    State(state): State<ApiState>,
    Query(query): Query<ListLlmModelsQuery>,
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

/// Fetch the current LLM runtime state.
#[utoipa::path(
    get,
    operation_id = "getLlmSession",
    path = "/llm/session",
    tag = "llm",
    responses(
        (status = 200, body = koharu_core::LlmState),
        (status = 503, body = ErrorResponse)
    )
)]
async fn get_llm_runtime(State(state): State<ApiState>) -> ApiResult<Json<koharu_core::LlmState>> {
    let resources = state.resources()?;
    Ok(Json(resources.llm.snapshot().await))
}

/// Load or replace the active LLM runtime.
#[utoipa::path(
    put,
    operation_id = "setLlmSession",
    path = "/llm/session",
    tag = "llm",
    request_body = LlmLoadRequest,
    responses(
        (status = 200, body = koharu_core::LlmState),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn load_llm_runtime(
    State(state): State<ApiState>,
    Json(request): Json<LlmLoadRequest>,
) -> ApiResult<Json<koharu_core::LlmState>> {
    let resources = state.resources()?;
    operations::llm_load(
        resources.clone(),
        LlmLoadPayload {
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

/// Unload the active LLM runtime.
#[utoipa::path(
    delete,
    operation_id = "deleteLlmSession",
    path = "/llm/session",
    tag = "llm",
    responses(
        (status = 200, body = koharu_core::LlmState),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn offload_llm_runtime(
    State(state): State<ApiState>,
) -> ApiResult<Json<koharu_core::LlmState>> {
    let resources = state.resources()?;
    operations::llm_offload(resources.clone()).await?;
    Ok(Json(resources.llm.snapshot().await))
}

/// Validate connectivity to an OpenAI-compatible LLM endpoint.
#[utoipa::path(
    post,
    operation_id = "pingLlm",
    path = "/llm/ping",
    tag = "llm",
    request_body = LlmPingRequest,
    responses((status = 200, body = LlmPingResponse))
)]
async fn ping_llm(
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

/// Fetch the configured API key for a provider.
#[utoipa::path(
    get,
    operation_id = "getProviderApiKey",
    path = "/providers/{provider}/credentials/api-key",
    tag = "providers",
    responses(
        (status = 200, body = ApiKeyResponse),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn get_api_key(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
) -> ApiResult<Json<ApiKeyResponse>> {
    let resources = state.resources()?;
    let result = operations::get_api_key(resources, ApiKeyGetPayload { provider }).await?;
    Ok(Json(ApiKeyResponse {
        api_key: result.api_key,
    }))
}

/// Set or replace the API key for a provider.
#[utoipa::path(
    put,
    operation_id = "setProviderApiKey",
    path = "/providers/{provider}/credentials/api-key",
    tag = "providers",
    request_body = ApiKeyValue,
    responses(
        (status = 204, description = "API key updated"),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn set_api_key(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
    Json(request): Json<ApiKeyValue>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    operations::set_api_key(
        resources,
        ApiKeySetPayload {
            provider,
            api_key: request.api_key,
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Start a new pipeline job.
#[utoipa::path(
    post,
    operation_id = "createPipelineJob",
    path = "/pipeline-jobs",
    tag = "jobs",
    request_body = PipelineJobRequest,
    responses(
        (status = 200, body = JobState),
        (status = 400, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn start_pipeline_job(
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
    state.events.publish_job(job.clone()).await;

    Ok(Json(job))
}

/// Cancel a running pipeline job.
#[utoipa::path(
    delete,
    operation_id = "cancelPipelineJob",
    path = "/pipeline-jobs/{job_id}",
    tag = "jobs",
    responses(
        (status = 204, description = "Pipeline job cancelled"),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
async fn cancel_pipeline_job(
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

/// Stream server-sent events for state updates.
#[utoipa::path(
    get,
    operation_id = "streamEvents",
    path = "/events",
    tag = "events",
    responses((status = 200, description = "SSE event stream", content_type = "text/event-stream"))
)]
async fn events_stream(
    State(state): State<ApiState>,
) -> ApiResult<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>> {
    let events = state.events.clone();
    let snapshot = events.snapshot().await?;
    let mut rx = events.subscribe();

    let stream = stream! {
        yield Ok(sse_event("snapshot", &snapshot));
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Some(event) = api_event_to_sse(event) {
                        yield Ok(event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE client lagged behind by {n} events, re-sending snapshot");
                    match events.snapshot().await {
                        Ok(snap) => yield Ok(sse_event("snapshot", &snap)),
                        Err(e) => tracing::warn!("Failed to build resync snapshot: {e}"),
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

fn api_event_to_sse(event: ApiEvent) -> Option<Event> {
    match event {
        ApiEvent::DocumentsChanged(payload) => Some(sse_event("documents.changed", &payload)),
        ApiEvent::DocumentChanged(payload) => Some(sse_event("document.changed", &payload)),
        ApiEvent::JobChanged(payload) => Some(sse_event("job.changed", &payload)),
        ApiEvent::DownloadChanged(payload) => Some(sse_event("download.changed", &payload)),
        ApiEvent::LlmChanged(payload) => Some(sse_event("llm.changed", &payload)),
    }
}

fn sse_event<T: serde::Serialize>(name: &str, payload: &T) -> Event {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    Event::default().event(name).data(data)
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

fn document_layer(
    document: &Document,
    layer: DocumentLayer,
) -> ApiResult<&SerializableDynamicImage> {
    match layer {
        DocumentLayer::Original => Ok(&document.image),
        DocumentLayer::Segment => document
            .segment
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No segment layer available")),
        DocumentLayer::Inpainted => document
            .inpainted
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No inpainted layer available")),
        DocumentLayer::Rendered => document
            .rendered
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No rendered layer available")),
        DocumentLayer::Brush => document
            .brush_layer
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No brush layer available")),
    }
}

fn encode_webp(image: &SerializableDynamicImage) -> ApiResult<Vec<u8>> {
    encode_image(image, "webp")
}

fn encode_image(image: &SerializableDynamicImage, ext: &str) -> ApiResult<Vec<u8>> {
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
) -> ApiResult<(&SerializableDynamicImage, String)> {
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
