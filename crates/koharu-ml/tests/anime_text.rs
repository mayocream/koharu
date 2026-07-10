use std::path::Path;

use koharu_ml::anime_text::AnimeTextDetector;

mod support;

#[tokio::test]
#[ignore = "requires model download and is not critical for CI"]
async fn anime_text_yolo() -> anyhow::Result<()> {
    let runtime = support::cpu_runtime();
    let model = AnimeTextDetector::load(&runtime, false).await?;

    let image = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let detection = model.inference(&image)?;

    assert_eq!(detection.image_width, image.width());
    assert_eq!(detection.image_height, image.height());
    assert!(
        !detection.text_blocks.is_empty(),
        "expected anime text YOLO to detect text blocks"
    );
    assert!(
        detection
            .text_blocks
            .iter()
            .all(|block| block.detector.as_deref() == Some("anime-text-yolo"))
    );

    Ok(())
}
