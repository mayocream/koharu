use axum::{
    body::Body,
    http::{
        HeaderValue,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::{Response, sse::Event},
};
use koharu_core::{
    Document, ExportLayer, Region, SerializableDynamicImage, TextBlock, TextBlockPatch,
};
use koharu_psd::{PsdExportOptions, TextLayerMode};

use crate::services::{AppResources, store, support::encode_image};

use super::routes::{ApiError, ApiResult};

pub(super) fn sse_event<T: serde::Serialize>(name: &str, payload: &T) -> Event {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    Event::default().event(name).data(data)
}

pub(super) async fn find_document(
    resources: &AppResources,
    document_id: &str,
) -> ApiResult<(usize, Document)> {
    let index = store::find_doc_index(&resources.state, document_id)
        .await
        .map_err(ApiError::from)?;
    let document = store::read_doc(&resources.state, index)
        .await
        .map_err(ApiError::from)?;
    Ok((index, document))
}

pub(super) fn find_text_block_index(document: &Document, text_block_id: &str) -> ApiResult<usize> {
    document
        .text_blocks
        .iter()
        .position(|block| block.id == text_block_id)
        .ok_or_else(|| ApiError::not_found(format!("Text block not found: {text_block_id}")))
}

pub(super) fn document_layer<'a>(
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

pub(super) fn encode_webp(image: &SerializableDynamicImage) -> ApiResult<Vec<u8>> {
    encode_image(image, "webp").map_err(ApiError::internal)
}

pub(super) fn encode_bytes(image: &SerializableDynamicImage, ext: &str) -> ApiResult<Vec<u8>> {
    encode_image(image, ext).map_err(ApiError::internal)
}

pub(super) fn mime_from_ext(ext: &str) -> &'static str {
    crate::services::support::mime_from_ext(ext)
}

pub(super) fn binary_response(
    data: Vec<u8>,
    content_type: &str,
    filename: Option<String>,
) -> Response {
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

pub(super) fn export_target(
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

pub(super) fn psd_export_filename(document: &Document) -> String {
    format!("{}_koharu.psd", document.name)
}

pub(super) fn app_psd_export_options() -> PsdExportOptions {
    PsdExportOptions {
        text_layer_mode: TextLayerMode::Editable,
        ..PsdExportOptions::default()
    }
}

pub(super) fn region_to_inpaint_region(region: Region) -> crate::services::request::ImageRegion {
    crate::services::request::ImageRegion::from(region)
}

pub(super) fn apply_text_block_patch(block: &mut TextBlock, patch: TextBlockPatch) {
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
        let options = app_psd_export_options();
        assert_eq!(options.text_layer_mode, TextLayerMode::Editable);
    }
}
