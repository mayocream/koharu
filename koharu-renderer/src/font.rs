use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use fontique::{
    Attributes, Blob, Collection, CollectionOptions, FamilyId, FontStyle as Style,
    FontWeight as Weight, FontWidth as Stretch, GenericFamily, QueryFamily, QueryStatus,
    SourceCache, SourceCacheOptions,
};
use once_cell::sync::OnceCell;

/// Font family names for font lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FamilyName {
    /// A named font family.
    Title(String),
    /// Generic serif family.
    Serif,
    /// Generic sans-serif family.
    SansSerif,
    /// Generic cursive family.
    Cursive,
    /// Generic fantasy family.
    Fantasy,
    /// Generic monospace family.
    Monospace,
}

impl FamilyName {
    fn to_query_family(&self) -> QueryFamily<'_> {
        match self {
            FamilyName::Title(name) => QueryFamily::Named(name.as_str()),
            FamilyName::Serif => QueryFamily::Generic(GenericFamily::Serif),
            FamilyName::SansSerif => QueryFamily::Generic(GenericFamily::SansSerif),
            FamilyName::Cursive => QueryFamily::Generic(GenericFamily::Cursive),
            FamilyName::Fantasy => QueryFamily::Generic(GenericFamily::Fantasy),
            FamilyName::Monospace => QueryFamily::Generic(GenericFamily::Monospace),
        }
    }
}

/// Font properties used to match against the font database.
#[derive(Debug, Clone)]
pub struct Properties {
    pub weight: Weight,
    pub stretch: Stretch,
    pub style: Style,
}

impl Default for Properties {
    fn default() -> Self {
        Self {
            weight: Weight::NORMAL,
            stretch: Stretch::NORMAL,
            style: Style::Normal,
        }
    }
}

impl Properties {
    fn to_attributes(&self) -> Attributes {
        Attributes::new(self.stretch, self.style, self.weight)
    }
}

/// A loaded font ready for shaping and rendering.
///
/// The font data is reference-counted for cheap cloning.
#[derive(Clone, Debug)]
pub struct Font {
    /// Font data stored in a shared blob for cheap cloning.
    blob: Blob<u8>,
    /// Index within font collection (0 for single-font files)
    index: u32,
    /// Cached fontdue font to avoid re-parsing font data on every render.
    fontdue: Arc<OnceCell<Arc<fontdue::Font>>>,
}

impl Font {
    /// Creates a skrifa FontRef for metric queries.
    pub fn skrifa(&self) -> anyhow::Result<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(self.blob.as_ref(), self.index)
            .context("failed to create skrifa FontRef")
    }

    /// Creates a harfrust FontRef for text shaping.
    pub fn harfrust(&self) -> anyhow::Result<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(self.blob.as_ref(), self.index)
            .context("failed to create harfrust FontRef")
    }

    pub fn fontdue(&self) -> anyhow::Result<Arc<fontdue::Font>> {
        let font = self.fontdue.get_or_try_init(|| {
            let settings = fontdue::FontSettings {
                collection_index: self.index,
                ..Default::default()
            };
            let font = fontdue::Font::from_bytes(self.blob.as_ref(), settings)
                .map_err(|err| anyhow::anyhow!(err))
                .context("failed to create fontdue Font")?;
            Ok::<_, anyhow::Error>(Arc::new(font))
        })?;
        Ok(Arc::clone(font))
    }

    /// Returns true if the font contains a glyph for the given character.
    pub fn has_glyph(&self, character: char) -> bool {
        self.fontdue()
            .map(|font| font.has_glyph(character))
            .unwrap_or(false)
    }
}

pub(crate) fn font_key(font: &Font) -> usize {
    font as *const Font as usize
}

pub(crate) fn select_font(cluster: &str, fonts: &[&Font]) -> usize {
    let mut chars = cluster.chars();
    let Some(ch) = chars.next() else {
        return 0;
    };

    if cluster.len() == ch.len_utf8() {
        for (index, font) in fonts.iter().enumerate() {
            if font.has_glyph(ch) {
                return index;
            }
        }
        return 0;
    }

    'fonts: for (index, font) in fonts.iter().enumerate() {
        for ch in cluster.chars() {
            if !font.has_glyph(ch) {
                continue 'fonts;
            }
        }
        return index;
    }

    0
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    family_id: FamilyId,
    family_index: usize,
    index: u32,
}

/// A collection of font sources for font discovery and loading.
///
/// Combines system fonts with optional custom font directories.
pub struct FontBook {
    collection: Collection,
    source_cache: SourceCache,
    cache: HashMap<CacheKey, Font>,
}

impl FontBook {
    /// Creates a FontBook with only system fonts.
    pub fn new() -> Self {
        let collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        let source_cache = SourceCache::new(SourceCacheOptions { shared: true });
        Self {
            collection,
            source_cache,
            cache: HashMap::new(),
        }
    }

    /// Returns all available font family names.
    pub fn all_families(&mut self) -> Vec<String> {
        self.collection
            .family_names()
            .map(|name| name.to_string())
            .collect()
    }

    /// Queries for a font by family names (with fallbacks) and properties.
    ///
    /// The first matching font from the family list will be returned.
    pub fn query(
        &mut self,
        families: &[FamilyName],
        properties: &Properties,
    ) -> anyhow::Result<Font> {
        let mut query = self.collection.query(&mut self.source_cache);
        query.set_families(families.iter().map(|name| name.to_query_family()));
        query.set_attributes(properties.to_attributes());

        let mut selected = None;
        query.matches_with(|font| {
            // Clone the necessary fields from font to avoid borrow issues
            selected = Some((font.family.0, font.family.1, font.index, font.blob.clone()));
            QueryStatus::Stop
        });

        let (family_id, family_index, index, blob) =
            selected.with_context(|| format!("no font found for families: {families:?}"))?;

        let cache_key = CacheKey {
            family_id,
            family_index,
            index,
        };
        if let Some(font) = self.cache.get(&cache_key) {
            return Ok(font.clone());
        }

        let font = Font {
            blob,
            index,
            fontdue: Arc::new(OnceCell::new()),
        };

        self.cache.insert(cache_key, font.clone());
        Ok(font)
    }
}

impl Default for FontBook {
    fn default() -> Self {
        Self::new()
    }
}
