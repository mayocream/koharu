use std::path::PathBuf;

use anyhow::Result;
use candle_core::Device;
use koharu_models::manga_ocr_candle::MangaOcr;

#[test]
fn manga_ocr_candle_smoke_test() -> Result<()> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should have a parent directory")
        .to_path_buf();
    let model_dir = workspace_root.join("temp/manga-ocr-base");
    let image_path = workspace_root.join("temp/manga-ocr/manga_ocr/assets/example.jpg");
    assert!(
        model_dir.exists(),
        "expected model dir at {}",
        model_dir.display()
    );
    assert!(
        image_path.exists(),
        "expected example image at {}",
        image_path.display()
    );

    let model = MangaOcr::from_dir(model_dir, Some(Device::new_cuda(0)?))?;
    let image = image::open(image_path)?;
    let text = model.infer(&image)?;
    assert!(!text.trim().is_empty(), "OCR output should not be empty");

    println!("OCR Output: {}", text);

    Ok(())
}
