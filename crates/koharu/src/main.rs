#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use clap::Parser;
use koharu::panic;
use koharu::sentry;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Arguments {
    #[arg(value_name = "PROJECT", conflicts_with = "worker")]
    project: Option<PathBuf>,

    #[arg(long, hide = true)]
    worker: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    if arguments.worker {
        return koharu_app::serve_worker().await;
    }
    let _guard = sentry::initialize();
    panic::install();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .with(sentry::tracing_layer())
        .with(koharu::tracing::TimingLayer::new())
        .init();
    koharu_app::app::run(arguments.project)
}
