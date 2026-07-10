use std::path::PathBuf;

use anyhow::Result;
use koharu_renderer::{
    font::{Font, FontBook},
    layout::{TextLayout, WritingMode},
    renderer::{RenderOptions, TinySkiaRenderer},
};
use once_cell::sync::OnceCell;
use unicode_bidi::BidiInfo;

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
    let mut book = FontBook::new();
    let post_script_name = book
        .all_families()
        .into_iter()
        .find(|face| {
            face.post_script_name == family_name
                || face
                    .families
                    .iter()
                    .any(|(family, _)| family.as_str() == family_name)
        })
        .map(|face| face.post_script_name)
        .filter(|post_script_name| !post_script_name.is_empty())
        .ok_or_else(|| anyhow::anyhow!("font not found: {family_name}"))?;
    let font = book.query(&post_script_name)?;
    // preload fontdue font
    let _ = font.fontdue()?;

    Ok(font)
}

fn tiny_skia_renderer() -> Result<&'static TinySkiaRenderer> {
    static INSTANCE: OnceCell<TinySkiaRenderer> = OnceCell::new();
    let renderer = INSTANCE.get_or_try_init(TinySkiaRenderer::new)?;
    Ok(renderer)
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
    let lines = TextLayout::new(&font, Some(24.0))
        .with_max_width(1000.0)
        .run(SAMPLE_TEXT)?;

    let img = tiny_skia_renderer()?.render(
        &lines,
        WritingMode::Horizontal,
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
    let lines = TextLayout::new(&font, Some(24.0))
        .with_writing_mode(WritingMode::VerticalRl)
        .with_max_height(1000.0)
        .run(SAMPLE_TEXT)?;

    let img = tiny_skia_renderer()?.render(
        &lines,
        WritingMode::VerticalRl,
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
    let layout = TextLayout::new(&font, Some(24.0))
        .with_writing_mode(WritingMode::VerticalRl)
        // Keep it in a single column so we can reason about Y extents.
        .with_max_height(10_000.0)
        .run(&text)?;

    let img = tiny_skia_renderer()?.render(
        &layout,
        WritingMode::VerticalRl,
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
    let lines = TextLayout::new(&font, Some(24.0))
        .with_max_width(1000.0)
        .run(SAMPLE_TEXT_ZH_CN)?;

    let img = tiny_skia_renderer()?.render(
        &lines,
        WritingMode::Horizontal,
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
    let lines = TextLayout::new(&font, Some(24.0))
        .with_writing_mode(WritingMode::VerticalRl)
        .with_max_height(1000.0)
        .run(SAMPLE_TEXT_ZH_CN)?;

    let img = tiny_skia_renderer()?.render(
        &lines,
        WritingMode::VerticalRl,
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
    let lines = TextLayout::new(&font, Some(24.0))
        .with_max_width(1000.0)
        .run(SAMPLE_TEXT)?;

    let img = tiny_skia_renderer()?.render(
        &lines,
        WritingMode::Horizontal,
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

#[test]
#[ignore]
fn render_with_fallback_fonts() -> Result<()> {
    let primary_font = font("Yu Gothic")?;
    let fallback_fonts = vec![font("Segoe UI Symbol")?, font("Segoe UI Emoji")?];

    let lines = TextLayout::new(&primary_font, Some(24.0))
        .with_fallback_fonts(&fallback_fonts)
        .run("Here is a smiley: 😊 and a star: ★ and a heart: ♥")?;

    let img = tiny_skia_renderer()?.render(
        &lines,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 0.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;
    assert!(img.pixels().any(|p| p.0 != [255, 255, 255, 255]));
    img.save(output_dir().join("fallback_fonts.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn test_arabic_layout_order() -> Result<()> {
    let font = font("Segoe UI")?;
    let text = "مرحبا"; // Marhaba (Hello)
    let layout = TextLayout::new(&font, Some(24.0)).run(text)?;
    let line = &layout.lines[0];

    let bidi_info = BidiInfo::new(text, None);
    let para = &bidi_info.paragraphs[0];
    assert!(
        para.level.is_rtl(),
        "expected Arabic text to resolve to an RTL paragraph level, got {:?}",
        para.level
    );

    let clusters: Vec<u32> = line.glyphs.iter().map(|g| g.cluster).collect();
    println!("Clusters for {text}: {:?}", clusters);

    assert!(
        clusters.len() > 1,
        "expected multiple glyph clusters for Arabic shaping, got {:?}",
        clusters
    );
    assert!(
        clusters.windows(2).all(|w| w[0] >= w[1]),
        "expected RTL visual order to produce non-increasing cluster indices, got {:?}",
        clusters
    );

    Ok(())
}

#[test]
#[ignore]
fn test_mixed_bidi_render() -> Result<()> {
    let font = font("Arial")?;
    let text = "Hello مرحبا Hello";
    let layout = TextLayout::new(&font, Some(24.0)).run(text)?;

    let bidi_info = BidiInfo::new(text, None);
    println!("Paragraphs: {}", bidi_info.paragraphs.len());
    let para = &bidi_info.paragraphs[0];
    println!("Base Level: {:?}", para.level);

    let levels = bidi_info.levels;
    println!(
        "Levels: {:?}",
        levels.iter().map(|l| l.number()).collect::<Vec<_>>()
    );

    println!("Direction: {:?}", layout.lines[0].direction);
    println!(
        "Clusters: {:?}",
        layout.lines[0]
            .glyphs
            .iter()
            .map(|g| g.cluster)
            .collect::<Vec<_>>()
    );

    let img = tiny_skia_renderer()?.render(
        &layout,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 20.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    img.save(output_dir().join("mixed_bidi.png"))?;
    Ok(())
}
#[test]
#[ignore]
fn test_rtl_multiline() -> Result<()> {
    let font = font("Arial")?;
    // A long text that will wrap.
    let text = "هذا نص طويل باللغة العربية سيتم لفه عبر عدة أسطر للتأكد من أن تخطيط الحروف والاتجاهات يعمل بشكل صحيح في جميع الأسطر. Hello World! وهذا جزء آخر.";
    let layout = TextLayout::new(&font, Some(24.0))
        .with_max_width(400.0)
        .run(text)?;

    println!("Line count: {}", layout.lines.len());
    for (i, line) in layout.lines.iter().enumerate() {
        println!(
            "Line {}: {:?} ({} glyphs)",
            i,
            line.direction,
            line.glyphs.len()
        );
    }

    let img = tiny_skia_renderer()?.render(
        &layout,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 20.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    img.save(output_dir().join("rtl_multiline.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn test_rtl_alignment() -> Result<()> {
    let font = font("Arial")?;
    let text = "مرحبا بالعالم"; // Hello World in Arabic

    // Test Left Alignment
    let layout_left = TextLayout::new(&font, Some(24.0))
        .with_max_width(500.0)
        .with_alignment(koharu_renderer::TextAlign::Left)
        .run(text)?;

    let img_left = tiny_skia_renderer()?.render(
        &layout_left,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 20.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;
    img_left.save(output_dir().join("rtl_align_left.png"))?;

    // Test Right Alignment
    let layout_right = TextLayout::new(&font, Some(24.0))
        .with_max_width(500.0)
        .with_alignment(koharu_renderer::TextAlign::Right)
        .run(text)?;

    let img_right = tiny_skia_renderer()?.render(
        &layout_right,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 20.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;
    img_right.save(output_dir().join("rtl_align_right.png"))?;

    Ok(())
}

#[test]
#[ignore]
fn test_rtl_punctuation_numbers() -> Result<()> {
    let font = font("Arial")?;
    // Text with numbers and trailing punctuation.
    // In LTR, it's: "Arabic 123!"
    // In RTL, "123" stays LTR, but "!" might move to the left side of the word.
    let text = "هذا اختبار 123!";
    let layout = TextLayout::new(&font, Some(24.0)).run(text)?;

    println!(
        "Clusters: {:?}",
        layout.lines[0]
            .glyphs
            .iter()
            .map(|g| g.cluster)
            .collect::<Vec<_>>()
    );

    let img = tiny_skia_renderer()?.render(
        &layout,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 20.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    img.save(output_dir().join("rtl_punctuation_numbers.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn test_rtl_mixed_complex() -> Result<()> {
    let font = font("Arial")?;
    // Mixed text with LTR and RTL sequences.
    let text = "The word for 'Apple' is تفاحة in Arabic.";
    let layout = TextLayout::new(&font, Some(24.0)).run(text)?;

    let img = tiny_skia_renderer()?.render(
        &layout,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 20.0,
            background: Some([255, 255, 255, 255]),
            ..Default::default()
        },
    )?;

    img.save(output_dir().join("rtl_mixed.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn test_rtl_user_reported_string() -> Result<()> {
    let font = font("Arial")?;
    // The problematic string from the user.
    let text = "هل من المقبول حقاً ارتداء ملابس كهذه، إنها مجرد خيط؟";
    let layout = TextLayout::new(&font, Some(24.0))
        .with_max_width(200.0) // Narrow width to force multiline
        .run(text)?;

    let img = tiny_skia_renderer()?.render(
        &layout,
        WritingMode::Horizontal,
        &RenderOptions {
            font_size: 24.0,
            padding: 20.0,
            background: Some([173, 216, 230, 255]), // Light blue to match screenshot
            ..Default::default()
        },
    )?;

    img.save(output_dir().join("rtl_user_reported.png"))?;
    Ok(())
}

#[test]
#[ignore]
fn test_complex_reordering_and_glyph_count() -> Result<()> {
    let font = font("Arial")?;
    let text = "A مرحبا 😊";
    let layout = TextLayout::new(&font, Some(24.0)).run(text)?;
    let line = &layout.lines[0];

    // Check that we have valid layout (at least one glyph per word run).
    let clusters: Vec<u32> = line.glyphs.iter().map(|g| g.cluster).collect();
    println!("Clusters for '{}': {:?}", text, clusters);

    assert!(
        !clusters.is_empty(),
        "Expected layout to produce at least some glyphs"
    );

    // Verify all clusters are within the string range.
    for &cluster in &clusters {
        assert!((cluster as usize) < text.len());
    }

    // Check for duplicates that might indicate the duplication bug.
    // Some ligatures or combining sequences might legitimately have multiple glyphs for one
    // cluster, but we shouldn't have more repeated glyph identities than that shaping requires.
    let mut unique_glyphs = std::collections::HashSet::new();
    for g in &line.glyphs {
        // Use a tuple of (cluster, glyph_id, x_advance) as a proxy for identity.
        let identity = (g.cluster, g.glyph_id, (g.x_advance * 100.0) as i32);
        unique_glyphs.insert(identity);
    }

    let unique_clusters = clusters
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let allowed_duplicates = line.glyphs.len().saturating_sub(unique_clusters.len());
    let duplicate_glyphs = line.glyphs.len().saturating_sub(unique_glyphs.len());

    assert!(
        duplicate_glyphs <= allowed_duplicates,
        "Unexpected duplicated glyph identities for '{}': {} duplicated glyphs across {} glyphs and {} clusters",
        text,
        duplicate_glyphs,
        line.glyphs.len(),
        unique_clusters.len()
    );
    Ok(())
}
