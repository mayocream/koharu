use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::{pp_ocr_v6::rec::PPOCRV6MediumRec, torch::Cuda};

#[tokio::main]
async fn main() -> Result<()> {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("ocr")
        .join("title.png");

    koharu_ml::init_torch().await?;

    let image = image::open(&input)?;
    let model = PPOCRV6MediumRec::load(koharu_ml::Device::cuda(0)).await?;

    Cuda::synchronize(0);
    black_box(model.inference(&image)?);
    Cuda::synchronize(0);

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    let benchmark = format!(
        "pp_ocr_v6/rec/inference/cuda0/{}x{}",
        image.width(),
        image.height()
    );
    criterion.bench_function(&benchmark, |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let recognition = model
                .inference(black_box(&image))
                .expect("PP-OCRv6 recognizer inference failed");
            Cuda::synchronize(0);
            black_box(recognition);
        });
    });
    criterion.final_summary();

    Ok(())
}
