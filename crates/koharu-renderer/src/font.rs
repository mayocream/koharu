//! Font discovery, matching, fallback, and resolved font instances.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use fontique::{
    Attributes, Blob, Collection, CollectionOptions, FallbackKey, FontStyle, GenericFamily,
    Language, QueryFamily, QueryFont, QueryStatus, Script, SourceCache,
};
use harfrust::{ShaperData, ShaperInstance, Variation};
use skrifa::{MetadataProvider, instance::LocationRef, string::StringId};

/// A resolved face and variable-font instance used by shaping, metrics, and drawing.
#[derive(Clone)]
pub struct Font {
    data: Blob<u8>,
    index: u32,
    family_name: String,
    post_script_name: String,
    synthesis: fontique::Synthesis,
    shaper_data: Arc<ShaperData>,
    shaper_instance: ShaperInstance,
    normalized_coords: Vec<skrifa::instance::NormalizedCoord>,
    vello_coords: Vec<vello::NormalizedCoord>,
}

impl fmt::Debug for Font {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Font")
            .field("index", &self.index)
            .field("family_name", &self.family_name)
            .field("post_script_name", &self.post_script_name)
            .field("synthesis", &self.synthesis)
            .finish_non_exhaustive()
    }
}

impl Font {
    fn from_query(font: QueryFont, family_name: String) -> Result<Self> {
        let variations = font
            .synthesis
            .variation_settings()
            .iter()
            .map(|(tag, value)| Variation {
                tag: harfrust::Tag::new(&tag.to_be_bytes()),
                value: *value,
            })
            .collect::<Vec<_>>();
        let harfrust_font = harfrust::FontRef::from_index(font.blob.as_ref(), font.index)
            .context("failed to create HarfRust font reference")?;
        let shaper_data = Arc::new(ShaperData::new(&harfrust_font));
        let shaper_instance = ShaperInstance::from_variations(&harfrust_font, &variations);
        let skrifa_font = skrifa::FontRef::from_index(font.blob.as_ref(), font.index)
            .context("failed to create Skrifa font reference")?;
        let location = skrifa_font
            .axes()
            .location(variations.iter().map(|variation| {
                (
                    skrifa::Tag::new(&variation.tag.into_bytes()),
                    variation.value,
                )
            }));
        let normalized_coords = location.coords().to_vec();
        let vello_coords = normalized_coords
            .iter()
            .map(|coord| coord.to_bits())
            .collect();
        let post_script_name = skrifa_font
            .localized_strings(StringId::new(6))
            .english_or_first()
            .map(|name| name.to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| family_name.clone());

        Ok(Self {
            data: font.blob,
            index: font.index,
            family_name,
            post_script_name,
            synthesis: font.synthesis,
            shaper_data,
            shaper_instance,
            normalized_coords,
            vello_coords,
        })
    }

    pub(crate) fn vello_data(&self) -> vello::peniko::FontData {
        vello::peniko::FontData::new(self.data.clone(), self.index)
    }

    pub(crate) fn normalized_coords(&self) -> &[vello::NormalizedCoord] {
        &self.vello_coords
    }

    pub(crate) fn location(&self) -> LocationRef<'_> {
        LocationRef::new(&self.normalized_coords)
    }

    pub(crate) fn shaper_instance(&self) -> &ShaperInstance {
        &self.shaper_instance
    }

    pub(crate) fn shaper_data(&self) -> &ShaperData {
        &self.shaper_data
    }

    pub(crate) fn synthetic_bold(&self) -> bool {
        self.synthesis.embolden()
    }

    pub(crate) fn synthetic_skew(&self) -> Option<f32> {
        self.synthesis.skew()
    }

    pub(crate) fn skrifa_ref(&self) -> Result<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(self.data.as_ref(), self.index)
            .context("failed to create Skrifa font reference")
    }

    pub(crate) fn harfrust_ref(&self) -> Result<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(self.data.as_ref(), self.index)
            .context("failed to create HarfRust font reference")
    }

    pub fn has_glyph(&self, character: char) -> bool {
        self.skrifa_ref()
            .is_ok_and(|font| font.charmap().map(character).is_some())
    }

    pub(crate) fn covers(&self, text: &str) -> bool {
        self.skrifa_ref().is_ok_and(|font| {
            let charmap = font.charmap();
            text.chars()
                .filter(|character| {
                    !character.is_control()
                        && !character.is_whitespace()
                        && !is_default_ignorable(*character)
                })
                .all(|character| charmap.map(character).is_some())
        })
    }

    pub fn family_name(&self) -> &str {
        &self.family_name
    }

    pub fn post_script_name(&self) -> &str {
        &self.post_script_name
    }
}

fn is_default_ignorable(character: char) -> bool {
    matches!(
        character as u32,
        0x200C | 0x200D | 0xFE00..=0xFE0F | 0xE0020..=0xE007F | 0xE0100..=0xE01EF
    )
}

pub(crate) fn font_key(font: &Font) -> usize {
    font as *const Font as usize
}

#[derive(Clone, Debug)]
pub(crate) struct FontFace {
    pub(crate) family_name: String,
    pub(crate) post_script_name: String,
}

/// Font discovery, matching, fallback, and source caching.
pub struct FontSystem {
    collection: Collection,
    sources: SourceCache,
    system_families: Vec<String>,
    system_faces: Option<Vec<FontFace>>,
    registered: HashMap<String, Vec<String>>,
    resolved: HashMap<ResolveKey, Vec<Font>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ResolveKey {
    families: Vec<String>,
    width: u32,
    weight: u32,
    style: (u8, u32),
    scripts: Vec<[u8; 4]>,
    language: Option<String>,
}

impl FontSystem {
    #[must_use]
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions::default());
        let system_families = collection.family_names().map(str::to_owned).collect();
        Self {
            collection,
            sources: SourceCache::new_shared(),
            system_families,
            system_faces: None,
            registered: HashMap::new(),
            resolved: HashMap::new(),
        }
    }

    /// Registers caller-provided font data exactly once for the supplied stable key.
    pub fn register(&mut self, key: &str, data: Vec<u8>) -> Result<Vec<String>> {
        if let Some(families) = self.registered.get(key) {
            return Ok(families.clone());
        }
        let added = self.collection.register_fonts(Blob::from(data), None);
        if added.is_empty() {
            bail!("font data for {key:?} contained no usable faces");
        }
        let families = added
            .into_iter()
            .filter_map(|(id, _)| self.collection.family_name(id).map(str::to_owned))
            .collect::<Vec<_>>();
        self.registered.insert(key.to_owned(), families.clone());
        self.resolved.clear();
        Ok(families)
    }

    /// Resolves an ordered font chain for the requested families, attributes, and scripts.
    pub fn resolve(
        &mut self,
        families: &[String],
        attributes: Attributes,
        scripts: &[Script],
        language: Option<&str>,
    ) -> Result<Vec<Font>> {
        let language = language.and_then(|tag| Language::parse(tag).ok());
        let scripts = if scripts.is_empty() {
            &[Script::from_bytes(*b"Latn")][..]
        } else {
            scripts
        };
        let style = match attributes.style {
            FontStyle::Normal => (0, 0),
            FontStyle::Italic => (1, 0),
            FontStyle::Oblique(angle) => (2, angle.unwrap_or(14.0).to_bits()),
        };
        let key = ResolveKey {
            families: families.to_vec(),
            width: attributes.width.percentage().to_bits(),
            weight: attributes.weight.value().to_bits(),
            style,
            scripts: scripts.iter().map(|script| script.to_bytes()).collect(),
            language: language
                .as_ref()
                .map(|language| language.as_str().to_owned()),
        };
        if let Some(fonts) = self.resolved.get(&key) {
            return Ok(fonts.clone());
        }
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();

        for &script in scripts {
            let mut candidates = Vec::new();
            {
                let mut query = self.collection.query(&mut self.sources);
                if families.is_empty() {
                    query.set_families([QueryFamily::Generic(GenericFamily::SansSerif)]);
                } else {
                    query.set_families(
                        families
                            .iter()
                            .map(|family| QueryFamily::Named(family.as_str())),
                    );
                }
                query.set_attributes(attributes);
                query.set_fallbacks(FallbackKey::new(script, language.as_ref()));
                query.matches_with(|font| {
                    candidates.push(font.clone());
                    QueryStatus::Continue
                });
            }

            for candidate in candidates {
                if !seen.insert((candidate.blob.id(), candidate.index)) {
                    continue;
                }
                let family_name = self
                    .collection
                    .family_name(candidate.family.0)
                    .unwrap_or("Unknown")
                    .to_owned();
                resolved.push(Font::from_query(candidate, family_name)?);
            }
        }

        if resolved.is_empty() && !families.is_empty() {
            resolved = self.resolve(
                &[],
                attributes,
                scripts,
                language.as_ref().map(Language::as_str),
            )?;
        }
        if resolved.is_empty() {
            bail!("no usable fonts are installed");
        }
        if self.resolved.len() >= 256 {
            self.resolved.clear();
        }
        self.resolved.insert(key, resolved.clone());
        Ok(resolved)
    }

    pub fn query_family(&mut self, family: &str) -> Result<Font> {
        self.resolve(
            &[family.to_owned()],
            Attributes::default(),
            &[Script::from_bytes(*b"Latn")],
            None,
        )?
        .into_iter()
        .find(|font| font.family_name().eq_ignore_ascii_case(family))
        .with_context(|| format!("no font found for family {family:?}"))
    }

    pub fn first_font(&mut self) -> Result<Font> {
        self.resolve(
            &[],
            Attributes::default(),
            &[Script::from_bytes(*b"Latn")],
            None,
        )?
        .into_iter()
        .next()
        .context("no usable fonts are installed")
    }

    pub(crate) fn system_faces(&mut self) -> Vec<FontFace> {
        if let Some(faces) = &self.system_faces {
            return faces.clone();
        }
        let mut faces = Vec::new();
        for family_name in self.system_families.clone() {
            let Some(family) = self.collection.family_by_name(&family_name) else {
                continue;
            };
            for info in family.fonts() {
                let Some(blob) = info.load(Some(&mut self.sources)) else {
                    continue;
                };
                let query = QueryFont {
                    family: (family.id(), 0),
                    blob,
                    index: info.index(),
                    synthesis: fontique::Synthesis::default(),
                    charmap_index: info.charmap_index(),
                };
                let Ok(font) = Font::from_query(query, family_name.clone()) else {
                    continue;
                };
                faces.push(FontFace {
                    family_name: family_name.clone(),
                    post_script_name: font.post_script_name,
                });
            }
        }
        self.system_faces = Some(faces.clone());
        faces
    }
}

impl Default for FontSystem {
    fn default() -> Self {
        Self::new()
    }
}
