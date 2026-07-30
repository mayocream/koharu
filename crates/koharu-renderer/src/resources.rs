//! Explicit, reusable font and decoded-image resources.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result as AnyResult};
use fontique::Attributes;
use koharu_scene::{Asset, BlobId, LanguageTag, SceneSnapshot};
use parking_lot::Mutex;

use crate::{
    Error, Font, FontFaceInfo, FontFallbackPolicy, FontSource, FontSystem, Result,
    script::fontique_scripts,
};

const DEFAULT_IMAGE_CACHE_BYTES: usize = 512 * 1024 * 1024;

/// Thread-safe font registry. Rendering only reads already-installed fonts.
pub struct FontManager {
    system: Mutex<FontSystem>,
    generation: AtomicU64,
    fallback_policy: FontFallbackPolicy,
}

impl FontManager {
    #[must_use]
    pub fn new() -> Self {
        Self::with_fallback_policy(FontFallbackPolicy::default())
    }

    #[must_use]
    pub fn with_fallback_policy(fallback_policy: FontFallbackPolicy) -> Self {
        Self {
            system: Mutex::new(FontSystem::new()),
            generation: AtomicU64::new(0),
            fallback_policy,
        }
    }

    #[must_use]
    pub const fn fallback_policy(&self) -> &FontFallbackPolicy {
        &self.fallback_policy
    }

    pub fn register(&self, key: &str, data: Vec<u8>) -> Result<Vec<String>> {
        let families = self
            .system
            .lock()
            .register(key, data)
            .map_err(Error::FontResource)?;
        self.generation.fetch_add(1, Ordering::Relaxed);
        Ok(families)
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    pub fn available_fonts(&self) -> Vec<FontFaceInfo> {
        let mut faces = self
            .system
            .lock()
            .system_faces()
            .into_iter()
            .map(|face| FontFaceInfo {
                family_name: face.family_name,
                post_script_name: face.post_script_name,
                weight: face.weight,
                stretch: face.stretch,
                style: face.style,
                source: FontSource::System,
            })
            .collect::<Vec<_>>();
        faces.sort();
        faces.dedup();
        faces
    }

    pub fn resolve_post_script_name(
        &self,
        preferred_font: Option<&str>,
        text: &str,
        language: Option<&LanguageTag>,
    ) -> Result<String> {
        self.resolve(preferred_font, &[], text, language.map(LanguageTag::as_str))
            .map_err(Error::FontResource)?
            .into_iter()
            .next()
            .map(|font| font.post_script_name().to_owned())
            .context("no usable fonts are installed")
            .map_err(Error::FontResource)
    }

    pub(crate) fn resolve(
        &self,
        preferred_font: Option<&str>,
        theme_families: &[String],
        text: &str,
        language: Option<&str>,
    ) -> AnyResult<Vec<Font>> {
        let mut families =
            Vec::with_capacity(theme_families.len() + usize::from(preferred_font.is_some()));
        if let Some(preferred) = preferred_font.filter(|value| !value.is_empty()) {
            families.push(preferred.to_owned());
        }
        for family in theme_families {
            if !families
                .iter()
                .any(|value| value.eq_ignore_ascii_case(family))
            {
                families.push(family.clone());
            }
        }
        self.system.lock().resolve_with_fallback_families(
            &families,
            self.fallback_policy.symbol_families(),
            Attributes::default(),
            &fontique_scripts(text),
            language,
        )
    }
}

impl Default for FontManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RenderResources {
    fonts: FontManager,
    images: Mutex<DecodedImageCache>,
}

impl RenderResources {
    #[must_use]
    pub fn new() -> Self {
        Self::with_image_cache_bytes(DEFAULT_IMAGE_CACHE_BYTES)
    }

    #[must_use]
    pub fn with_image_cache_bytes(max_bytes: usize) -> Self {
        Self::with_options(max_bytes, FontFallbackPolicy::default())
    }

    #[must_use]
    pub fn with_font_fallback_policy(fallback_policy: FontFallbackPolicy) -> Self {
        Self::with_options(DEFAULT_IMAGE_CACHE_BYTES, fallback_policy)
    }

    #[must_use]
    pub fn with_options(max_image_cache_bytes: usize, fallback_policy: FontFallbackPolicy) -> Self {
        Self {
            fonts: FontManager::with_fallback_policy(fallback_policy),
            images: Mutex::new(DecodedImageCache::new(max_image_cache_bytes)),
        }
    }

    #[must_use]
    pub const fn fonts(&self) -> &FontManager {
        &self.fonts
    }

    pub fn clear_images(&self) {
        self.images.lock().clear();
    }

    pub(crate) fn image(
        &self,
        snapshot: &SceneSnapshot,
        asset: &Asset,
    ) -> Result<Arc<DecodedImage>> {
        if let Some(image) = self.images.lock().get(asset.blob) {
            return Ok(image);
        }
        let bytes = snapshot.read_blob(asset.blob)?;
        let decoded = image::load_from_memory(&bytes)
            .map_err(|source| Error::Image {
                blob: asset.blob,
                source,
            })?
            .into_rgba8();
        if let (Some(expected_width), Some(expected_height)) =
            (asset.metadata.width, asset.metadata.height)
            && (decoded.width() != expected_width || decoded.height() != expected_height)
        {
            return Err(Error::invalid(format!(
                "blob {} decoded as {}x{}, expected {}x{}",
                asset.blob,
                decoded.width(),
                decoded.height(),
                expected_width,
                expected_height
            )));
        }
        let image = Arc::new(DecodedImage {
            width: decoded.width(),
            height: decoded.height(),
            pixels: Arc::from(decoded.into_raw()),
        });
        self.images.lock().insert(asset.blob, image.clone());
        Ok(image)
    }
}

impl Default for RenderResources {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub(crate) struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}

impl DecodedImage {
    fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

struct CacheEntry {
    image: Arc<DecodedImage>,
    last_used: u64,
}

struct DecodedImageCache {
    entries: HashMap<BlobId, CacheEntry>,
    max_bytes: usize,
    bytes: usize,
    clock: u64,
}

impl DecodedImageCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_bytes,
            bytes: 0,
            clock: 0,
        }
    }

    fn get(&mut self, id: BlobId) -> Option<Arc<DecodedImage>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&id)?;
        entry.last_used = self.clock;
        Some(entry.image.clone())
    }

    fn insert(&mut self, id: BlobId, image: Arc<DecodedImage>) {
        let image_bytes = image.byte_len();
        if self.max_bytes == 0 || image_bytes > self.max_bytes {
            return;
        }
        if let Some(previous) = self.entries.remove(&id) {
            self.bytes = self.bytes.saturating_sub(previous.image.byte_len());
        }
        while self.bytes.saturating_add(image_bytes) > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(id, _)| *id)
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(removed.image.byte_len());
            }
        }
        self.clock = self.clock.wrapping_add(1);
        self.bytes += image_bytes;
        self.entries.insert(
            id,
            CacheEntry {
                image,
                last_used: self.clock,
            },
        );
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(bytes: usize) -> Arc<DecodedImage> {
        Arc::new(DecodedImage {
            width: bytes as u32 / 4,
            height: 1,
            pixels: Arc::from(vec![0; bytes]),
        })
    }

    #[test]
    fn decoded_image_cache_is_byte_bounded_and_lru() {
        let first = BlobId::for_bytes(b"first");
        let second = BlobId::for_bytes(b"second");
        let third = BlobId::for_bytes(b"third");
        let mut cache = DecodedImageCache::new(8);
        cache.insert(first, image(4));
        cache.insert(second, image(4));
        assert!(cache.get(first).is_some());

        cache.insert(third, image(4));

        assert!(cache.get(first).is_some());
        assert!(cache.get(second).is_none());
        assert!(cache.get(third).is_some());
        assert!(cache.bytes <= cache.max_bytes);
    }

    #[test]
    fn oversized_images_are_not_cached() {
        let id = BlobId::for_bytes(b"large");
        let mut cache = DecodedImageCache::new(4);

        cache.insert(id, image(8));

        assert!(cache.get(id).is_none());
        assert_eq!(cache.bytes, 0);
    }
}
