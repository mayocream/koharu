mod model;

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, RgbImage};
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

    pub fn inference(&self, image: &DynamicImage, mask: &GrayImage) -> Result<RgbImage> {
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
                let boxes = boxes_from_mask(mask);
                if boxes.is_empty() {
                    return Ok(image);
                }

                let image_tensor = Tensor::from_slice(image.as_raw())
                    .view([i64::from(image.height()), i64::from(image.width()), 3])
                    .to_device(self.device);
                let mask_tensor = Tensor::from_slice(mask.as_raw())
                    .view([i64::from(mask.height()), i64::from(mask.width())])
                    .to_device(self.device);
                let image_width = image.width();
                let image_height = image.height();
                let mut result = image;
                for bounding_box in boxes {
                    let [left, top, right, bottom] =
                        crop_box(image_width, image_height, bounding_box);
                    let crop_result = self.forward(
                        image_tensor
                            .narrow(0, i64::from(top), i64::from(bottom - top))
                            .narrow(1, i64::from(left), i64::from(right - left)),
                        mask_tensor
                            .narrow(0, i64::from(top), i64::from(bottom - top))
                            .narrow(1, i64::from(left), i64::from(right - left)),
                    )?;
                    let source_stride = crop_result.width() as usize * 3;
                    let target_stride = image_width as usize * 3;
                    for y in 0..crop_result.height() as usize {
                        let source_start = y * source_stride;
                        let target_start = (top as usize + y) * target_stride + left as usize * 3;
                        result.as_mut()[target_start..target_start + source_stride]
                            .copy_from_slice(
                                &crop_result.as_raw()[source_start..source_start + source_stride],
                            );
                    }
                }
                Ok(result)
            } else {
                let image_tensor = Tensor::from_slice(image.as_raw())
                    .view([i64::from(image.height()), i64::from(image.width()), 3])
                    .to_device(self.device);
                let mask_tensor = Tensor::from_slice(mask.as_raw())
                    .view([i64::from(mask.height()), i64::from(mask.width())])
                    .to_device(self.device);
                self.forward(image_tensor, mask_tensor)
            }
        })
    }

    fn forward(&self, image: Tensor, mask: Tensor) -> Result<RgbImage> {
        let height = image.size()[0] as u32;
        let width = image.size()[1] as u32;
        let image = (image
            .permute([2, 0, 1])
            .unsqueeze(0)
            .to_kind(Kind::Float)
            .contiguous())
            / 255.0;
        let mask = mask
            .gt(0.0)
            .unsqueeze(0)
            .unsqueeze(0)
            .to_kind(Kind::Float)
            .contiguous();
        let image = symmetric_pad(image, width, height);
        let mask = symmetric_pad(mask, width, height);
        let output = self.model.forward(&image, &mask);
        tensor_to_rgb_image(&output, width, height)
    }
}

fn boxes_from_mask(mask: &GrayImage) -> Vec<[u32; 4]> {
    let width = mask.width();
    let mut left = width;
    let mut top = mask.height();
    let mut right = 0;
    let mut bottom = 0;
    for y in 0..mask.height() {
        let row = &mask.as_raw()[y as usize * width as usize..(y + 1) as usize * width as usize];
        let Some(row_left) = row.iter().position(|value| *value > 127) else {
            continue;
        };
        let row_right = row
            .iter()
            .rposition(|value| *value > 127)
            .expect("masked row must have a right edge");
        left = left.min(row_left as u32);
        top = top.min(y);
        right = right.max(row_right as u32 + 1);
        bottom = y + 1;
    }
    if right <= left || bottom <= top {
        return Vec::new();
    }

    let cropped_width = right - left;
    let cropped_height = bottom - top;
    let padded_width = cropped_width + 2;
    let mut padded = GrayImage::new(padded_width, cropped_height + 2);
    for y in 0..cropped_height as usize {
        let source_start = (top as usize + y) * width as usize + left as usize;
        let target_start = (y + 1) * padded_width as usize + 1;
        padded.as_mut()[target_start..target_start + cropped_width as usize]
            .copy_from_slice(&mask.as_raw()[source_start..source_start + cropped_width as usize]);
    }

    find_contours_with_threshold::<u32>(&padded, 127)
        .into_iter()
        .filter(|contour| contour.border_type == BorderType::Outer && contour.parent.is_none())
        .filter_map(|contour| {
            let mut points = contour.points.into_iter();
            let first = points.next()?;
            let mut contour_left = first.x;
            let mut contour_top = first.y;
            let mut contour_right = first.x;
            let mut contour_bottom = first.y;
            for point in points {
                contour_left = contour_left.min(point.x);
                contour_top = contour_top.min(point.y);
                contour_right = contour_right.max(point.x);
                contour_bottom = contour_bottom.max(point.y);
            }
            Some([
                left + contour_left.saturating_sub(1),
                top + contour_top.saturating_sub(1),
                (left + contour_right).min(mask.width()),
                (top + contour_bottom).min(mask.height()),
            ])
        })
        .filter(|[left, top, right, bottom]| right > left && bottom > top)
        .collect()
}

fn crop_box(image_width: u32, image_height: u32, [left, top, right, bottom]: [u32; 4]) -> [u32; 4] {
    let image_width = i64::from(image_width);
    let image_height = i64::from(image_height);
    let crop_width = i64::from(right - left) + 256;
    let crop_height = i64::from(bottom - top) + 256;
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

    [
        left.clamp(0, image_width) as u32,
        top.clamp(0, image_height) as u32,
        right.clamp(0, image_width) as u32,
        bottom.clamp(0, image_height) as u32,
    ]
}

fn tensor_to_rgb_image(tensor: &Tensor, width: u32, height: u32) -> Result<RgbImage> {
    let tensor = tensor
        .narrow(2, 0, i64::from(height))
        .narrow(3, 0, i64::from(width))
        .squeeze_dim(0)
        .permute([1, 2, 0])
        .clamp(0.0, 1.0)
        * 255.0;
    let tensor = tensor
        .to_kind(Kind::Uint8)
        .contiguous()
        .to_device(Device::Cpu)
        .view([-1]);
    let rgb = Vec::<u8>::try_from(&tensor)?;

    RgbImage::from_raw(width, height, rgb).context("failed to convert LaMa tensor to RGB image")
}

fn symmetric_pad(tensor: Tensor, width: u32, height: u32) -> Tensor {
    let mut tensor = tensor;
    let padded_height = ceil_modulo(height, 8);
    if padded_height != height {
        let indices = (0..padded_height)
            .map(|index| i64::from(symmetric_index(index, height)))
            .collect::<Vec<_>>();
        tensor = tensor.index_select(2, &Tensor::from_slice(&indices).to_device(tensor.device()));
    }
    let padded_width = ceil_modulo(width, 8);
    if padded_width != width {
        let indices = (0..padded_width)
            .map(|index| i64::from(symmetric_index(index, width)))
            .collect::<Vec<_>>();
        tensor = tensor.index_select(3, &Tensor::from_slice(&indices).to_device(tensor.device()));
    }
    tensor
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
