use std::path::Path;

use image::GenericImageView;
use koharu_ml::comic_text_detector::ComicTextDetector;

#[tokio::test]
async fn comic_text_detector() -> anyhow::Result<()> {
    let model = ComicTextDetector::load(false).await?;

    let img = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let detection = model.inference(&img)?;

    assert!(
        !detection.text_blocks.is_empty(),
        "expected CTD blocks, got line_polygons={}, mask_pixels={}",
        detection.line_polygons.len(),
        detection.mask.iter().filter(|&&v| v > 0u8).count()
    );
    assert!(
        detection.mask.iter().any(|&v| v > 0u8),
        "expected CTD mask, got line_polygons={}",
        detection.line_polygons.len()
    );
    assert_eq!(detection.shrink_map.dimensions(), img.dimensions());
    assert_eq!(detection.threshold_map.dimensions(), img.dimensions());
    assert!(detection.line_polygons.iter().all(|line| line.len() == 4));
    assert!(
        detection
            .text_blocks
            .iter()
            .any(|block| block.detector.as_deref() == Some("ctd"))
    );

    Ok(())
}
