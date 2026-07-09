use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use koharu_bindgen::Generator;

#[derive(Parser)]
#[command(
    version,
    about = "Generate bindgen bindings with top-level dynamic loading"
)]
struct Args {
    #[arg(long)]
    header: PathBuf,

    #[arg(short, long)]
    output: PathBuf,

    #[arg(long)]
    library_name: String,

    #[arg(long = "clang-arg")]
    clang_args: Vec<String>,

    #[arg(long = "allowlist-function")]
    allowlist_functions: Vec<String>,

    #[arg(long = "allowlist-type")]
    allowlist_types: Vec<String>,

    #[arg(long = "allowlist-var")]
    allowlist_vars: Vec<String>,

    #[arg(long = "blocklist-function")]
    blocklist_functions: Vec<String>,

    #[arg(long = "blocklist-type")]
    blocklist_types: Vec<String>,

    #[arg(long = "blocklist-var")]
    blocklist_vars: Vec<String>,

    #[arg(long)]
    no_layout_tests: bool,

    #[arg(long)]
    use_core: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut builder = bindgen::Builder::default().header(args.header.display().to_string());

    for arg in args.clang_args {
        builder = builder.clang_arg(arg);
    }
    for filter in args.allowlist_functions {
        builder = builder.allowlist_function(filter);
    }
    for filter in args.allowlist_types {
        builder = builder.allowlist_type(filter);
    }
    for filter in args.allowlist_vars {
        builder = builder.allowlist_var(filter);
    }
    for filter in args.blocklist_functions {
        builder = builder.blocklist_function(filter);
    }
    for filter in args.blocklist_types {
        builder = builder.blocklist_type(filter);
    }
    for filter in args.blocklist_vars {
        builder = builder.blocklist_var(filter);
    }
    if args.no_layout_tests {
        builder = builder.layout_tests(false);
    }
    if args.use_core {
        builder = builder.use_core();
    }

    let generator = Generator::new(builder, args.library_name);

    generator
        .write_to_file(&args.output)
        .with_context(|| format!("failed to write {}", args.output.display()))?;

    Ok(())
}
