use std::path::PathBuf;

use anyhow::Result;
use koharu_renderer::{
    font::{FamilyName, Font, FontBook, Properties},
    layout::{TextLayout, WritingMode},
    renderer::{RenderOptions, SkiaRenderer},
};

const SAMPLE_TEXT: &str = "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。何でも薄暗いじめじめした所でニャーニャー泣いていた事だけは記憶している。吾輩はここで始めて人間というものを見た。しかもあとで聞くとそれは書生という人間中で一番獰悪な種族であったそうだ。";
const SAMPLE_TEXT_ZH_CN: &str = "《我是猫》是日本作家夏目漱石创作的长篇小说，也是其代表作，它确立了夏目漱石在文学史上的地位。作品淋漓尽致地反映了二十世纪初，日本中小资产阶级的思想和生活，尖锐地揭露和批判了明治“文明开化”的资本主义社会。小说采用幽默、讽刺、滑稽的手法，借助一只猫的视觉、听觉、感觉，嘲笑了明治时代知识分子空虚的精神生活，小说构思奇巧，描写夸张，结构灵活，具有鲜明的艺术特色。";

fn output_dir() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tests");
    let _ = std::fs::create_dir_all(&path);
    path
}

fn font(family_name: &str) -> Result<Font> {
    let book = FontBook::new();
    book.query(
        &[FamilyName::Title(family_name.to_string())],
        &Properties::default(),
    )
}

fn non_bg_y_bounds(img: &image::RgbaImage, bg: [u8; 4]) -> Option<(u32, u32)> {
    let mut min_y = u32::MAX;
    let mut max_y = 0u32;
    let mut any = false;

    for (x, y, p) in img.enumerate_pixels() {
        let _ = x;
        if p.0 != bg {
            any = true;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    any.then_some((min_y, max_y))
}

#[test]
#[ignore]
fn render_horizontal() -> Result<()> {
    let font = font("Yu Gothic")?;
    let lines = TextLayout::new(&font, 24.0)
        .with_max_width(1000.0)
        .run(SAMPLE_TEXT)?;

    let img = SkiaRenderer::new().render(
        &lines,
        WritingMode::Horizontal,
        &font,
        &RenderOptions {
            font_size: 24.0,
            padding: 0.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    assert!(img.pixels().any(|p| p.0 != [255, 255, 255, 255]));
    img.save(output_dir().join("horizontal.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn render_vertical() -> Result<()> {
    let font = font("Yu Gothic")?;
    let lines = TextLayout::new(&font, 24.0)
        .with_writing_mode(WritingMode::VerticalRl)
        .with_max_height(1000.0)
        .run(SAMPLE_TEXT)?;

    let img = SkiaRenderer::new().render(
        &lines,
        WritingMode::VerticalRl,
        &font,
        &RenderOptions {
            font_size: 24.0,
            padding: 0.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    assert!(img.pixels().any(|p| p.0 != [255, 255, 255, 255]));
    img.save(output_dir().join("vertical.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn vertical_flows_top_to_bottom() -> Result<()> {
    let font = font("Yu Gothic")?;

    // Repeated CJK characters so vertical advances are obvious and stable.
    let text = "\u{65E5}\u{672C}\u{8A9E}".repeat(40);
    let layout = TextLayout::new(&font, 24.0)
        .with_writing_mode(WritingMode::VerticalRl)
        // Keep it in a single column so we can reason about Y extents.
        .with_max_height(10_000.0)
        .run(&text)?;

    let img = SkiaRenderer::new().render(
        &layout,
        WritingMode::VerticalRl,
        &font,
        &RenderOptions {
            font_size: 24.0,
            padding: 0.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    let (min_y, max_y) =
        non_bg_y_bounds(&img, [255, 255, 255, 255]).expect("expected non-background pixels");

    // If vertical pen advances are applied with the wrong sign, almost all ink ends up near the
    // top edge with a large empty region below. With correct top-to-bottom flow, ink should span
    // most of the image height.
    assert!(
        min_y < img.height() / 5,
        "ink starts too low (min_y={min_y})"
    );
    assert!(
        max_y > (img.height() * 3) / 5,
        "ink does not reach far enough down (max_y={max_y}, height={})",
        img.height()
    );

    Ok(())
}

#[test]
#[ignore]
fn render_horizontal_simplified_chinese() -> Result<()> {
    let font = font("Microsoft YaHei")?;
    let lines = TextLayout::new(&font, 24.0)
        .with_max_width(1000.0)
        .run(SAMPLE_TEXT_ZH_CN)?;

    let img = SkiaRenderer::new().render(
        &lines,
        WritingMode::Horizontal,
        &font,
        &RenderOptions {
            font_size: 24.0,
            padding: 0.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    assert!(img.pixels().any(|p| p.0 != [255, 255, 255, 255]));
    img.save(output_dir().join("horizontal_simplified_chinese.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn render_vertical_simplified_chinese() -> Result<()> {
    let font = font("Microsoft YaHei")?;
    let lines = TextLayout::new(&font, 24.0)
        .with_writing_mode(WritingMode::VerticalRl)
        .with_max_height(1000.0)
        .run(SAMPLE_TEXT_ZH_CN)?;

    let img = SkiaRenderer::new().render(
        &lines,
        WritingMode::VerticalRl,
        &font,
        &RenderOptions {
            font_size: 24.0,
            padding: 0.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    assert!(img.pixels().any(|p| p.0 != [255, 255, 255, 255]));
    img.save(output_dir().join("vertical_simplified_chinese.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn render_rgba_text() -> Result<()> {
    let font = font("Yu Gothic")?;
    let lines = TextLayout::new(&font, 24.0)
        .with_max_width(1000.0)
        .run(SAMPLE_TEXT)?;

    let img = SkiaRenderer::new().render(
        &lines,
        WritingMode::Horizontal,
        &font,
        &RenderOptions {
            font_size: 24.0,
            padding: 0.0,
            background: Some([255, 255, 255, 255]),
            color: [237, 178, 6, 255],
            ..Default::default()
        },
    )?;

    assert!(img.pixels().any(|p| p.0 != [255, 255, 255, 255]));
    img.save(output_dir().join("rgba_text.png"))?;
    Ok(())
}
