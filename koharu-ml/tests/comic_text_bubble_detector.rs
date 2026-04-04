use std::path::Path;

use koharu_ml::comic_text_bubble_detector::ComicTextBubbleDetector;

mod support;

#[tokio::test]
#[ignore = "requires model download and is not critical for CI"]
async fn comic_text_bubble_detector() -> anyhow::Result<()> {
    let runtime = support::cpu_runtime();
    let model = ComicTextBubbleDetector::load(&runtime, false).await?;

    let image = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let detection = model.inference(&image)?;

    assert!(
        !detection.detections.is_empty(),
        "expected RT-DETR regions for fixture image"
    );
    assert!(
        !detection.text_blocks.is_empty(),
        "expected RT-DETR text blocks for fixture image"
    );
    assert!(
        detection
            .detections
            .iter()
            .all(|region| region.bbox[2] >= region.bbox[0] && region.bbox[3] >= region.bbox[1]),
        "expected non-inverted RT-DETR boxes"
    );
    assert!(
        detection
            .text_blocks
            .iter()
            .all(|block| block.detector.as_deref() == Some("comic-text-bubble-detector")),
        "expected RT-DETR detector marker on all text blocks"
    );

    Ok(())
}
