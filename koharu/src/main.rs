// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;
use ort::execution_providers::CUDAExecutionProvider;
use rfd::{FileDialog, MessageDialog, MessageLevel};
use slint::VecModel;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("Panic")
            .set_description(&msg)
            .show();
    }));

    ort::init()
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])
        .commit()?;

    let _ctd = ComicTextDetector::new()?;
    let _manga_ocr = MangaOCR::new()?;
    let _lama = Lama::new()?;

    let app = App::new()?;
    let weak = app.as_weak();
    let logic = app.global::<Logic>();

    logic.on_open_file(move || {
        let files = FileDialog::new()
            .add_filter("images", &["png", "jpg", "jpeg", "webp"])
            .pick_files();
        if let Some(files) = files {
            let images: Vec<Image> = files
                .into_iter()
                .filter_map(|path| {
                    slint::Image::load_from_path(&path)
                        .map(|img| {
                            let size = img.size();
                            Image {
                                source: img,
                                width: size.width as i32,
                                height: size.height as i32,
                                path: path.to_string_lossy().to_string().into(),
                                name: path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string()
                                    .split(".")
                                    .next()
                                    .unwrap_or_default()
                                    .into(),
                            }
                        })
                        .ok()
                })
                .collect();
            weak.upgrade()
                .expect("Failed to upgrade weak reference")
                .global::<State>()
                .set_images(VecModel::from_slice(&images));
        }
    });
    logic.on_open_external(|path| open::that(path).expect("Failed to open path"));

    app.run()?;

    Ok(())
}
