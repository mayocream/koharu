use image::{DynamicImage, GrayImage, Luma, RgbImage, Rgba, RgbaImage};
use imageproc::{distance_transform::Norm, morphology::dilate};

const ALPHA_RING_RADIUS: u8 = 7;

pub fn binarize_mask(mask: &DynamicImage) -> GrayImage {
    let mut binary = mask.to_luma8();
    for pixel in binary.pixels_mut() {
        pixel.0[0] = if pixel.0[0] > 127 { 255 } else { 0 };
    }
    binary
}

pub fn extract_alpha(image: &RgbaImage) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut alpha = GrayImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels() {
        alpha.put_pixel(x, y, Luma([pixel.0[3]]));
    }
    alpha
}

pub fn restore_alpha_channel(
    image: &RgbImage,
    original_alpha: &GrayImage,
    mask: &GrayImage,
) -> RgbaImage {
    let mut result = RgbaImage::new(image.width(), image.height());
    let mut alpha = original_alpha.clone();

    let mask_dilated = dilate(mask, Norm::LInf, ALPHA_RING_RADIUS);
    let mut surrounding_alpha = Vec::new();
    for (x, y, pixel) in mask_dilated.enumerate_pixels() {
        if pixel.0[0] > 0 && mask.get_pixel(x, y).0[0] == 0 {
            surrounding_alpha.push(original_alpha.get_pixel(x, y).0[0]);
        }
    }

    if let Some(median_alpha) = median_u8(&surrounding_alpha)
        && median_alpha < 128
    {
        for (x, y, pixel) in mask.enumerate_pixels() {
            if pixel.0[0] > 0 {
                alpha.put_pixel(x, y, Luma([median_alpha]));
            }
        }
    }

    for (x, y, pixel) in image.enumerate_pixels() {
        result.put_pixel(
            x,
            y,
            Rgba([
                pixel.0[0],
                pixel.0[1],
                pixel.0[2],
                alpha.get_pixel(x, y).0[0],
            ]),
        );
    }

    result
}

fn median_u8(values: &[u8]) -> Option<u8> {
    if values.is_empty() {
        return None;
    }

    let mut values = values.to_vec();
    values.sort_unstable();
    let mid = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (u16::from(values[mid - 1]) + u16::from(values[mid])) / 2
    } else {
        u16::from(values[mid])
    };
    Some(median as u8)
}
