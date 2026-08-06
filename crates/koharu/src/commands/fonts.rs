use std::sync::LazyLock;

use anyhow::{Context as _, anyhow};
use koharu_renderer::{
    FontAxisRange as RenderFontAxisRange, FontFaceStyle as RenderFontStyle,
    FontSource as RenderFontSource, RasterOptions, Rasterizer, SceneRenderer,
};
use serde::Serialize;
use specta::Type;
use tauri::ipc::IpcResponse;

use super::Error;

#[derive(Clone, Debug, Serialize, Type)]
pub struct FontFamily {
    pub name: String,
    pub primary_script: Option<String>,
    pub scripts: Vec<String>,
    pub primary_language: Option<String>,
    pub languages: Vec<String>,
    pub faces: Vec<FontFace>,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct FontFace {
    pub name: String,
    pub postscript_name: String,
    pub weight: u16,
    pub weight_range: Option<FontRange>,
    pub stretch: u16,
    pub stretch_range: Option<FontRange>,
    pub style: FontStyle,
    pub source: FontSource,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
pub struct FontRange {
    pub minimum: u16,
    pub maximum: u16,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Copy, Debug, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    System,
    Bundled,
}

#[derive(Type)]
#[specta(transparent)]
pub(crate) struct FontPreviewBytes(#[specta(type = Vec<u8>)] Vec<u8>);

impl IpcResponse for FontPreviewBytes {
    fn body(self) -> tauri::Result<tauri::ipc::InvokeResponseBody> {
        Ok(self.0.into())
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_fonts() -> std::result::Result<Vec<FontFamily>, Error> {
    Ok(SceneRenderer::available_fonts()
        .await?
        .into_iter()
        .map(|family| FontFamily {
            name: family.family_name,
            primary_script: family.primary_script,
            scripts: family.scripts,
            primary_language: family.primary_language,
            languages: family.languages,
            faces: family
                .faces
                .into_iter()
                .map(|face| FontFace {
                    name: face.font_name,
                    postscript_name: face.post_script_name,
                    weight: face.weight,
                    weight_range: face.weight_range.map(FontRange::from),
                    stretch: face.stretch,
                    stretch_range: face.stretch_range.map(FontRange::from),
                    style: match face.style {
                        RenderFontStyle::Normal => FontStyle::Normal,
                        RenderFontStyle::Italic => FontStyle::Italic,
                        RenderFontStyle::Oblique => FontStyle::Oblique,
                    },
                    source: match face.source {
                        RenderFontSource::System => FontSource::System,
                        RenderFontSource::Bundled => FontSource::Bundled,
                    },
                })
                .collect(),
        })
        .collect())
}

impl From<RenderFontAxisRange> for FontRange {
    fn from(value: RenderFontAxisRange) -> Self {
        Self {
            minimum: value.minimum,
            maximum: value.maximum,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_font_preview(
    postscript_name: String,
) -> std::result::Result<FontPreviewBytes, Error> {
    let preview = SceneRenderer::font_preview(&postscript_name).await?;
    let bytes = tokio::task::spawn_blocking(move || {
        static RASTERIZER: LazyLock<std::result::Result<Rasterizer, String>> =
            LazyLock::new(|| Rasterizer::new().map_err(|error| error.to_string()));
        let rasterizer = RASTERIZER
            .as_ref()
            .map_err(|error| anyhow!(error.clone()))?;
        let (width, height) = preview.size();
        let image = rasterizer
            .rasterize_scene(
                preview.scene(),
                width,
                height,
                [0, 0, 0, 0],
                RasterOptions::default(),
            )
            .context("failed to rasterize the font preview")?;
        Ok::<_, anyhow::Error>(
            webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height())
                .encode(85.0)
                .to_vec(),
        )
    })
    .await
    .context("font preview worker stopped unexpectedly")??;
    Ok(FontPreviewBytes(bytes))
}
