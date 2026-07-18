use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::aot_inpainting::AotInpainting;
use koharu_torch::Cuda;

#[tokio::main]
async fn main() -> Result<()> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("inpaint");
    let image = image::open(fixtures.join("image_4k.jpg"))?;
    let mask = image::open(fixtures.join("mask_4k.png"))?.to_luma8();

    koharu_ml::init_torch().await?;
    let model = AotInpainting::load(koharu_ml::Device::cuda(0)).await?;
    model.inference(&image, &mask)?;
    Cuda::synchronize(0);

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    criterion.bench_function("aot_inpainting/inference/cuda0/3840x2074", |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let output = model
                .inference(black_box(&image), black_box(&mask))
                .expect("AOT inpainting inference failed");
            Cuda::synchronize(0);
            black_box(output);
        });
    });
    criterion.final_summary();
    Ok(())
}
