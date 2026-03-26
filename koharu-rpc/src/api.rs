use std::{convert::Infallible, io::Cursor, time::Duration};

use anyhow::Context;
use async_stream::stream;
use axum::{
    Json, Router,
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
    routing::{delete, get, patch, post, put},
};
use image::ImageFormat;
use koharu_pipeline::{
    AppResources, operations,
    state_tx::{self, ChangedField},
};
use koharu_psd::{PsdExportOptions, TextLayerMode};
use koharu_types::{
    ApiKeyGetPayload, ApiKeyResponse, ApiKeySetPayload, ApiKeyValue, CreateTextBlock,
    DetectPayload, Document, DocumentDetail, DocumentSummary, ExportLayer, ExportResult, FileEntry,
    FontFaceInfo, IndexPayload, InpaintPartialPayload, InpaintRegion, JobState, JobStatus,
    LlmLoadPayload, LlmLoadRequest, LlmModelInfo, LlmPingRequest, LlmPingResponse,
    MaskRegionRequest, MetaInfo, OpenDocumentsPayload, PipelineJobRequest, Region, RenderPayload,
    RenderRequest, SerializableDynamicImage, TextBlock, TextBlockDetail, TextBlockPatch,
    TranslateRequest, UpdateBrushLayerPayload, UpdateInpaintMaskPayload,
};
use serde::Deserialize;

use crate::{
    events::{ApiEvent, EventHub},
    shared::{SharedResources, get_resources},
};

const MAX_BODY_SIZE: usize = 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct ApiState {
    pub resources: SharedResources,
    pub events: EventHub,
}

impl ApiState {
    fn resources(&self) -> ApiResult<AppResources> {
        get_resources(&self.resources).map_err(ApiError::service_unavailable)
    }
}

pub fn router(resources: SharedResources, events: EventHub) -> Router {
    let state = ApiState { resources, events };

    Router::new()
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
        .route(
            "/documents/{document_id}/detect-options",
            post(detect_document_options),
        )
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
            "/documents/{document_id}/inpaint-free",
            post(inpaint_free_region),
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

type ApiResult<T> = Result<T, ApiError>;

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
        (self.status, self.message).into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportQuery {
    mode: Option<koharu_types::ImportMode>,
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
    let resources = state.resources()?;
    let device = operations::device(resources.clone()).await?;
    Ok(Json(MetaInfo {
        version: resources.version.to_string(),
        ml_device: device.ml_device,
    }))
}

async fn get_fonts(State(state): State<ApiState>) -> ApiResult<Json<Vec<FontFaceInfo>>> {
    let resources = state.resources()?;
    let fonts = operations::list_font_families(resources)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(fonts))
}

async fn list_documents(State(state): State<ApiState>) -> ApiResult<Json<Vec<DocumentSummary>>> {
    let resources = state.resources()?;
    let documents = state_tx::list_docs(&resources.state)
        .await
        .iter()
        .map(DocumentSummary::from)
        .collect();
    Ok(Json(documents))
}

async fn get_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<Json<DocumentDetail>> {
    let resources = state.resources()?;
    let (_, document) = find_document(&resources, &document_id).await?;
    Ok(Json(DocumentDetail::from(&document)))
}

async fn get_thumbnail(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let (_, document) = find_document(&resources, &document_id).await?;
    let source = document.rendered.as_ref().unwrap_or(&document.image);
    let thumbnail = source.thumbnail(200, 200);
    let bytes = encode_webp(&thumbnail.into())?;
    Ok(binary_response(bytes, "image/webp", None))
}

async fn get_document_layer(
    State(state): State<ApiState>,
    Path((document_id, layer)): Path<(String, String)>,
) -> ApiResult<Response> {
    let resources = state.resources()?;
    let (_, document) = find_document(&resources, &document_id).await?;
    let image = document_layer(&document, &layer)?;
    let bytes = encode_webp(image)?;
    Ok(binary_response(bytes, "image/webp", None))
}

async fn import_documents(
    State(state): State<ApiState>,
    Query(query): Query<ImportQuery>,
    mut multipart: Multipart,
) -> ApiResult<Json<koharu_types::ImportResult>> {
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

    let payload = OpenDocumentsPayload { files };
    match query.mode.unwrap_or(koharu_types::ImportMode::Replace) {
        koharu_types::ImportMode::Replace => {
            operations::open_documents(resources.clone(), payload).await?;
        }
        koharu_types::ImportMode::Append => {
            operations::add_documents(resources.clone(), payload).await?;
        }
    }

    let documents = state_tx::list_docs(&resources.state)
        .await
        .iter()
        .map(DocumentSummary::from)
        .collect::<Vec<_>>();

    Ok(Json(koharu_types::ImportResult {
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
    operations::detect(resources, IndexPayload { index }).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detect_document_options(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_types::DetectRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::detect_with_options(
        resources,
        DetectPayload {
            index,
            sensitive: request.sensitive,
            region: request.region.map(to_inpaint_region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ocr_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::ocr(resources, IndexPayload { index }).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn inpaint_document(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::inpaint(resources, IndexPayload { index }).await?;
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
        koharu_types::LlmGeneratePayload {
            index,
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
        UpdateInpaintMaskPayload {
            index,
            mask: request.data,
            region: request.region.map(to_inpaint_region),
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_brush_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_types::BrushRegionRequest>,
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

async fn inpaint_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_types::InpaintRegionRequest>,
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

async fn inpaint_free_region(
    State(state): State<ApiState>,
    Path(document_id): Path<String>,
    Json(request): Json<koharu_types::InpaintRegionRequest>,
) -> ApiResult<StatusCode> {
    let resources = state.resources()?;
    let (index, _) = find_document(&resources, &document_id).await?;
    operations::inpaint_free(
        resources,
        InpaintPartialPayload {
            index,
            region: to_inpaint_region(request.region),
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

async fn list_llm_models(
    State(state): State<ApiState>,
    Query(query): Query<LlmModelsQuery>,
) -> ApiResult<Json<Vec<LlmModelInfo>>> {
    let resources = state.resources()?;
    let models = operations::llm_list(
        resources,
        koharu_types::LlmListPayload {
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

async fn get_llm_state(State(state): State<ApiState>) -> ApiResult<Json<koharu_types::LlmState>> {
    let resources = state.resources()?;
    Ok(Json(resources.llm.snapshot().await))
}

async fn load_llm(
    State(state): State<ApiState>,
    Json(request): Json<LlmLoadRequest>,
) -> ApiResult<Json<koharu_types::LlmState>> {
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

async fn offload_llm(State(state): State<ApiState>) -> ApiResult<Json<koharu_types::LlmState>> {
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
    let result = operations::get_api_key(resources, ApiKeyGetPayload { provider }).await?;
    Ok(Json(ApiKeyResponse {
        api_key: result.api_key,
    }))
}

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
        None => state_tx::list_docs(&resources.state).await.len(),
    };

    let job_id = operations::process(
        resources.clone(),
        koharu_types::ProcessRequest {
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
        total_steps: koharu_types::PipelineStep::ALL.len(),
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
    let data = encode_image(image, &ext)?;
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
    let snapshot = state.events.snapshot().await?;
    let mut rx = state.events.subscribe();

    let stream = stream! {
        yield Ok(sse_event("snapshot", &snapshot));
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Some(event) = api_event_to_sse(event) {
                        yield Ok(event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
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

fn document_layer<'a>(
    document: &'a Document,
    layer: &str,
) -> ApiResult<&'a SerializableDynamicImage> {
    match layer {
        "original" => Ok(&document.image),
        "segment" => document
            .segment
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No segment layer available")),
        "inpainted" => document
            .inpainted
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No inpainted layer available")),
        "rendered" => document
            .rendered
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No rendered layer available")),
        "brush" => document
            .brush_layer
            .as_ref()
            .ok_or_else(|| ApiError::not_found("No brush layer available")),
        other => Err(ApiError::bad_request(format!("Unknown layer: {other}"))),
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
    use koharu_psd::TextLayerMode;
    use koharu_types::{Document, TextAlign, TextBlock, TextBlockPatch, TextDirection, TextStyle};

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
