use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use koharu_bundle::{BundleConfig, Package};

#[derive(Debug, Parser)]
#[command(about = "Bundle Koharu with cef-rs and tauri-bundler")]
struct Cli {
    #[arg(long, value_enum)]
    package: PackageArg,
    #[arg(long)]
    target: String,
    #[arg(long)]
    executable: PathBuf,
    #[arg(long, required = true)]
    libraries: Vec<PathBuf>,
    #[arg(long)]
    ui: PathBuf,
    #[arg(long = "license", required = true)]
    licenses: Vec<PathBuf>,
    #[arg(long, default_value = "packages/koharu/public/icon-large.png")]
    icon: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    version: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PackageArg {
    Nsis,
    Appimage,
    Dmg,
}

impl From<PackageArg> for Package {
    fn from(value: PackageArg) -> Self {
        match value {
            PackageArg::Nsis => Self::Nsis,
            PackageArg::Appimage => Self::AppImage,
            PackageArg::Dmg => Self::Dmg,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let result = koharu_bundle::bundle(&BundleConfig {
        package: cli.package.into(),
        target: cli.target,
        executable: cli.executable,
        libraries: cli.libraries,
        ui: cli.ui,
        licenses: cli.licenses,
        icon: cli.icon,
        output: cli.output,
        version: cli.version,
    });
    match result {
        Ok(paths) => {
            for path in paths {
                println!("{}", path.display());
            }
        }
        Err(error) => {
            eprintln!("failed to bundle Koharu: {error}");
            std::process::exit(1);
        }
    }
}
