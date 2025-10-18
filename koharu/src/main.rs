// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;
use ort::execution_providers::CUDAExecutionProvider;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    ort::init()
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])
        .commit()?;

    let _ctd = ComicTextDetector::new()?;
    let _manga_ocr = MangaOCR::new()?;
    let _lama = Lama::new()?;

    let app = App::new()?;
    let logic = app.global::<Logic>();
    logic.on_open(|path| open::that(path).expect("Failed to open path"));

    app.run()?;

    Ok(())
}
