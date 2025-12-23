/// API handlers for the Comic Read Script translation functionality.
///
use std::{io::Cursor, path::PathBuf, sync::Arc};

use axum::{
    body::Body,
    extract::{Multipart, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use image::ImageFormat;
use koharu_ml::language_from_tag;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::{
    api::{ApiError, ApiResult, ApiState},
    operations::{self, DocumentInput},
    state,
};

#[derive(Debug, Default, Deserialize)]
struct TranslateConfig {
    #[serde(default)]
    translator: Option<TranslateTranslatorConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct TranslateTranslatorConfig {
    #[serde(default)]
    target_lang: Option<String>,
}

impl TranslateConfig {
    fn target_language(&self) -> Option<String> {
        self.translator
            .as_ref()
            .and_then(|translator| translator.target_lang.as_ref())
            .map(|lang| language_from_tag(&lang.to_ascii_lowercase()).to_string())
    }
}

pub async fn translate_with_form_image_stream(
    State(state): State<ApiState>,
    multipart: Multipart,
) -> ApiResult<Response> {
    match translate_request(&state, multipart).await {
        Ok(png_data) => Ok(build_response(0, &png_data)),
        Err(err) => {
            let msg = err.message;
            tracing::error!(status = ?err.status, message = ?msg, "Translation pipeline failed");
            Ok(build_response(2, msg.as_bytes()))
        }
    }
}

async fn translate_request(api_state: &ApiState, mut multipart: Multipart) -> ApiResult<Vec<u8>> {
    let mut image_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut config: Option<TranslateConfig> = None;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().map(|s| s.to_string());
        let filename = field.file_name().map(|s| s.to_string());
        let data = field.bytes().await?;
        match name.as_deref() {
            Some("image") => {
                if data.is_empty() {
                    continue;
                }
                file_name = filename;
                image_bytes = Some(data.to_vec());
            }
            Some("config") => {
                config = serde_json::from_slice::<TranslateConfig>(&data).ok();
            }
            _ => {}
        }
    }

    let image_bytes =
        image_bytes.ok_or_else(|| ApiError::bad_request("Field \"image\" is required"))?;
    let documents = operations::load_documents(vec![DocumentInput {
        path: PathBuf::from(file_name.unwrap_or_else(|| "upload.png".to_string())),
        bytes: image_bytes,
    }])
    .map_err(ApiError::from)?;

    if documents.is_empty() {
        return Err(ApiError::bad_request("Failed to decode image"));
    }

    let target_language = config.unwrap_or_default().target_language();

    let working_state: state::AppState = Arc::new(RwLock::new(state::State { documents }));
    let doc_index = 0usize;

    let _ = operations::detect(&working_state, api_state.ml(), doc_index).await?;
    let _ = operations::ocr(&working_state, api_state.ml(), doc_index).await?;
    let _ = operations::inpaint(&working_state, api_state.ml(), doc_index).await?;
    let _ = operations::llm_generate(
        &working_state,
        api_state.llm(),
        doc_index,
        None,
        target_language,
    )
    .await?;
    let doc =
        operations::render(&working_state, api_state.renderer(), doc_index, None, None).await?;

    let image = doc
        .rendered
        .as_ref()
        .or(doc.inpainted.as_ref())
        .unwrap_or(&doc.image);

    let mut buf = Vec::new();
    image
        .0
        .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|err| ApiError::internal(err.to_string()))?;

    Ok(buf)
}

fn build_response(message_type: u8, data: &[u8]) -> Response {
    let mut message = Vec::with_capacity(5 + data.len());
    message.push(message_type);
    let size = data.len() as u32;
    message.extend_from_slice(&size.to_be_bytes());
    message.extend_from_slice(data);

    let mut response = Response::new(Body::from(message));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response
}
