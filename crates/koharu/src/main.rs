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

    #[arg(
        long,
        hide = true,
        value_name = "RUNTIME",
        value_parser = ["torch", "llama", "diffusion"]
    )]
    worker: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    if let Some(runtime) = arguments.worker {
        return koharu_worker::serve(&runtime);
    }
    let _guard = sentry::initialize();
    panic::install();
    app::run(arguments.project)
}
