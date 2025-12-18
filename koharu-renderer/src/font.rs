use std::{path::Path, sync::Arc};

use anyhow::Context;
use font_kit::{
    handle::Handle,
    source::SystemSource,
    sources::{fs::FsSource, multi::MultiSource},
};

pub use font_kit::{
    family_name::FamilyName,
    properties::{Properties, Stretch, Style, Weight},
};
use skia_safe::FontMgr;

/// A loaded font ready for shaping and rendering.
///
/// The font data is reference-counted for cheap cloning.
#[derive(Clone)]
pub struct Font {
    /// Font data stored in an Arc for cheap cloning
    data: Arc<Vec<u8>>,
    /// Index within font collection (0 for single-font files)
    index: u32,
}

impl Font {
    /// Creates a skrifa FontRef for metric queries.
    pub fn skrifa(&self) -> anyhow::Result<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(&self.data, self.index)
            .context("failed to create skrifa FontRef")
    }

    /// Creates a harfrust FontRef for text shaping.
    pub fn harfrust(&self) -> anyhow::Result<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(&self.data, self.index)
            .context("failed to create harfrust FontRef")
    }

    pub fn skia(&self) -> anyhow::Result<skia_safe::Typeface> {
        FontMgr::new()
            .new_from_data(&self.data, self.index as usize)
            .context("failed to create skia Typeface")
    }
}

/// A collection of font sources for font discovery and loading.
///
/// Combines system fonts with optional custom font directories.
pub struct FontBook {
    source: MultiSource,
}

impl FontBook {
    /// Creates a FontBook with only system fonts.
    pub fn new() -> Self {
        Self {
            source: MultiSource::from_sources(vec![Box::new(SystemSource::new())]),
        }
    }

    /// Creates a FontBook with system fonts and fonts from a custom directory.
    pub fn with_local_fonts<P: AsRef<Path>>(path: P) -> Self {
        Self {
            source: MultiSource::from_sources(vec![
                Box::new(SystemSource::new()),
                Box::new(FsSource::in_path(path)),
            ]),
        }
    }

    /// Returns all available font family names.
    pub fn all_families(&self) -> anyhow::Result<Vec<String>> {
        self.source
            .all_families()
            .context("failed to enumerate font families")
    }

    /// Queries for a font by family names (with fallbacks) and properties.
    ///
    /// The first matching font from the family list will be returned.
    pub fn query(&self, families: &[FamilyName], properties: &Properties) -> anyhow::Result<Font> {
        let handle = self
            .source
            .select_best_match(families, properties)
            .with_context(|| format!("no font found for families: {families:?}"))?;

        let font_index = match &handle {
            Handle::Path { font_index, .. } => *font_index,
            Handle::Memory { font_index, .. } => *font_index,
        };

        let loaded = handle.load().context("failed to load font")?;
        let data = loaded
            .copy_font_data()
            .context("failed to copy font data")?;

        Ok(Font {
            data,
            index: font_index,
        })
    }
}
