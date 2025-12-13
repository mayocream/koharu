use std::path::Path;

use koharu_ml::manga_ocr::MangaOcr;

#[tokio::test]
async fn manga_ocr_reads_dialog_image() -> anyhow::Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let image = image::open(fixtures.join("dialog.jpg"))?;

    let ocr = MangaOcr::load(false).await?;
    let results = ocr.inference(&[image])?;

    assert_eq!(results.len(), 1);
    assert!(
        !results[0].trim().is_empty(),
        "OCR result should contain text"
    );

    Ok(())
}
