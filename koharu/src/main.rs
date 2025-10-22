// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;
use ort::execution_providers::CUDAExecutionProvider;
use rfd::{FileDialog, MessageDialog, MessageLevel};
use slint::{SharedPixelBuffer, VecModel};
use tokio::fs;

slint::include_modules!();

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
        let weak = weak.clone();
        slint::spawn_local(async move {
            let files = FileDialog::new()
                .add_filter("images", &["png", "jpg", "jpeg", "webp"])
                .pick_files();

            if let Some(files) = files {
                let tasks: Vec<_> = files
                    .iter()
                    .map(|path| {
                        let path = path.clone();
                        tokio::spawn(async move {
                            let data = fs::read(&path).await.ok()?;
                            let img = image::load_from_memory(&data).ok()?;
                            Some((path, img))
                        })
                    })
                    .collect();

                let mut images = Vec::new();
                for task in tasks {
                    if let Ok(Some((path, img))) = task.await {
                        images.push(Image {
                            source: slint::Image::from_rgba8(
                                SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                                    img.to_rgba8().as_raw(),
                                    img.width(),
                                    img.height(),
                                ),
                            ),
                            width: img.width() as i32,
                            height: img.height() as i32,
                            path: path.to_string_lossy().to_string().into(),
                            name: path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string()
                                .into(),
                        });
                    }
                }

                weak.upgrade()
                    .expect("Failed to upgrade weak reference")
                    .global::<State>()
                    .set_images(VecModel::from_slice(&images));
            }
        })
        .unwrap();
    });
    logic.on_open_external(|path| open::that(path).expect("Failed to open path"));

    app.run()?;

    Ok(())
}
