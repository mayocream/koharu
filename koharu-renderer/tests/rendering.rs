use std::path::{Path, PathBuf};

use anyhow::Result;
use fontdb::Language;
use image::RgbaImage;
use koharu_renderer::{
    font::FontBook,
    layout::{LayoutRequest, Layouter, Orientation, calculate_bounds},
    render::{RenderRequest, Renderer},
};
use swash::text::Script;

fn output_dir() -> PathBuf {
    let path = Path::new(env!("CARGO_WORKSPACE_DIR"))
        .join("target")
        .join("tests");

    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn test_horizontal_text_rendering() -> Result<()> {
    let mut fontbook = FontBook::new();
    let fonts = fontbook
        .filter_by_language(
    &[Language::Chinese_PeoplesRepublicOfChina,
                Language::English_UnitedStates])
        .iter()
        .filter_map(|face| fontbook.font(face).ok())
        .collect::<Vec<_>>();

    let font_size = 50.0;

    let mut layouter = Layouter::new();
    let request = LayoutRequest {
        text: "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。何でも薄暗いじめじめした所でニャーニャー泣いていた事だけは記憶している。吾輩はここで始めて人間というものを見た。しかもあとで聞くとそれは書生という人間中で一番獰悪な種族であったそうだ。",
        fonts: &fonts,
        font_size,
        line_height: 60.0,
        script: Script::Han,
        max_primary_axis: 1000.0,
        direction: Orientation::Horizontal,
    };

    let layout = layouter.layout(&request)?;

    let mut image = RgbaImage::new(1000, 500);
    image.fill(255);

    let mut renderer = Renderer::new();
    let mut render_request = RenderRequest {
        layout: &layout,
        image: &mut image,
        x: 0.0,
        y: 50.0,
        font_size,
        color: [0, 0, 0, 255],
    };

    renderer.render(&mut render_request)?;

    assert!(image.pixels().any(|p| p.0 != [255, 255, 255, 255]));

    image.save(output_dir().join("test_horizontal_rendering.png"))?;

    Ok(())
}

#[test]
fn test_vertical_text_rendering() -> Result<()> {
    let mut fontbook = FontBook::new();
    let font_families = vec!["Microsoft Jhenghei".to_string(), "Microsoft YaHei".to_string(), "Arial".to_string(), "Yu Mincho".to_string()];
    let fonts = fontbook
        .filter_by_families(&font_families, &[
                //Language::Japanese_Japan,
                Language::Chinese_PeoplesRepublicOfChina,
                Language::Chinese_Taiwan,
                Language::Chinese_HongKongSAR,
                Language::English_UnitedStates,
            ])
        .iter()
        .filter_map(|face| fontbook.font(face).ok())
        .collect::<Vec<_>>();

    let font_size = 50.0;

    let mut layouter = Layouter::new();
    let request = LayoutRequest {
        text: "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。何でも薄暗いじめじめした所でニャーニャー泣いていた事だけは記憶している。吾輩はここで始めて人間というものを見た。しかもあとで聞くとそれは書生という人間中で一番獰悪な種族であったそうだ。",
        fonts: &fonts,
        font_size,
        line_height: 60.0,
        script: Script::Han,
        max_primary_axis: 1000.0,
        direction: Orientation::Vertical,
    };

    let layout = layouter.layout(&request)?;

    let (min_x, min_y, max_x, max_y) = calculate_bounds(&layout);
    let width = (max_x - min_x).ceil() as u32;
    let height = (max_y - min_y).ceil() as u32;

    let mut image = RgbaImage::new(width.max(500), height.max(1000));
    image.fill(255);

    let mut renderer = Renderer::new();
    let mut render_request = RenderRequest {
        layout: &layout,
        image: &mut image,
        x: 500.0 - font_size,
        y: 50.0,
        font_size,
        color: [0, 0, 0, 255],
    };

    renderer.render(&mut render_request)?;

    assert!(image.pixels().any(|p| p.0 != [255, 255, 255, 255]));

    image.save(output_dir().join("test_vertical_rendering.png"))?;

    Ok(())
}
