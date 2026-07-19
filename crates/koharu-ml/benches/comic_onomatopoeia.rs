use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::{
    comic_onomatopoeia::{ComicOnomatopoeiaDetector, ComicOnomatopoeiaRecognizer},
    torch::Cuda,
};

#[tokio::main]
async fn main() -> Result<()> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/fixtures");
    koharu_ml::init_torch().await?;
    let detector_image = image::open(fixtures.join("object_detection/1.jpg"))?;
    let recognizer_image = image::open(fixtures.join("ocr/title.png"))?;
    let detector = ComicOnomatopoeiaDetector::load(koharu_ml::Device::cuda(0)).await?;
    let recognizer = ComicOnomatopoeiaRecognizer::load(koharu_ml::Device::cuda(0)).await?;

    // Resolution, decoding, model construction, and one real warm-up stay outside timing.
    Cuda::synchronize(0);
    black_box(detector.inference(black_box(&detector_image))?);
    black_box(recognizer.inference(black_box(&recognizer_image))?);
    Cuda::synchronize(0);

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();
    criterion.bench_function("comic_onomatopoeia/mtsv3/inference", |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let detections = detector
                .inference(black_box(&detector_image))
                .expect("COO MTSv3 inference failed");
            Cuda::synchronize(0);
            black_box(detections);
        });
    });
    criterion.bench_function("comic_onomatopoeia/trba_hardroi_2d/inference", |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let recognition = recognizer
                .inference(black_box(&recognizer_image))
                .expect("COO TRBA inference failed");
            Cuda::synchronize(0);
            black_box(recognition);
        });
    });
    criterion.final_summary();
    Ok(())
}
