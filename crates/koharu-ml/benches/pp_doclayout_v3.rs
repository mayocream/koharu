use std::{hint::black_box, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use criterion::Criterion;
use koharu_ml::pp_doclayout_v3::PPDocLayoutV3;
use koharu_torch::{Cuda, Device};

const THRESHOLD: f32 = 0.5;

#[tokio::main]
async fn main() -> Result<()> {
    let input = PathBuf::from(env!("CARGO_WORKSPACE_DIR"))
        .join("data")
        .join("bluearchive_comics")
        .join("1.jpg");

    koharu_ml::init().await?;
    let image = image::open(&input)
        .with_context(|| format!("failed to open input image {}", input.display()))?;
    let model = PPDocLayoutV3::load(false).await?;

    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();

    criterion.bench_function("pp_doclayout_v3/inference", |bencher| {
        bencher.iter(|| {
            sync(model.device());
            let detections = model
                .inference(black_box(&image), black_box(THRESHOLD))
                .expect("PP-DocLayout-V3 inference failed");
            sync(model.device());
            black_box(detections);
        });
    });
    criterion.final_summary();

    Ok(())
}

fn sync(device: Device) {
    if let Device::Cuda(index) = device {
        Cuda::synchronize(index as i64);
    }
}
