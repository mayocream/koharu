use comic_text_detector::ComicTextDetector;
use image::{DynamicImage, RgbaImage};
use lama::Lama;
use manga_ocr::MangaOCR;
use ort::execution_providers::CUDAExecutionProvider;
use rfd::{FileDialog, MessageDialog, MessageLevel};
use slint::{ComponentHandle, VecModel};
use std::sync::{Arc, Mutex};
use std::thread;

slint::include_modules!();

impl From<&Image> for DynamicImage {
    fn from(image: &Image) -> Self {
        let width = image.width as u32;
        let height = image.height as u32;

        let buffer = image
            .source
            .to_rgba8()
            .expect("Failed to convert Slint image to RGBA8");

        let rgba_image = RgbaImage::from_raw(width, height, buffer.as_bytes().to_vec())
            .expect("Failed to create RgbaImage from raw buffer");

        DynamicImage::ImageRgba8(rgba_image)
    }
}

pub fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

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

    let ctd = Arc::new(Mutex::new(ComicTextDetector::new()?));
    let _manga_ocr = MangaOCR::new()?;
    let _lama = Lama::new()?;

    let app = App::new()?;
    let logic = app.global::<Logic>();
    let app_weak = app.as_weak();

    logic.on_open_file(move || {
        let files = FileDialog::new()
            .add_filter("images", &["png", "jpg", "jpeg", "webp"])
            .pick_files()
            .expect("No files selected");
        let mut images: Vec<Image> = files
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

        // Sort images by name
        images.sort_by_key(|image| image.name.clone());

        VecModel::from_slice(&images)
    });
    logic.on_open_external(|path| open::that(path).expect("Failed to open path"));
    logic.on_detect(move |image| {
        let image = DynamicImage::from(&image);
        let ctd = ctd.clone();
        let app_weak = app_weak.clone();

        thread::spawn(move || {
            let mut ctd = ctd.lock().unwrap();
            let result = ctd.inference(&image, 0.5, 0.5).unwrap();
            let mut text_blocks: Vec<TextBlock> = result
                .bboxes
                .into_iter()
                .map(|bbox| TextBlock {
                    x: bbox.xmin.round() as i32,
                    y: bbox.ymin.round() as i32,
                    width: (bbox.xmax - bbox.xmin).round() as i32,
                    height: (bbox.ymax - bbox.ymin).round() as i32,
                    confidence: bbox.confidence,
                    ..Default::default()
                })
                .collect();

            text_blocks.sort_by(|a, b| {
                (a.y + a.height / 2)
                    .partial_cmp(&(b.y + b.height / 2))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            app_weak
                .upgrade_in_event_loop(move |app| {
                    app.global::<State>()
                        .set_text_blocks(VecModel::from_slice(&text_blocks));
                })
                .unwrap();
        });
    });

    app.run()?;

    Ok(())
}
