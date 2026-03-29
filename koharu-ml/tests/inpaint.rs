use std::path::Path;

use image::GenericImageView;
use koharu_core::TextBlock;
use koharu_ml::lama::Lama;

mod support;

#[tokio::test]
#[ignore]
async fn lama_inpainting_updates_masked_region() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let models_root = support::default_models_root();

    let lama = Lama::load(false, &models_root).await?;
    let base = image::open(fixtures.join("image.jpg"))?;
    let mask = image::open(fixtures.join("mask.png"))?;

    let output = lama.inference(&base, &mask)?;

    assert_eq!(output.dimensions(), base.dimensions());

    let mask = mask.to_luma8();
    let base = base.to_rgb8();
    let output = output.to_rgb8();

    let mut changed = false;
    for ((mask_px, base_px), out_px) in mask.pixels().zip(base.pixels()).zip(output.pixels()) {
        if mask_px.0[0] > 0 && base_px.0 != out_px.0 {
            changed = true;
            break;
        }
    }

    assert!(
        changed,
        "inpainting should change at least one masked pixel"
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn lama_block_aware_inpainting_returns_same_size() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let models_root = support::default_models_root();

    let lama = Lama::load(false, &models_root).await?;
    let base = image::open(fixtures.join("image.jpg"))?;
    let mask = image::open(fixtures.join("mask.png"))?;
    let mask_luma = mask.to_luma8();

    let mut min_x = mask_luma.width();
    let mut min_y = mask_luma.height();
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for (x, y, pixel) in mask_luma.enumerate_pixels() {
        if pixel.0[0] == 0 {
            continue;
        }
        found = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    assert!(found, "mask fixture should contain a non-empty region");

    let block = TextBlock {
        x: min_x as f32,
        y: min_y as f32,
        width: max_x.saturating_sub(min_x).saturating_add(1) as f32,
        height: max_y.saturating_sub(min_y).saturating_add(1) as f32,
        ..Default::default()
    };

    let output = lama.inference_with_blocks(&base, &mask, Some(&[block]))?;
    assert_eq!(output.dimensions(), base.dimensions());
    Ok(())
}
