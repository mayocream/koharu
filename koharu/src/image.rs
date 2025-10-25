use std::ops::Deref;

use image::DynamicImage;
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Default, Clone)]
pub struct SerializableDynamicImage(pub DynamicImage);

#[derive(Serialize, Deserialize)]
struct RawImageData {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl From<&DynamicImage> for RawImageData {
    fn from(image: &DynamicImage) -> Self {
        let rgba = image.to_rgba8();
        RawImageData {
            width: rgba.width(),
            height: rgba.height(),
            data: rgba.into_raw(),
        }
    }
}

impl From<&RawImageData> for DynamicImage {
    fn from(raw: &RawImageData) -> Self {
        DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(raw.width, raw.height, raw.data.clone())
                .expect("Failed to create RgbaImage from raw data"),
        )
    }
}

impl Serialize for SerializableDynamicImage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let raw: RawImageData = (&self.0).into();
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableDynamicImage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = &RawImageData::deserialize(deserializer)?;
        Ok(SerializableDynamicImage(raw.into()))
    }
}

impl Deref for SerializableDynamicImage {
    type Target = DynamicImage;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<DynamicImage> for SerializableDynamicImage {
    fn from(image: DynamicImage) -> Self {
        SerializableDynamicImage(image)
    }
}

impl From<SerializableDynamicImage> for DynamicImage {
    fn from(wrapper: SerializableDynamicImage) -> Self {
        wrapper.0
    }
}
