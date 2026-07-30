use std::{collections::HashMap, fs};

use anyhow::{Context as _, Result, anyhow};
use koharu_desktop::DesktopHandle;
use koharu_pipeline::StopToken;
use koharu_psd::{
    PsdDocument, PsdExportOptions, PsdShaderEffect, PsdTextAlign, PsdTextBlock, PsdTextDirection,
    PsdTextStyle, ResolvedDocument, export_document,
};
use koharu_renderer::{RenderRequest, Renderer};
use koharu_scene::{
    Asset, EntityId, Geometry, LanguageTag, OcrAnalysis, SceneSnapshot, SourceText, TextAlignment,
    TextDirection, Translation, Typography, WritingMode,
};

use super::{ExportRequest, JobOutcome, NativeEvent, finish_job};
use crate::protocol::ExportFormat;

pub(super) fn run(
    renderer: &mut Option<Renderer>,
    request: ExportRequest,
    stop: StopToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let ExportRequest {
        id,
        snapshot,
        directory,
        pages,
        format,
        locale,
    } = request;
    let total = pages.len();
    let result = (|| -> Result<()> {
        if renderer.is_none() {
            *renderer = Some(Renderer::new().context("failed to initialize the export renderer")?);
        }
        let renderer = renderer.as_ref().expect("renderer initialized above");
        for (index, page_id) in pages.into_iter().enumerate() {
            if stop.stopped() {
                break;
            }
            let page = snapshot.page(page_id)?.page()?;
            let mut request = RenderRequest::new(page_id);
            request.locale = locale.clone();
            let rendered = renderer
                .render(&snapshot, &request)
                .with_context(|| format!("failed to render page {}", page.label))?;
            let stem = format!("{:04}_{}", index + 1, safe_name(&page.label));
            match format {
                ExportFormat::Png => rendered
                    .image
                    .save(directory.join(format!("{stem}.png")))
                    .with_context(|| format!("failed to export {}", page.label))?,
                ExportFormat::Psd => {
                    let bytes = export_psd(
                        &snapshot,
                        renderer,
                        page_id,
                        locale.as_ref(),
                        rendered.image,
                    )?;
                    fs::write(directory.join(format!("{stem}.psd")), bytes)
                        .with_context(|| format!("failed to export {}", page.label))?;
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
        JobOutcome {
            stopped: stop.stopped(),
            error: result.err().map(|error| error.to_string()),
            ..JobOutcome::default()
        },
    );
}

fn export_psd(
    snapshot: &SceneSnapshot,
    renderer: &Renderer,
    page: EntityId,
    locale: Option<&LanguageTag>,
    rendered: image::RgbaImage,
) -> Result<Vec<u8>> {
    let page_value = snapshot.page(page)?.page()?;
    let mut document = PsdDocument {
        width: rendered.width(),
        height: rendered.height(),
        ..PsdDocument::default()
    };
    for entity in snapshot.descendants(page)? {
        let entity = entity.id();
        let Some(source) = snapshot.component::<SourceText>(entity, "default")? else {
            continue;
        };
        let translated = locale
            .map(|locale| snapshot.component::<Translation>(entity, locale.as_str()))
            .transpose()?
            .flatten();
        let text = translated
            .as_ref()
            .map(|translation| translation.text.value.as_str())
            .unwrap_or(source.text.value.as_str());
        if text.is_empty() {
            continue;
        }
        let Some(geometry) = snapshot.component::<Geometry>(entity, "default")? else {
            continue;
        };
        let (x, y, width, height, rotation) = geometry_frame(&geometry)?;
        let typography = snapshot.component::<Typography>(entity, "default")?;
        let analysis = snapshot.component::<OcrAnalysis>(entity, "default")?;
        let preferred_font = typography
            .as_ref()
            .and_then(|typography| typography.preferred_font.as_deref());
        let post_script = renderer.resources().fonts().resolve_post_script_name(
            preferred_font,
            text,
            locale.or(source.language.as_ref()),
        )?;
        let font_index = document
            .fonts
            .iter()
            .position(|font| font == &post_script)
            .unwrap_or_else(|| {
                document.fonts.push(post_script);
                document.fonts.len() - 1
            });
        let source_direction = analysis
            .as_ref()
            .and_then(|analysis| match analysis.direction {
                TextDirection::Horizontal => Some(PsdTextDirection::Horizontal),
                TextDirection::Vertical => Some(PsdTextDirection::Vertical),
                TextDirection::Auto => None,
            });
        let rendered_direction = typography
            .as_ref()
            .and_then(|typography| typography.writing_mode)
            .map(|mode| match mode {
                WritingMode::Horizontal => PsdTextDirection::Horizontal,
                WritingMode::Vertical => PsdTextDirection::Vertical,
            })
            .or(source_direction);
        let font_size = typography
            .as_ref()
            .and_then(|typography| typography.size)
            .unwrap_or(16.0);
        document.text_blocks.push(PsdTextBlock {
            id: entity.to_string(),
            x,
            y,
            width,
            height,
            translation: Some(text.to_owned()),
            style: Some(PsdTextStyle {
                font_families: preferred_font.into_iter().map(str::to_owned).collect(),
                font_size: Some(font_size),
                color: [0, 0, 0, 255],
                effect: Some(PsdShaderEffect {
                    italic: false,
                    bold: false,
                }),
                text_align: Some(
                    match typography
                        .as_ref()
                        .and_then(|typography| typography.alignment)
                        .unwrap_or(TextAlignment::Center)
                    {
                        TextAlignment::Start | TextAlignment::Justify => PsdTextAlign::Left,
                        TextAlignment::Center => PsdTextAlign::Center,
                        TextAlignment::End => PsdTextAlign::Right,
                    },
                ),
            }),
            rotation_deg: Some(rotation),
            source_direction,
            rendered_direction,
            detected_font_size_px: Some(font_size),
            font_index: Some(font_index),
            ..PsdTextBlock::default()
        });
    }

    let source = read_image(snapshot, page, "source")?
        .ok_or_else(|| anyhow!("page {} has no source asset", page_value.label))?;
    let clean = read_image(snapshot, page, "clean")?;
    let text_mask = read_image(snapshot, page, "text-mask")?;
    let coo_mask = read_image(snapshot, page, "coo-mask")?;
    let brush = read_image(snapshot, page, "brush-mask")?;
    let removal_mask = combine_masks(text_mask, coo_mask);
    let rendered = image::DynamicImage::ImageRgba8(rendered);
    let resolved = ResolvedDocument {
        document: &document,
        source: &source,
        segment: removal_mask.as_ref(),
        inpainted: clean.as_ref(),
        rendered: Some(&rendered),
        brush_layer: brush.as_ref(),
        block_images: &HashMap::new(),
    };
    export_document(
        &resolved,
        &PsdExportOptions {
            include_brush_layer: brush.is_some(),
            ..PsdExportOptions::default()
        },
    )
    .map_err(Into::into)
}

fn read_image(
    snapshot: &SceneSnapshot,
    entity: EntityId,
    role: &str,
) -> Result<Option<image::DynamicImage>> {
    let Some(asset) = snapshot.component::<Asset>(entity, role)? else {
        return Ok(None);
    };
    let bytes = snapshot.read_blob(asset.blob)?;
    image::load_from_memory(&bytes)
        .map(Some)
        .map_err(Into::into)
}

fn geometry_frame(geometry: &Geometry) -> Result<(f32, f32, f32, f32, f32)> {
    let first = geometry
        .points
        .first()
        .ok_or_else(|| anyhow!("text geometry is empty"))?;
    if geometry.points.len() == 4 {
        let right = &geometry.points[1];
        let bottom = &geometry.points[2];
        let width = (right.x - first.x).hypot(right.y - first.y);
        let height = (bottom.x - right.x).hypot(bottom.y - right.y);
        if width > 0.0 && height > 0.0 {
            let center_x = geometry.points.iter().map(|point| point.x).sum::<f64>() * 0.25;
            let center_y = geometry.points.iter().map(|point| point.y).sum::<f64>() * 0.25;
            return Ok((
                (center_x - width * 0.5) as f32,
                (center_y - height * 0.5) as f32,
                width as f32,
                height as f32,
                (right.y - first.y).atan2(right.x - first.x).to_degrees() as f32,
            ));
        }
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
    for point in &geometry.points[1..] {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    Ok((
        min_x as f32,
        min_y as f32,
        (max_x - min_x) as f32,
        (max_y - min_y) as f32,
        0.0,
    ))
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
