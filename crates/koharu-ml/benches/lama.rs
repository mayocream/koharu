use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::lama::LaMa;
use koharu_torch::Cuda;

#[tokio::main]
async fn main() -> Result<()> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("inpaint");
    let image_path = fixtures.join("image_4k.jpg");
    let mask_path = fixtures.join("mask_4k.png");

    let image = image::open(&image_path)?;
    let mask = image::open(&mask_path)?.to_luma8();

    koharu_ml::init().await?;
    let model = LaMa::load(koharu_ml::Device::cuda(0)).await?;

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    criterion.bench_function("lama/inpaint/3840x2074", |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let output = model
                .inpaint(black_box(&image), black_box(&mask))
                .expect("LaMa inpainting failed");
            black_box(output);
            Cuda::synchronize(0);
        });
    });
    criterion.final_summary();

    Ok(())
}
