use std::path::Path;

use koharu_ml::manga_text_segmentation_2025::MangaTextSegmentation;

mod support;

#[tokio::test]
#[ignore = "requires model download and is not critical for CI"]
async fn manga_text_segmentation_2025() -> anyhow::Result<()> {
    let runtime = support::cpu_runtime();
    let model = MangaTextSegmentation::load(&runtime, false).await?;
    let image = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let probability_map = model.inference(&image)?;

    assert_eq!(probability_map.width, image.width());
    assert_eq!(probability_map.height, image.height());
    assert_eq!(
        probability_map.values.len(),
        (image.width() * image.height()) as usize
    );
    assert!(
        probability_map.max_value() > 0.05,
        "expected non-trivial probabilities, max={}",
        probability_map.max_value()
    );
    assert!(
        probability_map.values.iter().any(|&value| value > 0.1),
        "expected some text-like positive predictions"
    );

    let mask = probability_map.threshold(0.1)?;
    assert!(mask.pixels().any(|pixel| pixel[0] > 0));

    Ok(())
}
