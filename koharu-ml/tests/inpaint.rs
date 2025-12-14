use std::path::Path;

use image::GenericImageView;
use koharu_ml::lama::Lama;

#[tokio::test]
async fn lama_inpainting_updates_masked_region() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let lama = Lama::load(false).await?;
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
