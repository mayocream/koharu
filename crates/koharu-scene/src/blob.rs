use std::{io::Cursor, sync::Arc};

use image::{ImageDecoder, ImageReader};

use crate::{BlobId, Error, Result, Size};

#[derive(Clone)]
pub(crate) struct Attachment {
    pub id: BlobId,
    pub bytes: Arc<[u8]>,
    pub size: Size,
    pub single_channel: bool,
}

pub(crate) fn attach(bytes: impl Into<Arc<[u8]>>, mask: bool) -> Result<Attachment> {
    let bytes = bytes.into();
    if bytes.is_empty() {
        return Err(Error::invalid("image attachment is empty"));
    }
    let (size, single_channel) = {
        let reader = ImageReader::new(Cursor::new(bytes.as_ref())).with_guessed_format()?;
        let decoder = reader.into_decoder()?;
        let (width, height) = decoder.dimensions();
        (
            Size::new(width, height),
            decoder.color_type().channel_count() == 1,
        )
    };
    if !size.is_valid() {
        return Err(Error::invalid("image dimensions must be non-zero"));
    }
    if mask && !single_channel {
        return Err(Error::invalid("mask image must have exactly one channel"));
    }
    Ok(Attachment {
        id: BlobId::for_bytes(&bytes),
        bytes,
        size,
        single_channel,
    })
}
