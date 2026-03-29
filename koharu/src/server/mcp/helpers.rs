use std::io::Cursor;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, ImageFormat};

pub(crate) fn encode_png_base64(img: &DynamicImage, max_size: u32) -> String {
    let img = if img.width().max(img.height()) > max_size {
        img.resize(max_size, max_size, image::imageops::FilterType::Lanczos3)
    } else {
        img.clone()
    };
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding failed");
    BASE64.encode(&buf)
}
