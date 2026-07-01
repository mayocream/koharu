use std::path::Path;

use image::GenericImageView;
use koharu_ml::aot_inpainting::AotInpainting;
use koharu_ml::lama::Lama;

mod support;

#[tokio::test]
#[ignore]
async fn lama_inpainting_updates_masked_region() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let runtime = support::cpu_runtime();
    let lama = Lama::load(&runtime, false).await?;
    let base = image::open(fixtures.join("image.jpg"))?;
    let mask = image::open(fixtures.join("mask.png"))?;
    let bubble_mask = image::open(fixtures.join("mask.png"))?;

    let output = lama.inference(&base, &mask, &bubble_mask)?;

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
async fn aot_inpainting_updates_masked_region() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let runtime = support::cpu_runtime();
    let aot = AotInpainting::load(&runtime, false).await?;
    let base = image::open(fixtures.join("image.jpg"))?;
    let mask = image::open(fixtures.join("mask.png"))?;
    let bubble_mask = image::open(fixtures.join("mask.png"))?;

    let output = aot.inference(&base, &mask, &bubble_mask)?;

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
        "AOT inpainting should change at least one masked pixel"
    );
    Ok(())
}
