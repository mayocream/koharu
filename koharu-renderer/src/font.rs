use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, anyhow};
use fontdb::{Database, FaceInfo, Family, ID, Query, Stretch, Style, Weight};
use swash::{FontRef, text::Script};

/// Query parameters used to select a font from the system database.
#[derive(Clone, Copy, Debug)]
pub struct FontQuery<'a> {
    pub families: &'a [Family<'a>],
    pub weight: Weight,
    pub stretch: Stretch,
    pub style: Style,
    pub script: Option<Script>,
}

impl<'a> FontQuery<'a> {
    /// Creates a new query with the provided family list and default styling.
    pub fn new(families: &'a [Family<'a>]) -> Self {
        Self {
            families,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
            script: None,
        }
    }

    pub fn with_weight(mut self, weight: Weight) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_stretch(mut self, stretch: Stretch) -> Self {
        self.stretch = stretch;
        self
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn with_script(mut self, script: Script) -> Self {
        self.script = Some(script);
        self
    }

    fn to_fontdb_query(&self) -> Query<'a> {
        Query {
            families: self.families,
            weight: self.weight,
            stretch: self.stretch,
            style: self.style,
        }
    }
}

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

    /// Attempts to find a font that satisfies the query and optionally supports the requested script.
    ///
    /// The method first delegates to [`fontdb::Database::query`] for family/style matching. When a
    /// script is requested and the best match does not support it, the fallback search scans the
    /// database for the closest match that does.
    pub fn query_font(&mut self, query: &FontQuery<'_>) -> Result<Option<Font>> {
        let db_query = query.to_fontdb_query();
        if let Some(id) = self.database.query(&db_query) {
            let face = self
                .database
                .face(id)
                .cloned()
                .with_context(|| format!("missing face info for id {:?}", id))?;
            let font = self.build_font(&face)?;
            if query
                .script
                .map_or(true, |script| font.supports_script(script))
            {
                return Ok(Some(font));
            }
        }

        self.search_database(query)
    }

    /// Lists metadata for every font currently loaded.
    pub fn list_fonts(&self) -> Vec<FontMetadata> {
        let mut fonts: Vec<_> = self.database.faces().map(FontMetadata::from_face).collect();
        fonts.sort_by(|a, b| {
            a.primary_family
                .cmp(&b.primary_family)
                .then_with(|| a.post_script_name.cmp(&b.post_script_name))
        });
        fonts
    }

    fn search_database(&mut self, query: &FontQuery<'_>) -> Result<Option<Font>> {
        let mut best: Option<(Font, u32)> = None;
        let faces: Vec<_> = self.database.faces().cloned().collect();
        for face in &faces {
            if !self.matches_families(face, query.families) {
                continue;
            }

            let font = self.build_font(face)?;
            if let Some(script) = query.script {
                if !font.supports_script(script) {
                    continue;
                }
            }

            let score = self.match_score(face, query);
            match &mut best {
                Some((_, best_score)) if score >= *best_score => {}
                _ => best = Some((font, score)),
            }
        }

        Ok(best.map(|(font, _)| font))
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

    fn matches_families(&self, face: &FaceInfo, families: &[Family<'_>]) -> bool {
        if families.is_empty() {
            return true;
        }

        families.iter().any(|family| {
            let requested = self.database.family_name(family);
            face.families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(requested))
        })
    }

    fn match_score(&self, face: &FaceInfo, query: &FontQuery<'_>) -> u32 {
        let weight_penalty = face.weight.0.abs_diff(query.weight.0) as u32;
        let stretch_penalty =
            u32::from(face.stretch.to_number().abs_diff(query.stretch.to_number()));
        let style_penalty = if face.style == query.style { 0 } else { 5_000 };
        weight_penalty + stretch_penalty + style_penalty
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

    pub fn supports_script(&self, script: Script) -> bool {
        self.font_ref()
            .map(|font| {
                font.writing_systems()
                    .any(|system| system.script() == Some(script))
            })
            .unwrap_or(false)
    }
}

/// Read-only information describing a font in the system database.
#[derive(Debug, Clone)]
pub struct FontMetadata {
    pub id: ID,
    pub primary_family: String,
    pub families: Vec<String>,
    pub post_script_name: String,
    pub style: Style,
    pub weight: Weight,
    pub stretch: Stretch,
    pub monospaced: bool,
}

impl FontMetadata {
    fn from_face(face: &FaceInfo) -> Self {
        let families = face
            .families
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let post_script_name = face.post_script_name.clone();
        let primary_family = families
            .first()
            .cloned()
            .unwrap_or_else(|| post_script_name.clone());

        Self {
            id: face.id,
            primary_family,
            families,
            post_script_name,
            style: face.style,
            weight: face.weight,
            stretch: face.stretch,
            monospaced: face.monospaced,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_system_fonts_is_not_empty() {
        let provider = FontBook::new();
        let fonts = provider.list_fonts();
        assert!(!fonts.is_empty(), "system font list should not be empty");
        assert!(
            fonts
                .iter()
                .all(|font| !font.primary_family.is_empty() && !font.post_script_name.is_empty()),
            "every font entry should expose basic metadata"
        );
    }

    #[test]
    fn query_font_respects_script() -> Result<()> {
        let mut provider = FontBook::new();
        let families = [Family::SansSerif];
        let query = FontQuery::new(&families).with_script(Script::Latin);
        let font = provider
            .query_font(&query)?
            .expect("expected to find at least one sans-serif font supporting Latin");
        let font_ref = font.font_ref()?;
        assert!(
            font_ref
                .writing_systems()
                .any(|system| system.script() == Some(Script::Latin)),
            "selected font should report Latin script support"
        );
        Ok(())
    }
}
