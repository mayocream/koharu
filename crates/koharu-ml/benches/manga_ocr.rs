use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::{manga_ocr::MangaOcr, torch::Cuda};

#[tokio::main]
async fn main() -> Result<()> {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("ocr")
        .join("title.png");

    koharu_ml::init().await?;
    let image = image::open(&input)?;
    let model = MangaOcr::load(koharu_ml::Device::cuda(0)).await?;

    Cuda::synchronize(0);
    let warmup = model.inference(&image)?;
    Cuda::synchronize(0);
    assert_eq!(warmup, "対策委員会です！");
    black_box(warmup);

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();
    let benchmark = format!(
        "manga_ocr/inference/cuda0/{}x{}",
        image.width(),
        image.height()
    );

    criterion.bench_function(&benchmark, |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let text = model
                .inference(black_box(&image))
                .expect("Manga OCR inference failed");
            Cuda::synchronize(0);
            black_box(text);
        });
    });
    criterion.final_summary();

    Ok(())
}
