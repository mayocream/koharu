mod rewrite;

use std::{fs, io::Write, path::Path};

use anyhow::{Context, Result};

pub use rewrite::rewrite_bindings;

/// Bindgen-backed generator that post-processes C function bindings into
/// top-level dynamic-loading adapters.
pub struct Generator {
    builder: bindgen::Builder,
    library_name: String,
}

impl Generator {
    pub fn new(builder: bindgen::Builder, library_name: impl Into<String>) -> Self {
        Self {
            builder,
            library_name: library_name.into(),
        }
    }

    pub fn from_header(header: impl AsRef<Path>, library_name: impl Into<String>) -> Self {
        Self::new(
            bindgen::Builder::default().header(header.as_ref().display().to_string()),
            library_name,
        )
    }

    pub fn with_bindgen(
        mut self,
        configure: impl FnOnce(bindgen::Builder) -> bindgen::Builder,
    ) -> Self {
        self.builder = configure(self.builder);
        self
    }

    pub fn generate(self) -> Result<String> {
        let bindings = self
            .builder
            .generate()
            .map_err(|error| anyhow::anyhow!("{error}"))
            .context("failed to run bindgen")?;
        let source = bindings_to_string(&bindings).context("failed to render bindgen output")?;

        let source = rewrite_bindings(&source, &self.library_name)
            .context("failed to rewrite bindgen output")?;
        Ok(source)
    }

    pub fn write_to_file(self, path: impl AsRef<Path>) -> Result<()> {
        let source = self.generate()?;
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, source).with_context(|| format!("failed to write {}", path.display()))
    }
}

fn bindings_to_string(bindings: &bindgen::Bindings) -> Result<String> {
    let mut bytes = Vec::new();
    bindings.write(Box::new(&mut bytes) as Box<dyn Write>)?;
    String::from_utf8(bytes).context("bindgen emitted non-UTF-8 Rust source")
}
