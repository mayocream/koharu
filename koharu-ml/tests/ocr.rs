use std::path::Path;

use koharu_ml::comic_text_detector::extract_text_block_regions;
use koharu_ml::paddleocr_vl::{PaddleOcrVl, PaddleOcrVlTask};
use koharu_ml::{TextDirection, TextRegion};

mod support;

#[tokio::test]
#[ignore]
async fn paddleocr_vl_reads_dialog_image_via_default_block_path() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let image = image::open(fixtures.join("1.jpg"))?.crop_imm(66, 26, 270, 48);
    let block = TextRegion {
        x: 0.0,
        y: 0.0,
        width: image.width() as f32,
        height: image.height() as f32,
        line_polygons: Some(vec![[
            [0.0, 0.0],
            [image.width() as f32, 0.0],
            [image.width() as f32, image.height() as f32],
            [0.0, image.height() as f32],
        ]]),
        source_direction: Some(TextDirection::Horizontal),
        detector: Some("pp-doclayout-v3".to_string()),
        ..Default::default()
    };

    let regions = extract_text_block_regions(&image, &block);
    let runtime = support::cpu_runtime();
    let mut ocr = PaddleOcrVl::load(&runtime, false).await?;
    let results = ocr.inference_images(&regions, PaddleOcrVlTask::Ocr, 128)?;

    assert_eq!(results.len(), 1);
    assert!(
        !results[0].text.trim().is_empty(),
        "OCR result should contain text"
    );

    Ok(())
}
