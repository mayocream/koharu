mod model;

use anyhow::{Context, Result};
use image::{
    DynamicImage, GrayImage, RgbImage,
    imageops::{crop_imm, replace},
};
use imageproc::contours::{BorderType, find_contours_with_threshold};
use koharu_runtime::package::huggingface;
use koharu_torch::{Device, Kind, Tensor};

use self::model::Model;

koharu_runtime::huggingface! {
    WEIGHTS => "mayocream/lama-manga" => "lama-manga.safetensors",
}

#[derive(Debug)]
pub struct LaMa {
    device: Device,
    model: Model,
}

impl LaMa {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve LaMa weights")?;
        let mut model = Model::new(device);
        model
            .load_safetensors(&weights_path)
            .context("failed to load LaMa safetensors")?;
        Ok(Self { device, model })
    }

    pub fn inpaint(&self, image: &DynamicImage, mask: &GrayImage) -> Result<RgbImage> {
        let image = image.to_rgb8();
        anyhow::ensure!(
            image.dimensions() == mask.dimensions(),
            "image and mask dimensions differ: image={:?}, mask={:?}",
            image.dimensions(),
            mask.dimensions()
        );
        anyhow::ensure!(
            image.width() > 0 && image.height() > 0,
            "image dimensions must be non-zero"
        );

        koharu_torch::no_grad(|| {
            if image.width().max(image.height()) > 800 {
                let mut result = image.clone();
                for bounding_box in boxes_from_mask(mask) {
                    let (crop_image, crop_mask, [left, top, _, _]) =
                        crop_box(&image, mask, bounding_box, 128);
                    let crop_result = self.pad_forward(&crop_image, &crop_mask)?;
                    replace(&mut result, &crop_result, i64::from(left), i64::from(top));
                }
                Ok(result)
            } else {
                self.pad_forward(&image, mask)
            }
        })
    }

    fn pad_forward(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        let width = image.width();
        let height = image.height();
        let padded_width = ceil_modulo(width, 8);
        let padded_height = ceil_modulo(height, 8);

        let padded_image = RgbImage::from_fn(padded_width, padded_height, |x, y| {
            *image.get_pixel(symmetric_index(x, width), symmetric_index(y, height))
        });
        let padded_mask = GrayImage::from_fn(padded_width, padded_height, |x, y| {
            *mask.get_pixel(symmetric_index(x, width), symmetric_index(y, height))
        });

        let result = self.forward(&padded_image, &padded_mask)?;
        Ok(crop_imm(&result, 0, 0, width, height).to_image())
    }

    fn forward(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        let width = i64::from(image.width());
        let height = i64::from(image.height());
        let image_tensor = (Tensor::from_slice(image.as_raw())
            .view([1, height, width, 3])
            .permute([0, 3, 1, 2])
            .to_device(self.device)
            .to_kind(Kind::Float))
            / 255.0;
        let mask_tensor = Tensor::from_slice(mask.as_raw())
            .view([1, 1, height, width])
            .to_device(self.device)
            .gt(0.0)
            .to_kind(Kind::Float);

        let output = self.model.forward(&image_tensor, &mask_tensor);
        tensor_to_rgb_image(&output, image.width(), image.height())
    }
}

fn boxes_from_mask(mask: &GrayImage) -> Vec<[u32; 4]> {
    let Some(padded_width) = mask.width().checked_add(2) else {
        return Vec::new();
    };
    let Some(padded_height) = mask.height().checked_add(2) else {
        return Vec::new();
    };

    let mut padded = GrayImage::new(padded_width, padded_height);
    replace(&mut padded, mask, 1, 1);

    find_contours_with_threshold::<u32>(&padded, 127)
        .into_iter()
        .filter(|contour| contour.border_type == BorderType::Outer && contour.parent.is_none())
        .filter_map(|contour| {
            let mut points = contour.points.into_iter();
            let first = points.next()?;
            let mut left = first.x;
            let mut top = first.y;
            let mut right = first.x;
            let mut bottom = first.y;

            for point in points {
                left = left.min(point.x);
                top = top.min(point.y);
                right = right.max(point.x);
                bottom = bottom.max(point.y);
            }

            Some([
                left.saturating_sub(1),
                top.saturating_sub(1),
                right.min(mask.width()),
                bottom.min(mask.height()),
            ])
        })
        .filter(|[left, top, right, bottom]| right > left && bottom > top)
        .collect()
}

fn crop_box(
    image: &RgbImage,
    mask: &GrayImage,
    [left, top, right, bottom]: [u32; 4],
    margin: u32,
) -> (RgbImage, GrayImage, [u32; 4]) {
    let image_width = i64::from(image.width());
    let image_height = i64::from(image.height());
    let crop_width = i64::from(right - left) + i64::from(margin) * 2;
    let crop_height = i64::from(bottom - top) + i64::from(margin) * 2;
    let center_x = (i64::from(left) + i64::from(right)) / 2;
    let center_y = (i64::from(top) + i64::from(bottom)) / 2;

    let raw_left = center_x - crop_width / 2;
    let raw_right = center_x + crop_width / 2;
    let raw_top = center_y - crop_height / 2;
    let raw_bottom = center_y + crop_height / 2;

    let mut left = raw_left.max(0);
    let mut right = raw_right.min(image_width);
    let mut top = raw_top.max(0);
    let mut bottom = raw_bottom.min(image_height);

    if raw_left < 0 {
        right += -raw_left;
    }
    if raw_right > image_width {
        left -= raw_right - image_width;
    }
    if raw_top < 0 {
        bottom += -raw_top;
    }
    if raw_bottom > image_height {
        top -= raw_bottom - image_height;
    }

    let crop_box = [
        left.clamp(0, image_width) as u32,
        top.clamp(0, image_height) as u32,
        right.clamp(0, image_width) as u32,
        bottom.clamp(0, image_height) as u32,
    ];
    let [left, top, right, bottom] = crop_box;
    let crop_image = crop_imm(image, left, top, right - left, bottom - top).to_image();
    let crop_mask = crop_imm(mask, left, top, right - left, bottom - top).to_image();
    (crop_image, crop_mask, crop_box)
}

fn tensor_to_rgb_image(tensor: &Tensor, width: u32, height: u32) -> Result<RgbImage> {
    let tensor = tensor
        .squeeze_dim(0)
        .slice(1, 0, i64::from(height), 1)
        .slice(2, 0, i64::from(width), 1)
        .clamp(0.0, 1.0)
        * 255.0;
    let tensor = tensor
        .to_device(Device::Cpu)
        .to_kind(Kind::Uint8)
        .contiguous();
    let plane = width as usize * height as usize;
    let mut chw = vec![0u8; plane * 3];
    tensor.copy_data(&mut chw, plane * 3);

    let mut rgb = vec![0u8; plane * 3];
    for index in 0..plane {
        rgb[index * 3] = chw[index];
        rgb[index * 3 + 1] = chw[plane + index];
        rgb[index * 3 + 2] = chw[plane * 2 + index];
    }

    RgbImage::from_raw(width, height, rgb).context("failed to convert LaMa tensor to RGB image")
}

fn ceil_modulo(value: u32, modulo: u32) -> u32 {
    if value % modulo == 0 {
        value
    } else {
        (value / modulo + 1) * modulo
    }
}

fn symmetric_index(index: u32, len: u32) -> u32 {
    let index = index % (len * 2);
    if index < len {
        index
    } else {
        len * 2 - index - 1
    }
}
