use image::{DynamicImage, RgbaImage};
use rfd::FileDialog;
use slint::{ComponentHandle, Model, SharedPixelBuffer, VecModel};
use std::sync::Arc;
use std::thread;

slint::include_modules!();

use crate::inference::Inference;

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

pub fn setup(app: &App, inference: Arc<Inference>) {
    let logic = app.global::<Logic>();
    let app_weak = app.as_weak();

    logic.on_open_file(|| {
        let files = FileDialog::new()
            .add_filter("images", &["png", "jpg", "jpeg", "webp"])
            .pick_files()
            .unwrap_or_default();

        let mut images = files
            .into_iter()
            .filter_map(|path| {
                let img = slint::Image::load_from_path(&path).ok()?;
                let size = img.size();
                Some(Image {
                    source: img,
                    width: size.width as i32,
                    height: size.height as i32,
                    path: path.to_string_lossy().to_string().into(),
                    name: path.file_stem()?.to_string_lossy().to_string().into(),
                })
            })
            .collect::<Vec<_>>();
        images.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        VecModel::from_slice(&images).into()
    });

    logic.on_open_external(|path| {
        open::that(path.as_str()).ok();
    });

    logic.on_detect({
        let inference = inference.clone();
        let app_weak = app_weak.clone();

        move |image| {
            let image = DynamicImage::from(&image);
            let inference = inference.clone();
            let app_weak = app_weak.clone();

            thread::spawn(move || {
                let (blocks, segment) = inference.detect(&image).unwrap();
                app_weak
                    .upgrade_in_event_loop(move |app| {
                        app.global::<State>()
                            .set_text_blocks(VecModel::from_slice(&blocks));
                        app.global::<State>().set_segment(slint::Image::from_rgba8(
                            SharedPixelBuffer::clone_from_slice(
                                &segment.to_rgba8().into_raw(),
                                segment.width(),
                                segment.height(),
                            ),
                        ));
                    })
                    .unwrap();
            });
        }
    });

    logic.on_ocr({
        let inference = inference.clone();
        let app_weak = app_weak.clone();

        move |image, text_blocks| {
            let image = DynamicImage::from(&image);
            let blocks: Vec<_> = text_blocks.iter().collect();
            let inference = inference.clone();
            let app_weak = app_weak.clone();

            thread::spawn(move || {
                let blocks = inference.ocr(&image, &blocks).unwrap();
                app_weak
                    .upgrade_in_event_loop(move |app| {
                        app.global::<State>()
                            .set_text_blocks(VecModel::from_slice(&blocks));
                    })
                    .unwrap();
            });
        }
    });
}
