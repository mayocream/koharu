use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tower::{Service, ServiceBuilder, ServiceExt, service_fn, timeout::TimeoutLayer};

use crate::app::AppResources;
use crate::operations;

// --- Shared Resources (lazy init) ---

pub type SharedResources = Arc<tokio::sync::OnceCell<AppResources>>;

// --- Wire Protocol ---

#[derive(Debug, Deserialize)]
struct RawIncoming {
    id: u32,
    method: String,
    params: Option<rmpv::Value>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
    #[serde(rename = "res")]
    Response {
        id: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<rmpv::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename = "ntf")]
    Notification { method: String, params: rmpv::Value },
}

fn ok_response(id: u32, result: rmpv::Value) -> OutgoingMessage {
    OutgoingMessage::Response {
        id,
        result: Some(result),
        error: None,
    }
}

fn err_response(id: u32, msg: &str) -> OutgoingMessage {
    OutgoingMessage::Response {
        id,
        result: None,
        error: Some(msg.to_string()),
    }
}

// --- Value conversion helpers ---

fn to_value<T: Serialize>(val: &T) -> Result<rmpv::Value> {
    let bytes = rmp_serde::to_vec_named(val)?;
    Ok(rmp_serde::from_slice(&bytes)?)
}

fn from_value<T: serde::de::DeserializeOwned>(val: rmpv::Value) -> Result<T> {
    let bytes = rmp_serde::to_vec_named(&val)?;
    Ok(rmp_serde::from_slice(&bytes)?)
}

// --- Handler Trait ---

#[async_trait]
#[enum_dispatch::enum_dispatch]
pub trait RpcCall {
    async fn call(self, res: AppResources) -> Result<rmpv::Value>;
}

// --- Handler Structs ---

// No-param handlers

#[derive(Deserialize)]
pub struct AppVersionHandler;

#[async_trait]
impl RpcCall for AppVersionHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::app_version(state).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
pub struct DeviceHandler;

#[async_trait]
impl RpcCall for DeviceHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::device(state).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
pub struct GetDocumentsHandler;

#[async_trait]
impl RpcCall for GetDocumentsHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::get_documents(state).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
pub struct ListFontFamiliesHandler;

#[async_trait]
impl RpcCall for ListFontFamiliesHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::list_font_families(state).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
pub struct LlmListHandler;

#[async_trait]
impl RpcCall for LlmListHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::llm_list(state).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
pub struct LlmOffloadHandler;

#[async_trait]
impl RpcCall for LlmOffloadHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::llm_offload(state).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
pub struct LlmReadyHandler;

#[async_trait]
impl RpcCall for LlmReadyHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::llm_ready(state).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
pub struct SaveDocumentsHandler;

#[async_trait]
impl RpcCall for SaveDocumentsHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::save_documents(state).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
pub struct ProcessCancelHandler;

#[async_trait]
impl RpcCall for ProcessCancelHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::process_cancel(state).await?;
        Ok(rmpv::Value::Nil)
    }
}

// Index-based handlers (wrapping IndexPayload)

#[derive(Deserialize)]
#[serde(transparent)]
pub struct GetDocumentHandler(operations::IndexPayload);

#[async_trait]
impl RpcCall for GetDocumentHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::get_document(state, self.0).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct GetThumbnailHandler(operations::IndexPayload);

#[async_trait]
impl RpcCall for GetThumbnailHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::get_thumbnail(state, self.0).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct ExportDocumentHandler(operations::IndexPayload);

#[async_trait]
impl RpcCall for ExportDocumentHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::export_document(state, self.0).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct DetectHandler(operations::IndexPayload);

#[async_trait]
impl RpcCall for DetectHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::detect(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct OcrHandler(operations::IndexPayload);

#[async_trait]
impl RpcCall for OcrHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::ocr(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct InpaintHandler(operations::IndexPayload);

#[async_trait]
impl RpcCall for InpaintHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::inpaint(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

// Complex payload handlers

#[derive(Deserialize)]
#[serde(transparent)]
pub struct OpenExternalHandler(operations::OpenExternalPayload);

#[async_trait]
impl RpcCall for OpenExternalHandler {
    async fn call(self, _state: AppResources) -> Result<rmpv::Value> {
        operations::open_external(self.0)?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct OpenDocumentsHandler(operations::OpenDocumentsPayload);

#[async_trait]
impl RpcCall for OpenDocumentsHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        let v = operations::open_documents(state, self.0).await?;
        to_value(&v)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct UpdateInpaintMaskHandler(operations::UpdateInpaintMaskPayload);

#[async_trait]
impl RpcCall for UpdateInpaintMaskHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::update_inpaint_mask(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct UpdateBrushLayerHandler(operations::UpdateBrushLayerPayload);

#[async_trait]
impl RpcCall for UpdateBrushLayerHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::update_brush_layer(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct InpaintPartialHandler(operations::InpaintPartialPayload);

#[async_trait]
impl RpcCall for InpaintPartialHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::inpaint_partial(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct RenderHandler(operations::RenderPayload);

#[async_trait]
impl RpcCall for RenderHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::render(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct UpdateTextBlocksHandler(operations::UpdateTextBlocksPayload);

#[async_trait]
impl RpcCall for UpdateTextBlocksHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::update_text_blocks(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct LlmLoadHandler(operations::LlmLoadPayload);

#[async_trait]
impl RpcCall for LlmLoadHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::llm_load(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct LlmGenerateHandler(operations::LlmGeneratePayload);

#[async_trait]
impl RpcCall for LlmGenerateHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::llm_generate(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct ProcessHandler(crate::pipeline::ProcessRequest);

#[async_trait]
impl RpcCall for ProcessHandler {
    async fn call(self, state: AppResources) -> Result<rmpv::Value> {
        operations::process(state, self.0).await?;
        Ok(rmpv::Value::Nil)
    }
}

// --- Method Dispatch Enum ---

#[derive(Deserialize)]
#[serde(tag = "method", content = "params")]
#[enum_dispatch::enum_dispatch(RpcCall)]
pub enum Method {
    #[serde(rename = "app_version")]
    AppVersion(AppVersionHandler),
    #[serde(rename = "device")]
    Device(DeviceHandler),
    #[serde(rename = "open_external")]
    OpenExternal(OpenExternalHandler),
    #[serde(rename = "get_documents")]
    GetDocuments(GetDocumentsHandler),
    #[serde(rename = "get_document")]
    GetDocument(GetDocumentHandler),
    #[serde(rename = "get_thumbnail")]
    GetThumbnail(GetThumbnailHandler),
    #[serde(rename = "open_documents")]
    OpenDocuments(OpenDocumentsHandler),
    #[serde(rename = "save_documents")]
    SaveDocuments(SaveDocumentsHandler),
    #[serde(rename = "export_document")]
    ExportDocument(ExportDocumentHandler),
    #[serde(rename = "detect")]
    Detect(DetectHandler),
    #[serde(rename = "ocr")]
    Ocr(OcrHandler),
    #[serde(rename = "inpaint")]
    Inpaint(InpaintHandler),
    #[serde(rename = "update_inpaint_mask")]
    UpdateInpaintMask(UpdateInpaintMaskHandler),
    #[serde(rename = "update_brush_layer")]
    UpdateBrushLayer(UpdateBrushLayerHandler),
    #[serde(rename = "inpaint_partial")]
    InpaintPartial(InpaintPartialHandler),
    #[serde(rename = "render")]
    Render(RenderHandler),
    #[serde(rename = "update_text_blocks")]
    UpdateTextBlocks(UpdateTextBlocksHandler),
    #[serde(rename = "list_font_families")]
    ListFontFamilies(ListFontFamiliesHandler),
    #[serde(rename = "llm_list")]
    LlmList(LlmListHandler),
    #[serde(rename = "llm_load")]
    LlmLoad(LlmLoadHandler),
    #[serde(rename = "llm_offload")]
    LlmOffload(LlmOffloadHandler),
    #[serde(rename = "llm_ready")]
    LlmReady(LlmReadyHandler),
    #[serde(rename = "llm_generate")]
    LlmGenerate(LlmGenerateHandler),
    #[serde(rename = "process")]
    Process(ProcessHandler),
    #[serde(rename = "process_cancel")]
    ProcessCancel(ProcessCancelHandler),
}

/// Parse method name + params into a Method enum using serde adjacently-tagged deserialization.
fn parse_call(method: &str, params: rmpv::Value) -> Result<Method> {
    let tagged = rmpv::Value::Map(vec![
        (
            rmpv::Value::String("method".into()),
            rmpv::Value::String(method.into()),
        ),
        (rmpv::Value::String("params".into()), params),
    ]);
    from_value(tagged)
}

// --- WebSocket State ---

#[derive(Clone)]
pub struct WsState {
    pub resources: SharedResources,
}

// --- WebSocket Handler ---

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<WsState>) -> impl IntoResponse {
    ws.max_message_size(1024 * 1024 * 1024)
        .on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: WsState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut send_rx) = mpsc::channel::<OutgoingMessage>(256);

    // Notification forwarders
    spawn_notification_forwarder(
        "download_progress",
        koharu_core::download::subscribe(),
        tx.clone(),
    );
    spawn_notification_forwarder("process_progress", crate::pipeline::subscribe(), tx.clone());

    // Sender task: drain mpsc → WebSocket binary frames
    let send_task = tokio::spawn(async move {
        while let Some(msg) = send_rx.recv().await {
            let Ok(bytes) = rmp_serde::to_vec_named(&msg) else {
                continue;
            };
            if ws_sender.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    // Tower dispatch service (cloned per request)
    let dispatch_svc = ServiceBuilder::new()
        .layer(TimeoutLayer::new(Duration::from_secs(300)))
        .service(service_fn(
            |(method, res): (Method, AppResources)| async move { method.call(res).await },
        ));

    // Receive loop
    while let Some(Ok(msg)) = ws_receiver.next().await {
        let data = match msg {
            Message::Binary(data) => data,
            Message::Close(_) => break,
            _ => continue,
        };

        let raw: RawIncoming = match rmp_serde::from_slice(&data) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx
                    .send(err_response(0, &format!("Decode error: {e}")))
                    .await;
                continue;
            }
        };

        let id = raw.id;
        let tx = tx.clone();
        let resources = state.resources.clone();
        let mut svc = dispatch_svc.clone();

        tokio::spawn(async move {
            let response = match resources.get() {
                Some(res) => {
                    let params = raw.params.unwrap_or(rmpv::Value::Nil);
                    match parse_call(&raw.method, params) {
                        Ok(method) => match svc.ready().await {
                            Ok(ready) => match ready.call((method, res.clone())).await {
                                Ok(result) => ok_response(id, result),
                                Err(e) => err_response(id, &format!("{e:#}")),
                            },
                            Err(e) => err_response(id, &format!("{e}")),
                        },
                        Err(e) => err_response(id, &format!("{e:#}")),
                    }
                }
                None => err_response(id, "Resources not initialized"),
            };
            let _ = tx.send(response).await;
        });
    }

    send_task.abort();
}

fn spawn_notification_forwarder<T: Serialize + Clone + Send + 'static>(
    method: &'static str,
    mut rx: broadcast::Receiver<T>,
    tx: mpsc::Sender<OutgoingMessage>,
) {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(payload) => {
                    let params = to_value(&payload).unwrap_or(rmpv::Value::Nil);
                    let msg = OutgoingMessage::Notification {
                        method: method.to_string(),
                        params,
                    };
                    if tx.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
