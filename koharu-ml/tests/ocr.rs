use std::path::Path;

use koharu_ml::mit48px_ocr::Mit48pxOcr;
use koharu_types::TextBlock;

#[tokio::test]
#[ignore]
async fn mit48px_reads_dialog_image_via_default_block_path() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let image = image::open(fixtures.join("1.jpg"))?.crop_imm(66, 26, 270, 48);
    let block = TextBlock {
        x: 0.0,
        y: 0.0,
        width: image.width() as f32,
        height: image.height() as f32,
        ..Default::default()
    };

    let ocr = Mit48pxOcr::load(false).await?;
    let results = ocr.inference_text_blocks(&image, &[block])?;

    assert_eq!(results.len(), 1);
    assert!(
        !results[0].text.trim().is_empty(),
        "OCR result should contain text"
    );
    assert!(
        results[0].text.contains("対策"),
        "unexpected OCR output: {}",
        results[0].text
    );

    Ok(())
}
