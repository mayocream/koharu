use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::flux2_klein::{Flux2InpaintOptions, Flux2Klein};
use koharu_torch::Cuda;

#[tokio::main]
async fn main() -> Result<()> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("inpaint");
    let image = image::open(fixtures.join("image_4k.jpg"))?;
    let mask = image::open(fixtures.join("mask_4k.png"))?;
    let options = Flux2InpaintOptions::default();

    koharu_ml::init().await?;
    let model = Flux2Klein::load(koharu_ml::Device::cuda(0)).await?;

    Cuda::synchronize(0);
    let output = model.inpaint(&image, &mask, &options)?;
    assert_eq!(output.width(), image.width());
    assert_eq!(output.height(), image.height());
    black_box(output);
    Cuda::synchronize(0);

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    criterion.bench_function(
        "flux2_klein/inpaint/cuda0/3840x2074/max_pixels_1048576",
        |bencher| {
            bencher.iter(|| {
                Cuda::synchronize(0);
                let output = model
                    .inpaint(black_box(&image), black_box(&mask), black_box(&options))
                    .expect("FLUX.2 Klein inference failed");
                black_box(output);
                Cuda::synchronize(0);
            });
        },
    );
    criterion.final_summary();

    Ok(())
}
