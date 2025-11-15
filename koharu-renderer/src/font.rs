//! Font management and caching system.
//!
//! This module provides a high-level interface for working with system fonts,
//! including font discovery, caching, and conversion to swash FontRef objects
//! for text shaping and rendering.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, anyhow};
use fontdb::{Database, FaceInfo, ID, Query};
use swash::FontRef;

/// Font provider that loads and caches system fonts.
///
/// `FontBook` provides a high-level interface for font discovery and loading.
/// It maintains an internal cache of font data to avoid repeated file I/O
/// and integrates with the system's font management.
pub struct FontBook {
    /// The underlying font database for system font discovery.
    database: Database,
    /// Cache of loaded font data indexed by font ID.
    cache: HashMap<ID, Arc<[u8]>>,
}

impl FontBook {
    /// Creates a new font book with all available system fonts loaded.
    ///
    /// This eagerly discovers all fonts available on the system and prepares
    /// them for querying. The actual font data is loaded lazily when needed.
    pub fn new() -> Self {
        let mut database = Database::new();
        database.load_system_fonts();
        Self::from_database(database)
    }

    /// Creates a font book from an existing font database.
    ///
    /// This is primarily useful for testing with custom font collections
    /// or when you need fine-grained control over which fonts are available.
    pub fn from_database(database: Database) -> Self {
        Self {
            database,
            cache: HashMap::new(),
        }
    }

    /// Finds a font that best matches the given criteria.
    ///
    /// Returns `None` if no font matches the query, or an error if font loading fails.
    /// The query uses fontdb's matching algorithm to find the best available font.
    pub fn query(&mut self, query: &Query<'_>) -> Result<Option<Font>> {
        if let Some(id) = self.database.query(query) {
            let face = self
                .database
                .face(id)
                .cloned()
                .with_context(|| format!("missing face info for id {:?}", id))?;
            let font = self.build_font(&face)?;
            return Ok(Some(font));
        }

        Ok(None)
    }

    /// Returns metadata for all available fonts, sorted by name.
    ///
    /// This provides a complete list of all fonts that can be queried,
    /// useful for font selection UIs or debugging font availability.
    pub fn list_all(&self) -> Vec<FaceInfo> {
        let mut faces: Vec<_> = self.database.faces().cloned().collect();
        faces.sort_by_key(|face| face.post_script_name.clone());
        faces
    }

    fn build_font(&mut self, face: &FaceInfo) -> Result<Font> {
        let data = self.cached_face_data(face.id)?;
        Ok(Font {
            id: face.id,
            index: face.index,
            data,
        })
    }

    fn cached_face_data(&mut self, id: ID) -> Result<Arc<[u8]>> {
        if let Some(data) = self.cache.get(&id) {
            return Ok(data.clone());
        }

        let bytes = self
            .database
            .with_face_data(id, |data, _| data.to_vec())
            .with_context(|| format!("failed to load font data for {:?}", id))?;
        let data: Arc<[u8]> = Arc::from(bytes);
        self.cache.insert(id, data.clone());

        Ok(data)
    }
}

/// A cached font with its data ready for text shaping and rendering.
///
/// `Font` represents a loaded font face with cached binary data that can be
/// efficiently converted to swash FontRef objects for text operations.
#[derive(Clone, Debug)]
pub struct Font {
    /// Unique identifier from the font database.
    id: ID,
    /// Face index within the font file (for TTC files).
    index: u32,
    /// Cached font data shared across instances.
    data: Arc<[u8]>,
}

impl Font {
    /// Builds a `FontRef` backed by the cached font data.
    pub fn font_ref(&self) -> Result<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index as usize)
            .ok_or_else(|| anyhow!("unable to build FontRef for face {:?}", self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontdb::{Family, Query, Stretch, Style, Weight};

    #[test]
    fn list_system_fonts_is_not_empty() {
        let provider = FontBook::new();
        let fonts = provider.list_all();
        assert!(!fonts.is_empty(), "system font list should not be empty");
        assert!(
            fonts.iter().all(|font| !font.post_script_name.is_empty()),
            "every font entry should expose basic metadata"
        );
    }

    #[test]
    fn query_font_returns_font() -> Result<()> {
        let mut provider = FontBook::new();
        let families = [Family::SansSerif];
        let query = Query {
            families: &families,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        };
        let font = provider
            .query(&query)?
            .expect("expected to find at least one sans-serif font");
        let font_ref = font.font_ref()?;
        assert!(font_ref.writing_systems().next().is_some());
        Ok(())
    }
}
