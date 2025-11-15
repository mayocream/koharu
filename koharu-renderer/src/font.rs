use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, anyhow};
use fontdb::{Database, FaceInfo, ID, Query};
use swash::FontRef;

/// Loads and caches system fonts via [`fontdb`] and exposes them as [`FontRef`]s.
pub struct FontBook {
    database: Database,
    cache: HashMap<ID, Arc<[u8]>>,
}

impl FontBook {
    /// Builds a provider that eagerly loads fonts from the operating system.
    pub fn new() -> Self {
        let mut database = Database::new();
        database.load_system_fonts();
        Self::from_database(database)
    }

    /// Builds a provider from an existing database. Primarily useful in tests.
    pub fn from_database(database: Database) -> Self {
        Self {
            database,
            cache: HashMap::new(),
        }
    }

    /// Returns a read-only view of the underlying database.
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Attempts to find a font that satisfies the query.
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

    /// Lists metadata for every font currently loaded.
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

/// Holds cached font data and allows creating `swash::FontRef` values.
#[derive(Clone)]
pub struct Font {
    id: ID,
    index: u32,
    data: Arc<[u8]>,
}

impl Font {
    pub fn id(&self) -> ID {
        self.id
    }

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
