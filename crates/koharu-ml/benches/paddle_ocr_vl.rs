use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::{
    paddle_ocr_vl::{PaddleOCRVL, PaddleOCRVLTask},
    torch::Cuda,
};

#[tokio::main]
async fn main() -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::WARN.into())
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("ocr")
        .join("title.png");

    koharu_ml::init_torch().await?;
    let image = image::open(&input)?;
    let model = PaddleOCRVL::load(koharu_ml::Device::cuda(0)).await?;

    Cuda::synchronize(0);
    let warmup = model.inference(&image, PaddleOCRVLTask::Ocr)?;
    Cuda::synchronize(0);
    assert_eq!(warmup.text, "対策委員会です！");
    black_box(warmup);

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();
    let benchmark = format!(
        "paddle_ocr_vl/inference/ocr/cuda0/{}x{}",
        image.width(),
        image.height()
    );

    criterion.bench_function(&benchmark, |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let result = model
                .inference(black_box(&image), black_box(PaddleOCRVLTask::Ocr))
                .expect("PaddleOCR-VL inference failed");
            Cuda::synchronize(0);
            black_box(result);
        });
    });
    criterion.final_summary();

    Ok(())
}
