use std::io::Cursor;

use image::{ImageDecoder, ImageReader};

use crate::{Error, PixelSize, Result};

pub(crate) fn inspect(
    bytes: &[u8],
    require_single_channel: bool,
    max_width: u32,
    max_height: u32,
    max_pixels: u64,
) -> Result<PixelSize> {
    if bytes.is_empty() {
        return Err(Error::invalid("image attachment is empty"));
    }
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    let size = PixelSize::new(width, height);
    if width == 0 || height == 0 {
        return Err(Error::invalid("image dimensions must be non-zero"));
    }
    if width > max_width || height > max_height || size.pixels() > max_pixels {
        return Err(Error::invalid(format!(
            "image dimensions {width}x{height} exceed configured limits"
        )));
    }
    if require_single_channel && decoder.color_type().channel_count() != 1 {
        return Err(Error::invalid("mask image must have exactly one channel"));
    }
    Ok(size)
}
