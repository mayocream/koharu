use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::Result;
use criterion::Criterion;
use koharu_ml::pp_doclayout_v3::PPDocLayoutV3;
use koharu_torch::Cuda;

const THRESHOLD: f32 = 0.5;

#[tokio::main]
async fn main() -> Result<()> {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join("object_detection")
        .join("1.jpg");

    koharu_ml::init_torch().await?;
    let image = image::open(&input)?;
    let model = PPDocLayoutV3::load(koharu_ml::Device::cuda(0)).await?;

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    criterion.bench_function("pp_doclayout_v3/inference", |bencher| {
        bencher.iter(|| {
            Cuda::synchronize(0);
            let detections = model
                .inference(black_box(&image), black_box(THRESHOLD))
                .expect("PP-DocLayout-V3 inference failed");
            black_box(detections);
            Cuda::synchronize(0);
        });
    });
    criterion.final_summary();

    Ok(())
}
