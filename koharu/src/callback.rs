use std::{sync::Arc, thread};

use image::GenericImageView;
use rfd::FileDialog;
use slint::{ComponentHandle, Model, VecModel};

use crate::{
    image::SerializableDynamicImage,
    inference::Inference,
    state,
    ui::{self, App, Logic},
};

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
                let img = image::open(&path).ok()?;
                let (width, height) = img.dimensions();
                Some(ui::Image {
                    source: (&SerializableDynamicImage::from(img)).into(),
                    width: width as i32,
                    height: height as i32,
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
            let image = (&image.source).into();
            let inference = inference.clone();
            let app_weak = app_weak.clone();

            thread::spawn(move || {
                let (blocks, segment) = inference.detect(&image).unwrap();
                app_weak
                    .upgrade_in_event_loop(move |app| {
                        app.global::<ui::Document>()
                            .set_text_blocks(VecModel::from_slice(
                                &blocks
                                    .iter()
                                    .map(|block| block.into())
                                    .collect::<Vec<_>>()
                                    .as_slice(),
                            ));
                        app.global::<ui::Document>().set_segment((&segment).into());
                        app.global::<ui::Viewport>().set_in_progress(false);
                    })
                    .unwrap();
            });
        }
    });

    logic.on_ocr({
        let inference = inference.clone();
        let app_weak = app_weak.clone();

        move |image, text_blocks| {
            let image = (&image.source).into();
            let blocks: Vec<state::TextBlock> =
                text_blocks.iter().map(|block| (&block).into()).collect();
            let inference = inference.clone();
            let app_weak = app_weak.clone();

            thread::spawn(move || {
                let blocks = inference.ocr(&image, &blocks).unwrap();
                app_weak
                    .upgrade_in_event_loop(move |app| {
                        app.global::<ui::Document>()
                            .set_text_blocks(VecModel::from_slice(
                                &blocks
                                    .iter()
                                    .map(|block| block.into())
                                    .collect::<Vec<_>>()
                                    .as_slice(),
                            ));
                        app.global::<ui::Viewport>().set_in_progress(false);
                    })
                    .unwrap();
            });
        }
    });
}
