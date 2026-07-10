use image::RgbaImage;

use crate::error::PsdExportError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelId {
    Red,
    Green,
    Blue,
    Alpha,
}

impl ChannelId {
    pub fn psd_id(self) -> i16 {
        match self {
            Self::Red => 0,
            Self::Green => 1,
            Self::Blue => 2,
            Self::Alpha => -1,
        }
    }

    fn rgba_offset(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Green => 1,
            Self::Blue => 2,
            Self::Alpha => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EncodedChannel {
    pub channel_id: i16,
    pub data: Vec<u8>,
}

pub fn encode_image_rle(
    image: &RgbaImage,
    channels: &[ChannelId],
    layer_name: &str,
) -> Result<Vec<EncodedChannel>, PsdExportError> {
    let mut encoded = Vec::with_capacity(channels.len());

    for channel in channels {
        let mut lengths = Vec::with_capacity(image.height() as usize);
        let mut data = Vec::new();
        let width = image.width() as usize;
        let offset = channel.rgba_offset();

        for y in 0..image.height() {
            let start = data.len();
            let mut row = Vec::with_capacity(width);
            for x in 0..image.width() {
                row.push(image.get_pixel(x, y).0[offset]);
            }
            encode_row(&row, &mut data);
            let row_length = data.len() - start;
            if row_length > u16::MAX as usize {
                return Err(PsdExportError::InvalidChannelEncoding {
                    layer: layer_name.to_string(),
                    row: y as usize,
                    length: row_length,
                });
            }
            lengths.push(row_length as u16);
        }

        let mut out = Vec::with_capacity(lengths.len() * 2 + data.len());
        for length in lengths {
            out.extend_from_slice(&length.to_be_bytes());
        }
        out.extend_from_slice(&data);
        encoded.push(EncodedChannel {
            channel_id: channel.psd_id(),
            data: out,
        });
    }

    Ok(encoded)
}

fn encode_row(row: &[u8], out: &mut Vec<u8>) {
    let mut i = 0usize;
    while i < row.len() {
        let run_len = repeated_run_len(row, i);
        if run_len >= 3 {
            let chunk = run_len.min(128);
            out.push((1i16 - chunk as i16) as u8);
            out.push(row[i]);
            i += chunk;
            continue;
        }

        let literal_start = i;
        let mut literal_len = 0usize;
        while i < row.len() && literal_len < 128 {
            let next_run = repeated_run_len(row, i);
            if next_run >= 3 {
                break;
            }
            i += 1;
            literal_len += 1;
        }

        out.push((literal_len - 1) as u8);
        out.extend_from_slice(&row[literal_start..literal_start + literal_len]);
    }
}

fn repeated_run_len(row: &[u8], start: usize) -> usize {
    let value = row[start];
    let mut len = 1usize;
    while start + len < row.len() && row[start + len] == value && len < 128 {
        len += 1;
    }
    len
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::{ChannelId, encode_image_rle};

    #[test]
    fn packbits_repeated_rows_encode_with_short_repeat_packets() {
        let mut image = RgbaImage::new(4, 1);
        for x in 0..4 {
            image.put_pixel(x, 0, Rgba([10, 0, 0, 255]));
        }

        let channels = encode_image_rle(&image, &[ChannelId::Red], "row").expect("encode");
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].data, vec![0, 2, 253, 10]);
    }

    #[test]
    fn packbits_literal_rows_keep_original_order() {
        let mut image = RgbaImage::new(4, 1);
        let values = [1u8, 2, 3, 4];
        for (x, value) in values.into_iter().enumerate() {
            image.put_pixel(x as u32, 0, Rgba([value, 0, 0, 255]));
        }

        let channels = encode_image_rle(&image, &[ChannelId::Red], "row").expect("encode");
        assert_eq!(channels[0].data, vec![0, 5, 3, 1, 2, 3, 4]);
    }
}
