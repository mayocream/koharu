use std::{convert::Infallible, sync::Arc, time::Duration};

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, patch, post, put},
};
use koharu_core::{
    ApiKeyResponse, ApiKeyValue, Config, CreateTextBlock, DocumentDetail, DocumentSummary,
    ExportLayer, ExportResult, FileEntry, FontFaceInfo, JobState, JobStatus, LlmLoadRequest,
    LlmModelInfo, LlmPingRequest, LlmPingResponse, LlmState, LlmStateStatus, MaskRegionRequest,
    MetaInfo, PipelineJobRequest, RenderRequest, TextBlock, TextBlockDetail, TextBlockPatch,
    TranslateRequest,
};
use serde::Deserialize;

use super::support::{
    app_psd_export_options, apply_text_block_patch, binary_response, document_layer, encode_bytes,
    encode_webp, export_target, find_document, find_text_block_index, mime_from_ext,
    psd_export_filename, region_to_inpaint_region, sse_event,
};
use crate::bootstrap::BootstrapManager;
use crate::server::{
    events::{ApiEvent, EventHub},
    state::{SharedResources, get_resources},
};
use crate::services::{
    AppResources, operations,
    request::{
        ApiKeyUpdate, BrushLayerUpdate, InpaintMaskUpdate, LlmLoadJob, ModelCatalogQuery,
        PartialInpaintJob, PipelineJob, RenderJob, TranslateJob,
    },
    store::{self, ChangedField},
};

const MAX_BODY_SIZE: usize = 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct ApiState {
    pub resources: SharedResources,
    pub bootstrap: Arc<BootstrapManager>,
    pub events: EventHub,
}

impl ApiState {
    fn resources(&self) -> ApiResult<AppResources> {
        get_resources(&self.resources).map_err(ApiError::service_unavailable)
    }
}

pub fn router(
    resources: SharedResources,
    bootstrap: Arc<BootstrapManager>,
    events: EventHub,
) -> Router {
    let state = ApiState {
        resources,
        bootstrap,
        events,
    };

    Router::new()
        .route("/config", get(get_config).put(update_config))
        .route("/initialize", post(initialize))
        .route("/meta", get(get_meta))
        .route("/fonts", get(get_fonts))
        .route("/documents", get(list_documents))
        .route("/documents/import", post(import_documents))
        .route("/documents/{document_id}", get(get_document))
        .route("/documents/{document_id}/thumbnail", get(get_thumbnail))
        .route(
            "/documents/{document_id}/layers/{layer}",
            get(get_document_layer),
        )
        .route("/documents/{document_id}/detect", post(detect_document))
        .route("/documents/{document_id}/ocr", post(ocr_document))
        .route("/documents/{document_id}/inpaint", post(inpaint_document))
        .route("/documents/{document_id}/render", post(render_document))
        .route(
            "/documents/{document_id}/translate",
            post(translate_document),
        )
        .route(
            "/documents/{document_id}/mask-region",
            put(update_mask_region),
        )
        .route(
            "/documents/{document_id}/brush-region",
            put(update_brush_region),
        )
        .route(
            "/documents/{document_id}/inpaint-region",
            post(inpaint_region),
        )
        .route(
            "/documents/{document_id}/text-blocks",
            post(create_text_block),
        )
        .route(
            "/documents/{document_id}/text-blocks/{text_block_id}",
            patch(patch_text_block).delete(delete_text_block),
        )
        .route("/documents/{document_id}/export", get(export_document))
        .route(
            "/documents/{document_id}/export/psd",
            get(export_document_psd),
        )
        .route("/llm/models", get(list_llm_models))
        .route("/llm/state", get(get_llm_state))
        .route("/llm/load", post(load_llm))
        .route("/llm/offload", post(offload_llm))
        .route("/llm/ping", post(ping_llm))
        .route(
            "/providers/{provider}/api-key",
            get(get_api_key).put(set_api_key),
        )
        .route("/jobs/pipeline", post(start_pipeline_job))
        .route("/jobs/{job_id}", delete(cancel_pipeline_job))
        .route("/exports", post(export_all))
        .route("/events", get(events_stream))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}

pub(super) type ApiResult<T> = Result<T, ApiError>;

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

    pub(super) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub(super) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub(super) fn service_unavailable(error: anyhow::Error) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string())
    }

    pub(super) fn internal(error: anyhow::Error) -> Self {
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
        (self.status, self.message).into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportQuery {
    mode: Option<koharu_core::ImportMode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayerQuery {
    layer: Option<ExportLayer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmModelsQuery {
    language: Option<String>,
    openai_compatible_base_url: Option<String>,
}

async fn get_meta(State(state): State<ApiState>) -> ApiResult<Json<MetaInfo>> {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let ml_device = match state.resources() {
        Ok(resources) => Some(operations::device(&resources).to_string()),
        Err(_) => None,
    };
    Ok(Json(MetaInfo { version, ml_device }))
}

async fn get_fonts(State(state): State<ApiState>) -> ApiResult<Json<Vec<FontFaceInfo>>> {
    let resources = state.resources()?;
    let fonts = operations::list_font_families(resources)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(fonts))
}

async fn list_documents(State(state): State<ApiState>) -> ApiResult<Json<Vec<DocumentSummary>>> {
    match state.resources() {
        Ok(resources) => {
            let guard = resources.state.read().await;
            let documents = guard.documents.iter().map(DocumentSummary::from).collect();
            Ok(Json(documents))
        }
        Err(_) => Ok(Json(Vec::new())),
    }
}

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
    Ok(Json(DocumentDetail::from(doc)))
}

async fn get_thumbnail(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<Response> {
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

async fn get_document_layer(
    State(state): State<ApiState>,
    Path((document_id, layer)): Path<(String, String)>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let guard = resources.state.read().await;
    let doc = guard
        .documents
        .iter()
        .find(|d| d.id == document_id)
        .ok_or_else(|| ApiError::not_found("Document not found"))?;
    let image = document_layer(doc, &layer)?;
    let bytes = encode_webp(image)?;
    drop(guard);
    Ok(binary_response(bytes, "image/webp", None))
}

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
        files.push(FileEntry {
            name: filename,
            data: data.to_vec(),
        });
    }

    if files.is_empty() {
        return Err(ApiError::bad_request("No files uploaded"));
    }

    match query.mode.unwrap_or(koharu_core::ImportMode::Replace) {
        koharu_core::ImportMode::Replace => {
            operations::open_documents(resources.clone(), files).await?;
        }
        koharu_core::ImportMode::Append => {
            operations::add_documents(resources.clone(), files).await?;
        }
    }

    let documents = store::list_docs(&resources.state)
        .await
        .iter()
        .map(DocumentSummary::from)
        .collect::<Vec<_>>();

    Ok(Json(koharu_core::ImportResult {
        total_count: documents.len(),
        documents,
    }))
}

async fn detect_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::detect(resources, index).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ocr_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::ocr(resources, index).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn inpaint_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::inpaint(resources, index).await?;
    Ok(StatusCode::NO_CONTENT)
}

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
        RenderJob {
            document_index: index,
            text_block_index,
            shader_effect: request.shader_effect,
            shader_stroke: request.shader_stroke,
            font_family: request.font_family,
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

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
        TranslateJob {
            document_index: index,
            text_block_index,
            language: request.language,
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn update_mask_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<MaskRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::update_inpaint_mask(
        resources,
        InpaintMaskUpdate {
            document_index: index,
            mask: request.data,
            region: request.region.map(region_to_inpaint_region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_brush_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_core::BrushRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::update_brush_layer(
        resources,
        BrushLayerUpdate {
            document_index: index,
            patch: request.data,
            region: region_to_inpaint_region(request.region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn inpaint_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_core::InpaintRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::inpaint_partial(
        resources,
        PartialInpaintJob {
            document_index: index,
            region: region_to_inpaint_region(request.region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_text_block(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<CreateTextBlock>,
) -> ApiResult<Json<TextBlockDetail>> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;

    let detail = store::mutate_doc(
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

async fn patch_text_block(
    State(state): State<ApiState>,
    Path((document_id, text_block_id)): Path<(String, String)>,
    Json(request): Json<TextBlockPatch>,
) -> ApiResult<Json<TextBlockDetail>> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;

    let detail = store::mutate_doc(
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

async fn delete_text_block(
    State(state): State<ApiState>,
    Path((document_id, text_block_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;

    store::mutate_doc(
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

async fn list_llm_models(
    State(state): State<ApiState>,
    Query(query): Query<LlmModelsQuery>,
) -> ApiResult<Json<Vec<LlmModelInfo>>> {
    let Ok(resources) = state.resources() else {
        return Ok(Json(Vec::new()));
    };
    let models = operations::llm_list(
        resources,
        ModelCatalogQuery {
            language: query.language,
            openai_compatible_base_url: query.openai_compatible_base_url,
        },
    )
    .await?;
    Ok(Json(models))
}

async fn get_llm_state(State(state): State<ApiState>) -> ApiResult<Json<koharu_core::LlmState>> {
    match state.resources() {
        Ok(resources) => Ok(Json(resources.llm.snapshot().await)),
        Err(_) => Ok(Json(LlmState {
            status: LlmStateStatus::Empty,
            model_id: None,
            source: None,
            error: None,
        })),
    }
}

async fn load_llm(
    State(state): State<ApiState>,
    Json(request): Json<LlmLoadRequest>,
) -> ApiResult<Json<koharu_core::LlmState>> {
    let resources = state.resources()?;
    operations::llm_load(
        resources.clone(),
        LlmLoadJob {
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

async fn offload_llm(State(state): State<ApiState>) -> ApiResult<Json<koharu_core::LlmState>> {
    let resources = state.resources()?;
    operations::llm_offload(resources.clone()).await?;
    Ok(Json(resources.llm.snapshot().await))
}

async fn ping_llm(Json(request): Json<LlmPingRequest>) -> ApiResult<Json<LlmPingResponse>> {
    match operations::llm_ping(&request.base_url, request.api_key.as_deref()).await {
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

async fn get_api_key(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
) -> ApiResult<Json<ApiKeyResponse>> {
    let resources = state.resources()?;
    let api_key = operations::get_api_key(resources, &provider).await?;
    Ok(Json(ApiKeyResponse { api_key }))
}

async fn set_api_key(
    State(state): State<ApiState>,
    Path(provider): Path<String>,
    Json(request): Json<ApiKeyValue>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    operations::set_api_key(
        resources,
        ApiKeyUpdate {
            provider,
            api_key: request.api_key,
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn start_pipeline_job(
    State(state): State<ApiState>,
    Json(request): Json<PipelineJobRequest>,
) -> ApiResult<Json<JobState>> {
    let resources = state.resources()?;
    let index = if let Some(document_id) = request.document_id.as_deref() {
        Some(store::find_doc_index(&resources.state, document_id).await?)
    } else {
        None
    };
    let total_documents = match index {
        Some(_) => 1,
        None => store::doc_count(&resources.state).await,
    };

    let job_id = operations::process(
        resources.clone(),
        PipelineJob {
            document_index: index,
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

async fn export_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Query(query): Query<LayerQuery>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let (_, document) = find_document(&resources, &document_id).await?;
    let layer = query.layer.unwrap_or(ExportLayer::Rendered);
    let (image, filename) = export_target(&document, layer)?;
    let ext = document
        .path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let data = encode_bytes(image, &ext)?;
    let content_type = mime_from_ext(&ext);
    Ok(binary_response(data, content_type, Some(filename)))
}

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

async fn export_all(
    State(state): State<ApiState>,
    Query(query): Query<LayerQuery>,
) -> ApiResult<Json<ExportResult>> {
    let resources = state.resources()?;
    let count = match query.layer.unwrap_or(ExportLayer::Rendered) {
        ExportLayer::Rendered => operations::export_all_rendered(resources).await?,
        ExportLayer::Inpainted => operations::export_all_inpainted(resources).await?,
    };
    Ok(Json(ExportResult { count }))
}

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
                    yield Ok(api_event_to_sse(event));
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

fn api_event_to_sse(event: ApiEvent) -> Event {
    match event {
        ApiEvent::Documents(payload) => sse_event("documents.changed", &payload),
        ApiEvent::Document(payload) => sse_event("document.changed", &payload),
        ApiEvent::Job(payload) => sse_event("job.changed", &payload),
        ApiEvent::Download(payload) => sse_event("download.changed", &payload),
        ApiEvent::Llm(payload) => sse_event("llm.changed", &payload),
    }
}

async fn get_config(State(state): State<ApiState>) -> ApiResult<Json<Config>> {
    Ok(Json(state.bootstrap.config()))
}

async fn update_config(
    State(state): State<ApiState>,
    Json(config): Json<Config>,
) -> ApiResult<Json<Config>> {
    let config = state
        .bootstrap
        .update_config(config)
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(config))
}

async fn initialize(State(state): State<ApiState>) -> ApiResult<StatusCode> {
    state
        .bootstrap
        .initialize()
        .await
        .map_err(|error| ApiError::internal(anyhow::Error::new(error)))?;
    Ok(StatusCode::NO_CONTENT)
}
