use std::path::Path;

use image::GenericImageView;
use koharu_ml::comic_text_detector::ComicTextDetector;

mod support;

#[tokio::test]
#[ignore = "requires model download and is not critical for CI"]
async fn comic_text_detector() -> anyhow::Result<()> {
    let runtime = support::cpu_runtime();
    let model = ComicTextDetector::load(&runtime, false).await?;

    let img = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let detection = model.inference(&img)?;

    assert!(!detection.text_blocks.is_empty(), "expected CTD boxes");
    assert!(detection.mask.iter().any(|&v| v > 0u8), "expected CTD mask");
    assert_eq!(detection.shrink_map.dimensions(), img.dimensions());
    assert_eq!(detection.threshold_map.dimensions(), img.dimensions());
    assert!(detection.line_polygons.is_empty());
    assert!(
        detection
            .text_blocks
            .iter()
            .all(|block| block.line_polygons.is_none() && block.detector.is_none())
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires model download and is not critical for CI"]
async fn comic_text_detector_segmentation_only() -> anyhow::Result<()> {
    let runtime = support::cpu_runtime();
    let model = ComicTextDetector::load_segmentation_only(&runtime, false).await?;

    let img = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let mask = model.inference_segmentation(&img)?;

    assert_eq!(mask.dimensions(), img.dimensions());
    assert!(
        mask.iter().any(|&value| value > 0),
        "expected CTD segmentation-only mask to contain foreground pixels"
    );

    Ok(())
}
