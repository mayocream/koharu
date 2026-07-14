use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::{manga_text_segmentation::MangaTextSegmentation, torch::Cuda};

#[tokio::main]
async fn main() -> Result<()> {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("object_detection")
        .join("1.jpg");

    koharu_ml::init().await?;

    let image = image::open(input)?;
    let model = MangaTextSegmentation::load(koharu_ml::Device::cuda(0)).await?;

    model.inference(&image)?;
    Cuda::synchronize(0);

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    criterion.bench_function("manga_text_segmentation/inference/770x1080", |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let output = model
                .inference(black_box(&image))
                .expect("Manga Text Segmentation inference failed");
            Cuda::synchronize(0);
            black_box(output);
        });
    });
    criterion.final_summary();

    Ok(())
}
