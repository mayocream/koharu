use candle_transformers::object_detection::{Bbox, non_maximum_suppression};
use clap::Parser;
use image::GenericImageView;
use ndarray::Array;
use ort::{
    inputs,
    session::{Session, builder::GraphOptimizationLevel},
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    image: String,

    #[arg(long, default_value = "comictextdetector.pt.onnx")]
    model: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let model = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(4)?
        .commit_from_file(args.model)?;

    println!("loaded model: {model:?}");

    let orig_image =
        image::open(&args.image).map_err(|e| anyhow::anyhow!("Failed to open image: {e}"))?;
    let (orig_width, orig_height) = orig_image.dimensions();
    let w_ratio = orig_width as f32 / 1024 as f32;
    let h_ratio = orig_height as f32 / 1024 as f32;

    let image = orig_image.resize_exact(1024, 1024, image::imageops::FilterType::CatmullRom);

    let mut input = Array::zeros((1, 3, 1024, 1024));
    for pixel in image.pixels() {
        let x = pixel.0 as _;
        let y = pixel.1 as _;
        let [r, g, b, _] = pixel.2.0;
        input[[0, 0, y, x]] = (r as f32) / 255.;
        input[[0, 1, y, x]] = (g as f32) / 255.;
        input[[0, 2, y, x]] = (b as f32) / 255.;
    }

    let inputs = inputs!["images" => input.view()]?;
    let outputs = model.run(inputs)?;

    let blk = outputs["blk"].try_extract_tensor::<f32>()?;
    let blk = blk.view();

    let mut boxes: Vec<Vec<Bbox<_>>> = (0..2).map(|_| vec![]).collect();
    for i in 0..blk.shape()[1] {
        let confidence = blk[[0, i, 4]];
        if confidence < 0.25 {
            continue;
        }

        let mut class_index = 0;
        if blk[[0, i, 5]] > blk[[0, i, 6]] {
            class_index = 1;
        }

        let center_x = blk[[0, i, 0]] * w_ratio;
        let center_y = blk[[0, i, 1]] * h_ratio;
        let width = blk[[0, i, 2]] * w_ratio;
        let height = blk[[0, i, 3]] * h_ratio;

        boxes[class_index].push(Bbox {
            confidence,
            xmin: center_x - width / 2.,
            ymin: center_y - height / 2.,
            xmax: center_x + width / 2.,
            ymax: center_y + height / 2.,
            data: (),
        });
    }

    non_maximum_suppression(&mut boxes, 0.45);

    for (i, box_list) in boxes.iter().enumerate() {
        for bbox in box_list {
            let x0 = bbox.xmin as u32;
            let y0 = bbox.ymin as u32;
            let x1 = bbox.xmax as u32;
            let y1 = bbox.ymax as u32;

            println!(
                "Class: {i}, Confidence: {:.2}, Box: ({x0}, {y0}), ({x1}, {y1})",
                bbox.confidence
            );
        }
    }

    Ok(())
}
