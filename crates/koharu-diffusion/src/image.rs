use std::slice;

use ::image::{GrayImage, ImageBuffer, Rgb, RgbImage};

use crate::{Error, Result, sys};

/// An RGB image borrowing native preview pixels for one callback invocation.
pub type RgbImageView<'a> = ImageBuffer<Rgb<u8>, &'a [u8]>;

fn image_len(width: u32, height: u32, channels: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(Error::ZeroImageDimension);
    }
    usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(height as usize))
        .and_then(|pixels| pixels.checked_mul(channels as usize))
        .filter(|len| *len <= isize::MAX as usize)
        .ok_or(Error::ImageDimensionsOverflow)
}

pub(crate) fn raw_rgb_image(image: &RgbImage) -> Result<sys::sd_image_t> {
    image_len(image.width(), image.height(), 3)?;
    Ok(sys::sd_image_t {
        width: image.width(),
        height: image.height(),
        channel: 3,
        // stable-diffusion.cpp treats generation inputs as read-only even
        // though the C structure predates const-qualified image data.
        data: image.as_raw().as_ptr().cast_mut(),
    })
}

pub(crate) fn raw_gray_image(image: &GrayImage) -> Result<sys::sd_image_t> {
    image_len(image.width(), image.height(), 1)?;
    Ok(sys::sd_image_t {
        width: image.width(),
        height: image.height(),
        channel: 1,
        data: image.as_raw().as_ptr().cast_mut(),
    })
}

pub(crate) fn optional_raw_rgb_image(image: Option<&RgbImage>) -> Result<sys::sd_image_t> {
    image.map_or_else(|| Ok(empty_raw_image()), raw_rgb_image)
}

pub(crate) fn optional_raw_gray_image(image: Option<&GrayImage>) -> Result<sys::sd_image_t> {
    image.map_or_else(|| Ok(empty_raw_image()), raw_gray_image)
}

pub(crate) fn raw_rgb_images(images: &[RgbImage]) -> Result<Vec<sys::sd_image_t>> {
    images.iter().map(raw_rgb_image).collect()
}

pub(crate) unsafe fn copy_rgb_from_raw(raw: &sys::sd_image_t) -> Result<RgbImage> {
    if raw.channel != 3 {
        return Err(Error::UnexpectedNativeImageChannelCount {
            expected: 3,
            actual: raw.channel,
        });
    }
    let len = image_len(raw.width, raw.height, raw.channel)?;
    if raw.data.is_null() {
        return Err(Error::InvalidNativeOutput { kind: "RGB image" });
    }
    let bytes = unsafe { slice::from_raw_parts(raw.data, len) }.to_vec();
    RgbImage::from_raw(raw.width, raw.height, bytes)
        .ok_or(Error::InvalidNativeOutput { kind: "RGB image" })
}

pub(crate) unsafe fn rgb_view_from_raw<'a>(raw: &'a sys::sd_image_t) -> Result<RgbImageView<'a>> {
    if raw.channel != 3 {
        return Err(Error::UnexpectedNativeImageChannelCount {
            expected: 3,
            actual: raw.channel,
        });
    }
    let len = image_len(raw.width, raw.height, raw.channel)?;
    if raw.data.is_null() {
        return Err(Error::InvalidNativeOutput {
            kind: "RGB preview image",
        });
    }
    let bytes = unsafe { slice::from_raw_parts(raw.data, len) };
    ImageBuffer::from_raw(raw.width, raw.height, bytes).ok_or(Error::InvalidNativeOutput {
        kind: "RGB preview image",
    })
}

/// Owned interleaved floating-point audio returned by video generation.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    sample_rate: u32,
    channels: u32,
    sample_count: u64,
    data: Vec<f32>,
}

impl Audio {
    pub fn new(sample_rate: u32, channels: u32, sample_count: u64, data: Vec<f32>) -> Result<Self> {
        let expected = usize::try_from(sample_count)
            .ok()
            .and_then(|samples| samples.checked_mul(channels as usize))
            .filter(|len| *len <= isize::MAX as usize / size_of::<f32>())
            .ok_or(Error::AudioDimensionsOverflow)?;
        if channels == 0 || data.len() != expected {
            return Err(Error::InvalidAudioBuffer {
                sample_count,
                channels,
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            sample_rate,
            channels,
            sample_count,
            data,
        })
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn channels(&self) -> u32 {
        self.channels
    }

    /// Number of samples per channel.
    #[must_use]
    pub const fn sample_count(&self) -> u64 {
        self.sample_count
    }

    #[must_use]
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    #[must_use]
    pub fn into_data(self) -> Vec<f32> {
        self.data
    }

    pub(crate) unsafe fn copy_from_raw(raw: &sys::sd_audio_t) -> Result<Self> {
        let len = usize::try_from(raw.sample_count)
            .ok()
            .and_then(|samples| samples.checked_mul(raw.channels as usize))
            .filter(|len| *len <= isize::MAX as usize / size_of::<f32>())
            .ok_or(Error::AudioDimensionsOverflow)?;
        if raw.data.is_null() && len != 0 {
            return Err(Error::InvalidNativeOutput { kind: "audio" });
        }
        let data = if len == 0 {
            Vec::new()
        } else {
            unsafe { slice::from_raw_parts(raw.data, len) }.to_vec()
        };
        Self::new(raw.sample_rate, raw.channels, raw.sample_count, data)
    }
}

/// Frames and optional audio returned by video generation.
#[derive(Debug, Clone, PartialEq)]
pub struct Video {
    pub frames: Vec<RgbImage>,
    pub audio: Option<Audio>,
    pub fps: u32,
}

pub(crate) const fn empty_raw_image() -> sys::sd_image_t {
    sys::sd_image_t {
        width: 0,
        height: 0,
        channel: 0,
        data: std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use ::image::RgbImage;

    use super::{copy_rgb_from_raw, raw_rgb_image, rgb_view_from_raw};
    use crate::sys;

    #[test]
    fn rejects_zero_sized_rgb_inputs() {
        assert!(raw_rgb_image(&RgbImage::new(0, 1)).is_err());
        assert!(raw_rgb_image(&RgbImage::new(1, 0)).is_err());
    }

    #[test]
    fn converts_rgb_inputs_to_native_views() {
        let image = RgbImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6]).unwrap();
        let raw = raw_rgb_image(&image).unwrap();
        assert_eq!((raw.width, raw.height, raw.channel), (2, 1, 3));
        assert_eq!(raw.data, image.as_raw().as_ptr().cast_mut());
    }

    #[test]
    fn copies_native_outputs_into_rgb_images() {
        let mut pixels = vec![1, 2, 3, 4, 5, 6];
        let raw = sys::sd_image_t {
            width: 2,
            height: 1,
            channel: 3,
            data: pixels.as_mut_ptr(),
        };
        let image = unsafe { copy_rgb_from_raw(&raw) }.unwrap();
        assert_eq!(image.into_raw(), pixels);
    }

    #[test]
    fn preview_views_borrow_native_rgb_pixels() {
        let mut pixels = vec![1, 2, 3];
        let raw = sys::sd_image_t {
            width: 1,
            height: 1,
            channel: 3,
            data: pixels.as_mut_ptr(),
        };
        let view = unsafe { rgb_view_from_raw(&raw) }.unwrap();
        assert_eq!(*view.as_raw(), pixels.as_slice());
        assert_eq!(view.as_raw().as_ptr(), pixels.as_ptr());
    }
}
