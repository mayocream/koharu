use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use fontdb::{Family, Query, Stretch, Style, Weight};
use koharu_renderer::{
    FontBook, LayoutRequest, Orientation, LayoutSession, RenderRequest, TextLayouter,
    TextRenderer,
};
use swash::text::Script;

const FALLBACK_FAMILY: [Family<'static>; 1] = [Family::SansSerif];

fn workspace_dir() -> PathBuf {
    if let Ok(value) = std::env::var("CARGO_WORKSPACE_DIR") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR should be provided by cargo");
    Path::new(&manifest)
        .parent()
        .expect("crate should live inside workspace")
        .to_path_buf()
}

#[test]
fn writes_cjk_paragraph_preview() -> Result<()> {
    let mut book = FontBook::new();
    let mut layouter = TextLayouter::new();
    let text = "\
静かな夜、翻訳のことばがページを越えて流れ、\n\
月光は小春のレンダラーを照らす。\
";

    let options = LayoutRequest {
        text,
        font_query: Query {
            families: &FALLBACK_FAMILY,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        },
        script: Some(Script::Han),
        font_size: 28.0,
        max_primary_axis: 220.0,
        line_height: 34.0,
        direction: Orientation::Vertical,
    };

    let LayoutSession { font, output } = layouter.layout(&mut book, &options)?;
    let mut renderer = TextRenderer::new();
    let request = RenderRequest {
        font: &font,
        layout: &output,
        foreground: [0, 0, 0, 255],
        background: [255, 255, 255, 255],
    };
    let rendered = renderer.render(&request)?;

    let mut dir = workspace_dir();
    dir.push("target");
    dir.push("renderer");
    fs::create_dir_all(&dir)?;
    let file = dir.join("cjk_paragraph.png");
    rendered.image.save(&file)?;

    assert!(
        file.exists(),
        "render integration test expected to create: {}",
        file.display()
    );
    Ok(())
}
