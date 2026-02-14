use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::{broadcast, mpsc};

use koharu_pipeline::AppResources;
use koharu_pipeline::operations;

pub type SharedResources = Arc<tokio::sync::OnceCell<AppResources>>;

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

fn to_value<T: Serialize>(val: &T) -> Result<rmpv::Value> {
    let bytes = rmp_serde::to_vec_named(val)?;
    Ok(rmp_serde::from_slice(&bytes)?)
}

fn from_value<T: DeserializeOwned>(val: rmpv::Value) -> Result<T> {
    let bytes = rmp_serde::to_vec_named(&val)?;
    Ok(rmp_serde::from_slice(&bytes)?)
}

async fn call<F, Fut, P, T>(f: F, state: AppResources, params: rmpv::Value) -> Result<rmpv::Value>
where
    F: FnOnce(AppResources, P) -> Fut,
    Fut: Future<Output = Result<T>>,
    P: DeserializeOwned,
    T: Serialize,
{
    to_value(&f(state, from_value(params)?).await?)
}

async fn call0<F, Fut, T>(f: F, state: AppResources) -> Result<rmpv::Value>
where
    F: FnOnce(AppResources) -> Fut,
    Fut: Future<Output = Result<T>>,
    T: Serialize,
{
    to_value(&f(state).await?)
}

async fn dispatch(method: &str, params: rmpv::Value, state: AppResources) -> Result<rmpv::Value> {
    match method {
        "app_version" => call0(operations::app_version, state).await,
        "device" => call0(operations::device, state).await,
        "get_documents" => call0(operations::get_documents, state).await,
        "list_font_families" => call0(operations::list_font_families, state).await,
        "llm_list" => call0(operations::llm_list, state).await,
        "llm_ready" => call0(operations::llm_ready, state).await,
        "llm_offload" => call0(operations::llm_offload, state).await,
        "process_cancel" => call0(operations::process_cancel, state).await,
        "get_document" => call(operations::get_document, state, params).await,
        "get_thumbnail" => call(operations::get_thumbnail, state, params).await,
        "export_document" => call(operations::export_document, state, params).await,
        "open_documents" => call(operations::open_documents, state, params).await,
        "open_external" => call(operations::open_external, state, params).await,
        "detect" => call(operations::detect, state, params).await,
        "ocr" => call(operations::ocr, state, params).await,
        "inpaint" => call(operations::inpaint, state, params).await,
        "update_inpaint_mask" => call(operations::update_inpaint_mask, state, params).await,
        "update_brush_layer" => call(operations::update_brush_layer, state, params).await,
        "inpaint_partial" => call(operations::inpaint_partial, state, params).await,
        "render" => call(operations::render, state, params).await,
        "update_text_blocks" => call(operations::update_text_blocks, state, params).await,
        "llm_load" => call(operations::llm_load, state, params).await,
        "llm_generate" => call(operations::llm_generate, state, params).await,
        "process" => call(operations::process, state, params).await,
        _ => Err(anyhow::anyhow!("Unknown method: {}", method)),
    }
}

#[derive(Clone)]
pub struct WsState {
    pub resources: SharedResources,
}

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
        koharu_http::download::subscribe(),
        tx.clone(),
    );
    spawn_notification_forwarder(
        "process_progress",
        koharu_pipeline::pipeline::subscribe(),
        tx.clone(),
    );

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

        tokio::spawn(async move {
            let response = match resources.get() {
                Some(res) => {
                    let params = raw.params.unwrap_or(rmpv::Value::Nil);
                    match tokio::time::timeout(
                        Duration::from_secs(300),
                        dispatch(&raw.method, params, res.clone()),
                    )
                    .await
                    {
                        Ok(Ok(result)) => ok_response(id, result),
                        Ok(Err(e)) => err_response(id, &format!("{e:#}")),
                        Err(_) => err_response(id, "Request timed out"),
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
