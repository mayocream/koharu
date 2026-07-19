#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use clap::Parser;
use koharu::app;
use koharu::panic;
use koharu::sentry;

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
        return koharu_pipeline::serve_worker().await;
    }
    let _guard = sentry::initialize();
    panic::install();
    app::run(arguments.project)
}
