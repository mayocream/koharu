use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::{comic_text_bubble_detector::ComicTextBubbleDetector, torch::Cuda};

#[tokio::main]
async fn main() -> Result<()> {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("object_detection")
        .join("1.jpg");

    koharu_ml::init().await?;

    let image = image::open(&input)?;
    let model = ComicTextBubbleDetector::load(koharu_ml::Device::cuda(0)).await?;

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    criterion.bench_function("comic_text_bubble_detector/inference/770x1080", |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let detection = model
                .inference(black_box(&image), black_box(0.3))
                .expect("Comic Text Bubble Detector inference failed");
            Cuda::synchronize(0);
            black_box(detection);
        });
    });
    criterion.final_summary();

    Ok(())
}
