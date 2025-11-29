use std::ops::Deref;

use image::{ColorType, DynamicImage, codecs::webp::WebPEncoder};
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Default, Clone)]
pub struct SerializableDynamicImage(pub DynamicImage);

impl Serialize for SerializableDynamicImage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let rgba = self.0.to_rgba8();
        let (width, height) = rgba.dimensions();
        let raw = rgba.into_raw();

        let mut buf = Vec::new();
        let enc = WebPEncoder::new_lossless(&mut buf);
        enc.encode(&raw, width, height, ColorType::Rgba8.into())
            .map_err(serde::ser::Error::custom)?;

        serde_bytes::serialize(&buf, serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableDynamicImage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde_bytes::deserialize(deserializer)?;
        let img = image::load_from_memory(&bytes).map_err(serde::de::Error::custom)?;
        Ok(SerializableDynamicImage(img))
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
