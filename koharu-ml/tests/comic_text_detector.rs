use std::path::Path;

use koharu_ml::{comic_text_detector::ComicTextDetector};

#[tokio::test]
async fn comic_text_detector() -> anyhow::Result<()> {
    let model = ComicTextDetector::load(false).await?;

    let img = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let (boxes, mask) = model.inference(&img)?;

    assert!(!boxes.is_empty());
    assert!(mask.iter().any(|&v| v > 0u8));

    Ok(())
}
