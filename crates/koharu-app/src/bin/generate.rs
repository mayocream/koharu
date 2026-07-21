use std::{borrow::Cow, fs, path::PathBuf};

use anyhow::{Context, Result};
use koharu_app::protocol::{BridgeEvent, BridgeMessage};
use specta::{Format, FormatError, Types, datatype::DataType};
use specta_typescript::{Typescript, semantic::Configuration};

fn main() -> Result<()> {
    let output = output_path();
    fs::write(&output, generate()?)
        .with_context(|| format!("failed to write generated types to {}", output.display()))?;
    println!("generated {}", output.display());
    Ok(())
}

fn output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_WORKSPACE_DIR")).join("ui/lib/koharu/protocol.ts")
}

fn generate() -> Result<String> {
    let types = Types::default()
        .register::<BridgeMessage>()
        .register::<BridgeEvent>();
    Typescript::default()
        .header(
            "// This file is generated from the Rust desktop protocol by `cargo run -p koharu-app --bin generate`.\n// Do not edit it by hand.\n",
        )
        .export(&types, DesktopFormat::default())
        .context("failed to export the Rust desktop protocol")
}

struct DesktopFormat {
    semantic: Configuration,
}

impl Default for DesktopFormat {
    fn default() -> Self {
        Self {
            // Every protocol float is validated as finite before it crosses the
            // JSON bridge, so its JavaScript representation is always `number`.
            semantic: Configuration::empty().enable_lossless_floats(),
        }
    }
}

impl Format for DesktopFormat {
    fn map_types(&'_ self, types: &Types) -> Result<Cow<'_, Types>, FormatError> {
        let types = specta_serde::Format.map_types(types)?;
        Ok(Cow::Owned(
            self.semantic.apply_types(types.as_ref()).into_owned(),
        ))
    }

    fn map_type(
        &'_ self,
        types: &Types,
        data_type: &DataType,
    ) -> Result<Cow<'_, DataType>, FormatError> {
        Ok(Cow::Owned(
            specta_serde::Format
                .map_type(types, data_type)?
                .into_owned(),
        ))
    }
}
