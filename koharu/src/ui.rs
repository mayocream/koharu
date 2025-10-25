use std::{ops::Deref, path::PathBuf};

use image::DynamicImage;
use slint::{Model, SharedString};

use crate::{document, image::SerializableDynamicImage};

slint::include_modules!();

impl From<&slint::Image> for SerializableDynamicImage {
    fn from(image: &slint::Image) -> Self {
        let buffer = image
            .to_rgba8()
            .expect("Failed to convert Slint image to RGBA8");

        let rgba = image::RgbaImage::from_raw(
            image.size().width,
            image.size().height,
            buffer.as_bytes().to_vec(),
        )
        .expect("Failed to create RgbaImage from raw buffer");

        DynamicImage::ImageRgba8(rgba).into()
    }
}

impl From<&SerializableDynamicImage> for slint::Image {
    fn from(image: &SerializableDynamicImage) -> Self {
        let rgba = image.deref().to_rgba8();
        let width = rgba.width();
        let height = rgba.height();

        slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
            &rgba.into_raw(),
            width,
            height,
        ))
    }
}

impl From<&TextBlock> for document::TextBlock {
    fn from(block: &TextBlock) -> Self {
        document::TextBlock {
            x: block.x as u32,
            y: block.y as u32,
            width: block.width as u32,
            height: block.height as u32,
            confidence: block.confidence,
            text: block.text.to_string().into(),
            translation: block.translation.to_string().into(),
        }
    }
}

impl From<&document::TextBlock> for TextBlock {
    fn from(block: &document::TextBlock) -> Self {
        TextBlock {
            x: block.x as i32,
            y: block.y as i32,
            width: block.width as i32,
            height: block.height as i32,
            confidence: block.confidence,
            text: SharedString::from(block.text.as_deref().unwrap_or_default()),
            translation: SharedString::from(block.translation.as_deref().unwrap_or_default()),
        }
    }
}

impl From<&Image> for document::Image {
    fn from(image: &Image) -> Self {
        document::Image {
            source: (&image.source).into(),
            width: image.width as u32,
            height: image.height as u32,
            path: PathBuf::from(image.path.as_str()),
            name: image.name.to_string(),
        }
    }
}

impl From<&document::Image> for Image {
    fn from(image: &document::Image) -> Self {
        Image {
            source: (&image.source).into(),
            width: image.width as i32,
            height: image.height as i32,
            path: image.path.to_string_lossy().to_string().into(),
            name: image.name.clone().into(),
        }
    }
}

impl From<&Document<'_>> for document::Document {
    fn from(doc: &Document) -> Self {
        let segment = doc.get_segment();

        document::Document {
            image: (&doc.get_image()).into(),
            text_blocks: doc
                .get_text_blocks()
                .iter()
                .map(|block| (&block).into())
                .collect(),
            // segment is optional, Slint doesn't support optional type yet
            // refer: https://github.com/slint-ui/slint/issues/5164
            segment: match segment.size().width {
                0 => None,
                _ => Some((&segment).into()),
            },
        }
    }
}
