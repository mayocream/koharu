use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, anyhow};
use fontdb::Database;
use swash::FontRef;

pub use fontdb::{FaceInfo, ID, Language};

/// A font provider that loads system fonts and caches font data.
pub struct FontBook {
    database: Database,
    /// cached font data by face ID
    cache: HashMap<ID, Arc<[u8]>>,
}

impl FontBook {
    pub fn new() -> Self {
        let mut database = Database::new();
        database.load_system_fonts();
        Self {
            database,
            cache: HashMap::new(),
        }
    }

    /// Returns all available font faces sorted by PostScript name.
    pub fn all(&self) -> Vec<FaceInfo> {
        let mut faces: Vec<_> = self.database.faces().cloned().collect();
        faces.sort_by_key(|face| face.post_script_name.clone());
        faces
    }

    /// Returns font faces that support the specified language.
    ///
    /// refer: https://learn.microsoft.com/en-us/typography/opentype/spec/name#windows-language-ids
    pub fn filter_by_language(&self, languages: &[Language]) -> Vec<FaceInfo> {
        self.all()
            .iter()
            .filter(|face| {
                face.families
                    .iter()
                    .any(|(_, language)| languages.iter().any(|l| l == language))
            })
            .cloned()
            .collect()
    }

    /// Returns font faces that belong to any of the specified families.
    pub fn filter_by_families(&self, families: &[String], languages: &[Language]) -> Vec<FaceInfo> {
        let mut collected: Vec<FaceInfo> = self.all()
            .iter()
            .filter(|face| {
                face.families
                    .iter()
                    .any(|(family, _)| families.iter().any(|f| f == family))
            })
            .cloned()
            .collect();

        collected.sort_by_key(|face| {
                languages.iter().position(|lang| {
                    face.families.iter().any(|(_, l)| l == lang)
                }).unwrap_or(languages.len())
            });
        collected
    }

    /// Loads the font data for the specified face, utilizing caching.
    pub fn font(&mut self, face: &FaceInfo) -> Result<Font> {
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

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Font {
    id: ID,
    index: u32,
    data: Arc<[u8]>,
}

impl Font {
    /// Builds a swash's `FontRef` for this font.
    pub fn font_ref(&self) -> Result<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index as usize)
            .ok_or_else(|| anyhow!("unable to build FontRef for face {:?}", self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_system_fonts_is_not_empty() {
        let provider = FontBook::new();
        let fonts = provider.all();

        assert!(!fonts.is_empty(), "system font list should not be empty");
        assert!(
            fonts.iter().all(|font| !font.post_script_name.is_empty()),
            "every font entry should expose basic metadata"
        );
    }

    #[test]
    fn filter_by_language_tag() {
        let provider = FontBook::new();
        let fonts = provider.all();
        let ja = fonts
            .iter()
            .filter(|face| {
                face.families
                    .iter()
                    .any(|(_, lang)| lang == &Language::Japanese_Japan)
            })
            .map(|face| face.post_script_name.clone())
            .collect::<Vec<_>>();

        assert!(!ja.is_empty(), "expected to find Japanese fonts");
    }
}
