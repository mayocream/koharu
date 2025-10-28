use hf_hub::api::sync::Api;
use image::{DynamicImage, GenericImageView};
use ort::{inputs, session::Session, value::TensorRef};
use std::cmp::{max, min};

#[derive(Debug)]
pub struct Lama {
    model: Session,
}

fn resize_with_padding(
    img: &DynamicImage,
    target_size: u32,
    filter: image::imageops::FilterType,
) -> (DynamicImage, (u32, u32, u32, u32)) {
    let (orig_width, orig_height) = img.dimensions();

    // Calculate new dimensions while preserving aspect ratio
    let (new_width, new_height) = if orig_width > orig_height {
        // Width is the longer dimension
        let height = (target_size as f32 * orig_height as f32 / orig_width as f32).round() as u32;
        (target_size, height)
    } else {
        // Height is the longer dimension
        let width = (target_size as f32 * orig_width as f32 / orig_height as f32).round() as u32;
        (width, target_size)
    };

    // Resize the image
    let resized = img.resize(new_width, new_height, filter);

    // Calculate padding needed
    let pad_right = target_size.saturating_sub(new_width);
    let pad_bottom = target_size.saturating_sub(new_height);

    // Create a new image with padding
    let mut padded = DynamicImage::new_rgba8(target_size, target_size);

    // Copy the resized image to the padded image
    image::imageops::replace(&mut padded, &resized, 0, 0);

    // Add reflection padding
    if pad_right > 0 || pad_bottom > 0 {
        let mut buffer = padded.to_rgba8();

        // Add right padding (reflect)
        if pad_right > 0 {
            for y in 0..new_height {
                for x in 0..pad_right {
                    let source_x = new_width.saturating_sub(1 + x % new_width);
                    let pixel = buffer.get_pixel(source_x, y);
                    buffer.put_pixel(new_width + x, y, *pixel);
                }
            }
        }

        // Add bottom padding (reflect)
        if pad_bottom > 0 {
            for y in 0..pad_bottom {
                for x in 0..target_size {
                    let source_y = new_height.saturating_sub(1 + y % new_height);
                    let pixel = buffer.get_pixel(x, source_y);
                    buffer.put_pixel(x, new_height + y, *pixel);
                }
            }
        }

        padded = DynamicImage::ImageRgba8(buffer);
    }

    // Return padded image and padding info for reverting
    (padded, (new_width, new_height, pad_right, pad_bottom))
}

fn revert_resize_padding(
    padded: &DynamicImage,
    original_dimensions: (u32, u32),
    resize_info: (u32, u32, u32, u32),
    filter: image::imageops::FilterType,
) -> DynamicImage {
    let (orig_width, orig_height) = original_dimensions;
    let (resized_width, resized_height, _, _) = resize_info;

    // First crop to remove padding
    let cropped = padded.crop_imm(0, 0, resized_width, resized_height);

    // Then resize back to original dimensions
    cropped.resize(orig_width, orig_height, filter)
}

impl Lama {
    pub fn new() -> anyhow::Result<Self> {
        let api = Api::new()?;
        let repo = api.model("mayocream/lama-manga-onnx".to_string());
        let model_path = repo.get("lama-manga.onnx")?;

        let model = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .commit_from_file(model_path)?;

        Ok(Lama { model })
    }

    pub fn inference(
        &mut self,
        image: &DynamicImage,
        mask: &DynamicImage,
    ) -> anyhow::Result<DynamicImage> {
        let (orig_width, orig_height) = image.dimensions();
        let (image, resize_info) =
            resize_with_padding(image, 512, image::imageops::FilterType::CatmullRom);
        let (mask, _) = resize_with_padding(mask, 512, image::imageops::FilterType::CatmullRom);

        let mut image_data = ndarray::Array::zeros((1, 3, 512, 512));
        for pixel in image.pixels() {
            let (x, y, pixel) = pixel;
            let x = x as usize;
            let y = y as usize;

            // Channel order: RGB
            image_data[[0, 0, y, x]] = (pixel[0] as f32) / 255.0;
            image_data[[0, 1, y, x]] = (pixel[1] as f32) / 255.0;
            image_data[[0, 2, y, x]] = (pixel[2] as f32) / 255.0;
        }

        // Fixed mask interpretation - black pixels (0) are now the area TO inpaint (value 1.0)
        let mut mask_data = ndarray::Array::zeros((1, 1, 512, 512));

        for pixel in mask.pixels() {
            let (x, y, pixel) = pixel;
            let x = x as usize;
            let y = y as usize;

            // For LaMa, mask value of 1 indicates area to be inpainted
            mask_data[[0, 0, y, x]] = if pixel[0] > 0 { 1.0f32 } else { 0.0f32 };
        }

        let inputs = inputs![
            "image" => TensorRef::from_array_view(image_data.view())?,
            "mask" => TensorRef::from_array_view(mask_data.view())?,
        ];
        let outputs = self.model.run(inputs)?;
        let output = outputs["output"].try_extract_array::<f32>()?;
        let output = output.view();

        let mut output_image = image::RgbImage::new(512, 512);
        for y in 0..512 {
            for x in 0..512 {
                let r = (output[[0, 0, y, x]] * 255.0).clamp(0.0, 255.0).round() as u8;
                let g = (output[[0, 1, y, x]] * 255.0).clamp(0.0, 255.0).round() as u8;
                let b = (output[[0, 2, y, x]] * 255.0).clamp(0.0, 255.0).round() as u8;
                output_image.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
            }
        }

        let mut output_image = DynamicImage::ImageRgb8(output_image);
        output_image = revert_resize_padding(
            &output_image,
            (orig_width, orig_height),
            resize_info,
            image::imageops::FilterType::CatmullRom,
        );

        Ok(output_image)
    }

    /// Inpaint an image using tiled inference with multiresolution-style blending.
    ///
    /// - `tile_size`: size of model input tiles (typically 512 for LaMa).
    /// - `overlap`: pixels of overlap between neighboring tiles (e.g., 128).
    ///
    /// The final result preserves original pixels outside the mask and blends
    /// inpainted tiles smoothly inside the masked regions.
    pub fn inference_tiled(
        &mut self,
        image: &DynamicImage,
        mask: &DynamicImage,
        tile_size: u32,
        overlap: u32,
    ) -> anyhow::Result<DynamicImage> {
        let (w, h) = image.dimensions();
        let tile = max(32, tile_size); // guard against tiny tiles
        let ovl = min(overlap, tile.saturating_sub(1));
        let stride = tile.saturating_sub(ovl);

        // Accumulators for weighted blending
        let mut acc_r = vec![0f32; (w * h) as usize];
        let mut acc_g = vec![0f32; (w * h) as usize];
        let mut acc_b = vec![0f32; (w * h) as usize];
        let mut acc_w = vec![0f32; (w * h) as usize];

        // Convert inputs to RGB/Gray for faster pixel access
        let img_rgb = image.to_rgb8();
        // Interpret mask: >0 means inpaint region
        let mask_luma = mask.to_luma8();

        // Iterate tiles
        let mut y0 = 0u32;
        while y0 < h {
            let mut x0 = 0u32;
            while x0 < w {
                let x1 = min(x0 + tile, w);
                let y1 = min(y0 + tile, h);
                let eff_w = x1 - x0;
                let eff_h = y1 - y0;

                // Skip tiles with no masked pixels in effective region
                let mut any_masked = false;
                'mask_check: for yy in 0..eff_h {
                    for xx in 0..eff_w {
                        if mask_luma.get_pixel(x0 + xx, y0 + yy)[0] > 0 {
                            any_masked = true;
                            break 'mask_check;
                        }
                    }
                }
                if !any_masked {
                    x0 = x0.saturating_add(stride).min(w);
                    continue;
                }

                // Build 512x512 (or tile x tile) reflected-padded tiles for image & mask
                let (tile_img, tile_mask) =
                    extract_reflect_padded_tile(&img_rgb, &mask_luma, x0, y0, eff_w, eff_h, tile);

                // Run model on tile
                let tile_out = self.infer_tile_512(&tile_img, &tile_mask)?; // RGB tile x tile

                // Extract effective region (top-left eff_w x eff_h)
                let mut tile_out_crop = image::RgbImage::new(eff_w, eff_h);
                for yy in 0..eff_h {
                    for xx in 0..eff_w {
                        tile_out_crop.put_pixel(xx, yy, *tile_out.get_pixel(xx, yy));
                    }
                }

                // Compute blending weights for this tile (raised-cosine over overlap)
                let weights = make_tile_weights(eff_w, eff_h, ovl);

                // Multiply weights by mask>0 to ensure we only blend inpaint areas
                // (softening via raised-cosine already smooths across tiles)
                // Accumulate
                for yy in 0..eff_h {
                    for xx in 0..eff_w {
                        let global_x = x0 + xx;
                        let global_y = y0 + yy;
                        let idx = (global_y * w + global_x) as usize;

                        let m = if mask_luma.get_pixel(global_x, global_y)[0] > 0 {
                            1.0f32
                        } else {
                            0.0f32
                        };

                        if m == 0.0 {
                            continue;
                        }

                        let wgt = weights[(yy * eff_w + xx) as usize] * m;
                        if wgt <= 0.0 {
                            continue;
                        }

                        let p = tile_out_crop.get_pixel(xx, yy);
                        acc_r[idx] += p[0] as f32 * wgt;
                        acc_g[idx] += p[1] as f32 * wgt;
                        acc_b[idx] += p[2] as f32 * wgt;
                        acc_w[idx] += wgt;
                    }
                }

                x0 = x0.saturating_add(stride).min(w);
            }
            y0 = y0.saturating_add(stride).min(h);
        }

        // Compose final image: use original outside mask, blended result inside
        let mut out = img_rgb.clone();
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                if mask_luma.get_pixel(x, y)[0] == 0 {
                    continue; // keep original
                }
                let wsum = acc_w[idx];
                if wsum > 0.0 {
                    let r = (acc_r[idx] / wsum).clamp(0.0, 255.0) as u8;
                    let g = (acc_g[idx] / wsum).clamp(0.0, 255.0) as u8;
                    let b = (acc_b[idx] / wsum).clamp(0.0, 255.0) as u8;
                    out.put_pixel(x, y, image::Rgb([r, g, b]));
                }
            }
        }

        Ok(DynamicImage::ImageRgb8(out))
    }
}

/// Extract a tile of size (tile x tile) using reflection padding as needed from (x0..x0+eff_w, y0..y0+eff_h).
fn extract_reflect_padded_tile(
    img: &image::RgbImage,
    mask: &image::GrayImage,
    x0: u32,
    y0: u32,
    eff_w: u32,
    eff_h: u32,
    tile: u32,
) -> (image::RgbImage, image::GrayImage) {
    let mut out_img = image::RgbImage::new(tile, tile);
    let mut out_msk = image::GrayImage::new(tile, tile);

    // copy valid region to top-left
    for yy in 0..eff_h {
        for xx in 0..eff_w {
            let src_x = x0 + xx;
            let src_y = y0 + yy;
            out_img.put_pixel(xx, yy, *img.get_pixel(src_x, src_y));
            out_msk.put_pixel(xx, yy, *mask.get_pixel(src_x, src_y));
        }
    }

    // reflect-pad on right
    for yy in 0..eff_h {
        for xx in eff_w..tile {
            let rx = if eff_w == 0 {
                0
            } else {
                eff_w - 1 - ((xx - eff_w) % eff_w)
            };
            let p = *out_img.get_pixel(rx, yy);
            let m = *out_msk.get_pixel(rx, yy);
            out_img.put_pixel(xx, yy, p);
            out_msk.put_pixel(xx, yy, m);
        }
    }
    // reflect-pad on bottom
    for yy in eff_h..tile {
        let sy = if eff_h == 0 {
            0
        } else {
            eff_h - 1 - ((yy - eff_h) % eff_h)
        };
        for xx in 0..tile {
            let p = *out_img.get_pixel(xx, sy);
            let m = *out_msk.get_pixel(xx, sy);
            out_img.put_pixel(xx, yy, p);
            out_msk.put_pixel(xx, yy, m);
        }
    }

    (out_img, out_msk)
}

/// Raised-cosine feathering weights within a tile effective region.
/// Weight = 1 in the center, smoothly drops to 0 across an overlap/2 band near borders.
fn make_tile_weights(w: u32, h: u32, overlap: u32) -> Vec<f32> {
    use std::f32::consts::PI;
    let mut weights = vec![1.0f32; (w * h) as usize];
    let half = (overlap as f32) / 2.0;
    if overlap == 0 {
        return weights;
    }

    for y in 0..h {
        for x in 0..w {
            let dx = min(x, w - 1 - x) as f32;
            let dy = min(y, h - 1 - y) as f32;
            let d = dx.min(dy);
            let wxy = if d >= half || half <= 1e-3 {
                1.0
            } else {
                // raised cosine from 0 at border to 1 at distance >= half
                let t = (d / half).clamp(0.0, 1.0);
                0.5 * (1.0 - (PI * (1.0 - t)).cos())
            };
            weights[(y * w + x) as usize] = wxy;
        }
    }
    weights
}

impl Lama {
    /// Run a single-tile inference assuming a square tile of size 512 (or arbitrary tile size equal on both dims)
    /// No resizing/aspect logic, inputs must match model size.
    fn infer_tile_512(
        &mut self,
        tile_img: &image::RgbImage,
        tile_mask: &image::GrayImage,
    ) -> anyhow::Result<image::RgbImage> {
        let (tw, th) = tile_img.dimensions();
        // Model is trained for 512x512; allow other sizes if the model supports dynamic shapes.
        let w = tw as usize;
        let h = th as usize;

        let mut image_data = ndarray::Array::zeros((1, 3, h, w));
        for y in 0..th {
            for x in 0..tw {
                let p = tile_img.get_pixel(x, y);
                let fx = x as usize;
                let fy = y as usize;
                image_data[[0, 0, fy, fx]] = (p[0] as f32) / 255.0;
                image_data[[0, 1, fy, fx]] = (p[1] as f32) / 255.0;
                image_data[[0, 2, fy, fx]] = (p[2] as f32) / 255.0;
            }
        }

        let mut mask_data = ndarray::Array::zeros((1, 1, h, w));
        for y in 0..th {
            for x in 0..tw {
                let m = tile_mask.get_pixel(x, y)[0];
                let fx = x as usize;
                let fy = y as usize;
                mask_data[[0, 0, fy, fx]] = if m > 0 { 1.0f32 } else { 0.0f32 };
            }
        }

        let inputs = inputs![
            "image" => TensorRef::from_array_view(image_data.view())?,
            "mask" => TensorRef::from_array_view(mask_data.view())?,
        ];
        let outputs = self.model.run(inputs)?;
        let output = outputs["output"].try_extract_array::<f32>()?;
        let output = output.view();

        let mut out_img = image::RgbImage::new(tw, th);
        for y in 0..th {
            for x in 0..tw {
                let r = (output[[0, 0, y as usize, x as usize]] * 255.0).clamp(0.0, 255.0) as u8;
                let g = (output[[0, 1, y as usize, x as usize]] * 255.0).clamp(0.0, 255.0) as u8;
                let b = (output[[0, 2, y as usize, x as usize]] * 255.0).clamp(0.0, 255.0) as u8;
                out_img.put_pixel(x, y, image::Rgb([r, g, b]));
            }
        }
        Ok(out_img)
    }
}
