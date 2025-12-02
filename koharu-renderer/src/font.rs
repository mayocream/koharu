use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, anyhow};
use fontdb::Database;
use swash::FontRef;
use swash::text::Script;
use unicode_script::UnicodeScript;

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
    pub fn filter_by_families(&self, families: &[String]) -> Vec<FaceInfo> {
        self.all()
            .iter()
            .filter(|face| {
                face.families
                    .iter()
                    .any(|(family, _)| families.iter().any(|f| f == family)) &&
                // skip thin, italic, bold variants
                face.weight == fontdb::Weight::NORMAL &&
                face.stretch == fontdb::Stretch::Normal &&
                face.style == fontdb::Style::Normal
            })
            .cloned()
            .collect()
    }

    /// Returns font faces that belong to any of the specified families,
    /// prioritized by language support for the given text.
    pub fn filter_by_families_for_text(
        &self,
        families: &[String],
        text: &String,
    ) -> (Vec<FaceInfo>, Script) {
        let mut collected = self.filter_by_families(families);

        let script = {
            let chars: Vec<char> = text.chars().collect();

            if chars
                .iter()
                .all(|ch| ch.script() == unicode_script::Script::Latin)
            {
                Script::Latin // only latin
            } else if chars.iter().any(|ch| {
                matches!(
                    ch.script(),
                    unicode_script::Script::Hiragana | unicode_script::Script::Katakana
                )
            }) {
                Script::Hiragana // Using Hiragana to represent Japanese scripts
            } else if chars
                .iter()
                .any(|ch| ch.script() == unicode_script::Script::Hangul)
            {
                Script::Hangul // Korean
            } else if chars
                .iter()
                .any(|ch| ch.script() == unicode_script::Script::Han)
            {
                Script::Han
            } else {
                Script::Latin // Default
            }
        };

        // Define language order based on script
        let languages = match script {
            Script::Han => vec![
                Language::Chinese_PeoplesRepublicOfChina,
                Language::Chinese_Taiwan,
                Language::Chinese_HongKongSAR,
                Language::English_UnitedStates,
            ],
            Script::Hiragana => vec![Language::Japanese_Japan, Language::English_UnitedStates],
            Script::Hangul => vec![Language::Korean_Korea, Language::English_UnitedStates],
            _ => vec![],
        };

        collected.sort_by_key(|face| {
            languages
                .iter()
                .position(|lang| face.families.iter().any(|(_, l)| l == lang))
                .unwrap_or(languages.len())
        });
        (collected, script)
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
