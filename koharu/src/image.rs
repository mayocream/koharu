use std::ops::Deref;

use image::{ColorType, DynamicImage, codecs::webp::WebPEncoder};
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Default, Clone)]
pub struct SerializableDynamicImage(pub DynamicImage);

#[derive(Serialize, Deserialize)]
pub struct Webp {
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

impl TryFrom<&DynamicImage> for Webp {
    type Error = image::ImageError;

    fn try_from(image: &DynamicImage) -> Result<Self, image::ImageError> {
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        let raw = rgba.into_raw();

        let mut buf = Vec::new();
        let enc = WebPEncoder::new_lossless(&mut buf);
        enc.encode(&raw, width, height, ColorType::Rgba8.into())?;

        Ok(Webp { data: buf })
    }
}

impl TryFrom<&Webp> for DynamicImage {
    type Error = image::ImageError;

    fn try_from(raw: &Webp) -> Result<Self, image::ImageError> {
        image::load_from_memory(&raw.data)
    }
}

impl Serialize for SerializableDynamicImage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let raw: Webp = (&self.0).try_into().map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableDynamicImage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = &Webp::deserialize(deserializer)?;
        let img: DynamicImage = raw.try_into().map_err(serde::de::Error::custom)?;
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
