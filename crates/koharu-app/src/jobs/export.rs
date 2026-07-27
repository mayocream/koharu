use std::{collections::HashMap, fs};

use anyhow::{Context as _, Result};
use koharu_desktop::DesktopHandle;
use koharu_pipeline::CancellationToken;
use koharu_psd::{
    PsdDocument, PsdExportOptions, PsdShaderEffect, PsdTextAlign, PsdTextBlock, PsdTextDirection,
    PsdTextStyle, ResolvedDocument, export_document,
};
use koharu_renderer::{PageRenderOptions, Renderer};
use koharu_scene::{ElementKind, FontSlant, Page, Session, TextDirection, WritingMode};

use super::{ExportRequest, JobOutcome, NativeEvent, finish_job};
use crate::protocol::ExportFormat;

pub(super) fn run(
    renderer: &mut Option<Renderer>,
    request: ExportRequest,
    cancellation: CancellationToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let ExportRequest {
        id,
        path,
        directory,
        pages,
        format,
    } = request;
    let total = pages.len();
    let result = (|| -> Result<()> {
        let session =
            Session::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        if renderer.is_none() {
            *renderer = Some(Renderer::new().context("failed to initialize the export renderer")?);
        }
        let renderer = renderer.as_ref().expect("renderer initialized above");
        for (index, page_id) in pages.into_iter().enumerate() {
            if cancellation.is_cancelled() {
                break;
            }
            let page = session.page(page_id)?;
            let base_id = page.assets.clean.unwrap_or(page.source);
            let base = image::load_from_memory(&session.read_blob(base_id)?)
                .with_context(|| format!("failed to decode page {}", page.name))?;
            let rendered = renderer.composite_page(
                &base,
                page,
                |blob| session.read_blob(blob).map_err(Into::into),
                &PageRenderOptions::default(),
            )?;
            let stem = format!("{:04}_{}", index + 1, safe_name(&page.name));
            match format {
                ExportFormat::Png => rendered
                    .image
                    .save(directory.join(format!("{stem}.png")))
                    .with_context(|| format!("failed to export {}", page.name))?,
                ExportFormat::Psd => {
                    let bytes = export_psd(&session, renderer, page, rendered.image)?;
                    fs::write(directory.join(format!("{stem}.psd")), bytes)
                        .with_context(|| format!("failed to export {}", page.name))?;
                }
            }
            let _ = desktop.send_event(NativeEvent::ExportProgress {
                job: id,
                completed: index + 1,
                total,
            });
        }
        Ok(())
    })();
    finish_job(
        &desktop,
        id,
        &cancellation,
        JobOutcome {
            error: result.err().map(|error| error.to_string()),
            ..JobOutcome::default()
        },
    );
}

fn export_psd(
    session: &Session,
    renderer: &Renderer,
    page: &Page,
    rendered: image::RgbaImage,
) -> Result<Vec<u8>> {
    let mut document = PsdDocument {
        width: page.size.width,
        height: page.size.height,
        ..PsdDocument::default()
    };
    for element in &page.elements {
        let ElementKind::Text(text) = &element.kind else {
            continue;
        };
        let value = text.translation.as_deref().unwrap_or_default();
        let post_script = renderer.resolve_post_script_name(&text.style, Some(value))?;
        let font_index = document
            .fonts
            .iter()
            .position(|font| font == &post_script)
            .unwrap_or_else(|| {
                document.fonts.push(post_script);
                document.fonts.len() - 1
            });
        let source_direction = text
            .source
            .as_ref()
            .and_then(|source| match source.direction {
                TextDirection::Horizontal => Some(PsdTextDirection::Horizontal),
                TextDirection::Vertical => Some(PsdTextDirection::Vertical),
                TextDirection::Auto => None,
            });
        let rendered_direction = match text.layout.writing_mode {
            WritingMode::Horizontal => Some(PsdTextDirection::Horizontal),
            WritingMode::VerticalRightToLeft | WritingMode::VerticalLeftToRight => {
                Some(PsdTextDirection::Vertical)
            }
            WritingMode::Auto => source_direction,
        };
        document.text_blocks.push(PsdTextBlock {
            id: element.id.to_string(),
            x: element.frame.x,
            y: element.frame.y,
            width: element.frame.width,
            height: element.frame.height,
            translation: text.translation.clone(),
            style: Some(PsdTextStyle {
                font_families: text.style.font_families.clone(),
                font_size: Some(text.style.font_size),
                color: text.style.color,
                effect: Some(PsdShaderEffect {
                    italic: !matches!(text.style.font_slant, FontSlant::Normal),
                    bold: text.style.font_weight >= 600,
                }),
                text_align: Some(match text.layout.horizontal_align {
                    koharu_scene::TextAlign::Start | koharu_scene::TextAlign::Justify => {
                        PsdTextAlign::Left
                    }
                    koharu_scene::TextAlign::Center => PsdTextAlign::Center,
                    koharu_scene::TextAlign::End => PsdTextAlign::Right,
                }),
            }),
            rotation_deg: Some(element.frame.angle_degrees + text.style.angle_degrees),
            source_direction,
            rendered_direction,
            detected_font_size_px: Some(text.style.font_size),
            font_index: Some(font_index),
            ..PsdTextBlock::default()
        });
    }

    let source = image::load_from_memory(&session.read_blob(page.source)?)?;
    let clean = page
        .assets
        .clean
        .map(|blob| session.read_blob(blob))
        .transpose()?
        .map(|bytes| image::load_from_memory(&bytes))
        .transpose()?;
    let text_mask = page
        .assets
        .text_mask
        .map(|blob| session.read_blob(blob))
        .transpose()?
        .map(|bytes| image::load_from_memory(&bytes))
        .transpose()?;
    let coo_mask = page
        .assets
        .coo_mask
        .map(|blob| session.read_blob(blob))
        .transpose()?
        .map(|bytes| image::load_from_memory(&bytes))
        .transpose()?;
    let removal_mask = combine_masks(text_mask, coo_mask);
    let rendered = image::DynamicImage::ImageRgba8(rendered);
    let resolved = ResolvedDocument {
        document: &document,
        source: &source,
        segment: removal_mask.as_ref(),
        inpainted: clean.as_ref(),
        rendered: Some(&rendered),
        brush_layer: None,
        block_images: &HashMap::new(),
    };
    export_document(
        &resolved,
        &PsdExportOptions {
            include_brush_layer: false,
            ..PsdExportOptions::default()
        },
    )
    .map_err(Into::into)
}

fn combine_masks(
    text: Option<image::DynamicImage>,
    coo: Option<image::DynamicImage>,
) -> Option<image::DynamicImage> {
    match (text, coo) {
        (None, None) => None,
        (Some(mask), None) | (None, Some(mask)) => Some(mask),
        (Some(text), Some(coo)) => {
            let mut text = text.into_luma8();
            let coo = coo.into_luma8();
            for (target, source) in text.pixels_mut().zip(coo.pixels()) {
                target.0[0] = target.0[0].max(source.0[0]);
            }
            Some(image::DynamicImage::ImageLuma8(text))
        }
    }
}

fn safe_name(name: &str) -> String {
    let value = name
        .trim()
        .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
    let value = value.rsplit_once('.').map_or(value, |(stem, _)| stem);
    let value = value
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "page".into()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    #[test]
    fn export_names_cannot_escape_the_selected_directory() {
        assert_eq!(safe_name("../chapter:01.png"), ".._chapter_01");
        assert_eq!(safe_name(".png"), "page");
    }

    #[test]
    fn psd_removal_layer_contains_text_and_onomatopoeia() {
        let text = image::DynamicImage::ImageLuma8(GrayImage::from_fn(2, 1, |x, _| {
            Luma([if x == 0 { 255 } else { 0 }])
        }));
        let coo = image::DynamicImage::ImageLuma8(GrayImage::from_fn(2, 1, |x, _| {
            Luma([if x == 1 { 255 } else { 0 }])
        }));

        let mask = combine_masks(Some(text), Some(coo)).unwrap().into_luma8();

        assert_eq!(mask.as_raw(), &[255, 255]);
    }
}
