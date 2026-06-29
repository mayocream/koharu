use clap::Parser;
use image::DynamicImage;
use koharu_ml::lama::Lama;
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};
use std::{
    env,
    time::{Duration, Instant},
};

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    mask: String,

    #[arg(long, value_name = "FILE")]
    bubble_mask: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, default_value_t = false)]
    cpu: bool,

    #[arg(long, default_value_t = 0)]
    warmup: usize,

    #[arg(long, default_value_t = 1)]
    iterations: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    common::init_tracing();

    let cli = Cli::parse();
    if cli.iterations == 0 {
        anyhow::bail!("--iterations must be greater than 0");
    }

    let runtime = RuntimeManager::new(
        default_app_data_root(),
        if cli.cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    runtime.prepare().await?;
    if !cli.cpu {
        add_cuda_runtime_path(&runtime)?;
    }

    let image = image::open(&cli.input)?;
    let mask = image::open(&cli.mask)?;
    let bubble_mask = image::open(&cli.bubble_mask)?;

    let model = Lama::load(&runtime, cli.cpu).await?;
    let output = benchmark("lama", cli.warmup, cli.iterations, || {
        model.inference(&image, &mask, &bubble_mask)
    })?;
    output.save(&cli.output)?;

    Ok(())
}

fn benchmark(
    label: &str,
    warmup: usize,
    iterations: usize,
    mut run: impl FnMut() -> anyhow::Result<DynamicImage>,
) -> anyhow::Result<DynamicImage> {
    for index in 0..warmup {
        let start = Instant::now();
        let _ = run()?;
        println!(
            "{label} warmup {}/{} took: {:?}",
            index + 1,
            warmup,
            start.elapsed()
        );
    }

    let mut durations = Vec::with_capacity(iterations);
    let mut output = None;
    for index in 0..iterations {
        let start = Instant::now();
        let result = run()?;
        let duration = start.elapsed();
        println!(
            "{label} iteration {}/{} took: {:?}",
            index + 1,
            iterations,
            duration
        );
        durations.push(duration);
        output = Some(result);
    }

    print_summary(label, &durations);

    Ok(output.expect("at least one iteration should have produced output"))
}

fn print_summary(label: &str, durations: &[Duration]) {
    let total: Duration = durations.iter().copied().sum();
    let mean_ms = total.as_secs_f64() * 1000.0 / durations.len() as f64;
    let min_ms = durations
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .fold(f64::INFINITY, f64::min);
    let max_ms = durations
        .iter()
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .fold(f64::NEG_INFINITY, f64::max);
    println!(
        "{label} benchmark summary: iterations={}, mean={mean_ms:.2} ms, min={min_ms:.2} ms, max={max_ms:.2} ms",
        durations.len()
    );
}

fn add_cuda_runtime_path(runtime: &RuntimeManager) -> anyhow::Result<()> {
    let cuda_dir = runtime.root().join("runtime").join("cuda");
    if !cuda_dir.exists() {
        return Ok(());
    }

    let mut paths = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    if paths.iter().any(|path| path == &cuda_dir) {
        return Ok(());
    }

    paths.insert(0, cuda_dir);
    let joined = env::join_paths(paths)?;

    unsafe {
        env::set_var("PATH", joined);
    }

    Ok(())
}
