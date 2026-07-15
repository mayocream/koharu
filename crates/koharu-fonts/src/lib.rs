use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use koharu_runtime::{download::client::Client, package::STORE_DIR};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::debug;
use utoipa::ToSchema;

const CATALOG_JSON: &str = include_str!("../data/google-fonts-catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFontVariant {
    pub style: String,
    pub weight: u16,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFontEntry {
    pub family: String,
    pub category: String,
    pub subsets: Vec<String>,
    pub variants: Vec<GoogleFontVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoogleFontCatalog {
    pub fonts: Vec<GoogleFontEntry>,
}

/// On-demand Google Fonts service with persistent disk caching.
pub struct GoogleFonts {
    catalog: GoogleFontCatalog,
    cache_dir: PathBuf,
    http: Client,
    /// Tracks which families have been downloaded to disk.
    cached_families: Mutex<HashMap<String, Vec<PathBuf>>>,
}

impl GoogleFonts {
    pub fn new() -> Result<Self> {
        let catalog: GoogleFontCatalog =
            serde_json::from_str(CATALOG_JSON).context("failed to parse Google Fonts catalog")?;
        let cache_dir = STORE_DIR.join("fonts").join("google");
        std::fs::create_dir_all(&cache_dir).context("failed to create Google Fonts cache dir")?;
        let http = Client::new().context("failed to create Google Fonts HTTP client")?;

        // Scan existing cache to populate known cached families
        let mut cached_families = HashMap::new();
        for entry in &catalog.fonts {
            let family_dir = cache_dir.join(normalize_family_dir(&entry.family));
            if family_dir.exists() {
                let paths: Vec<PathBuf> = entry
                    .variants
                    .iter()
                    .map(|v| family_dir.join(&v.filename))
                    .filter(|p| p.exists())
                    .collect();
                if !paths.is_empty() {
                    cached_families.insert(entry.family.clone(), paths);
                }
            }
        }

        Ok(Self {
            catalog,
            cache_dir,
            http,
            cached_families: Mutex::new(cached_families),
        })
    }

    /// Returns the full catalog for browsing.
    pub fn catalog(&self) -> &GoogleFontCatalog {
        &self.catalog
    }

    /// Checks if a family has been cached to disk.
    pub async fn is_cached(&self, family: &str) -> bool {
        self.cached_families.lock().await.contains_key(family)
    }

    /// Checks if a specific variant has been cached to disk.
    pub fn is_variant_cached(&self, family: &str, variant: &GoogleFontVariant) -> bool {
        let family_dir = self.cache_dir.join(normalize_family_dir(family));
        family_dir.join(&variant.filename).exists()
    }

    /// Downloads a font family's regular variant to disk cache.
    /// Returns the path to the cached .ttf file.
    /// No-op if already cached.
    pub async fn fetch_family(&self, family: &str) -> Result<PathBuf> {
        self.fetch_variant(family, 400, "normal").await
    }

    /// Downloads a specific variant to disk cache.
    pub async fn fetch_variant(&self, family: &str, weight: u16, style: &str) -> Result<PathBuf> {
        let entry = self
            .catalog
            .fonts
            .iter()
            .find(|e| e.family == family)
            .with_context(|| format!("font family not found in catalog: {family}"))?;

        let variant = entry
            .variants
            .iter()
            .find(|v| v.weight == weight && v.style == style)
            .or_else(|| {
                // Fallback to regular if requested variant not found
                entry
                    .variants
                    .iter()
                    .find(|v| v.weight == 400 && v.style == "normal")
            })
            .or_else(|| entry.variants.first())
            .context("font has no variants")?;

        let family_dir_name = normalize_family_dir(&entry.family);
        let file_path = self
            .cache_dir
            .join(&family_dir_name)
            .join(&variant.filename);

        // Check cache first
        if file_path.exists() {
            return Ok(file_path);
        }

        // Try different license categories on Google Fonts GitHub
        let categories = ["ofl", "apache", "ufl"];
        let mut last_error = None;

        for category in categories {
            let url = format!(
                "https://raw.githubusercontent.com/google/fonts/main/{}/{}/{}",
                category, family_dir_name, variant.filename
            );

            debug!(%family, %url, "trying to download Google Font");
            match self.http.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let bytes = resp.bytes().await.context("failed to read font bytes")?;
                    std::fs::create_dir_all(file_path.parent().unwrap())?;
                    std::fs::write(&file_path, &bytes)?;

                    // Update in-memory cache tracking
                    let mut cached = self.cached_families.lock().await;
                    let entries = cached.entry(family.to_string()).or_default();
                    if !entries.contains(&file_path) {
                        entries.push(file_path.clone());
                    }

                    return Ok(file_path);
                }
                Ok(resp) if resp.status() == 404 => {
                    // If exact filename failed, it might be a naming mismatch on the CDN
                    // This is rare for the main repo but happens with some older fonts
                    last_error = Some(anyhow::anyhow!(
                        "Font file {} not found in {}",
                        variant.filename,
                        category
                    ));
                    continue;
                }
                Ok(resp) => {
                    last_error = Some(anyhow::anyhow!("CDN returned {}", resp.status()));
                }
                Err(e) => {
                    last_error = Some(e.into());
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Font not found in any known category")))
    }

    /// Reads the cached font file bytes. Returns None if not cached.
    pub fn read_cached_file(&self, family: &str) -> Result<Option<Vec<u8>>> {
        self.read_cached_variant(family, 400, "normal")
    }

    /// Reads a specific cached variant.
    pub fn read_cached_variant(
        &self,
        family: &str,
        weight: u16,
        style: &str,
    ) -> Result<Option<Vec<u8>>> {
        let entry = self.catalog.fonts.iter().find(|e| e.family == family);
        let Some(entry) = entry else {
            return Ok(None);
        };
        let variant = entry
            .variants
            .iter()
            .find(|v| v.weight == weight && v.style == style);

        let Some(variant) = variant else {
            // If the specific variant isn't in the catalog, we can't load it
            return Ok(None);
        };
        let file_path = self
            .cache_dir
            .join(normalize_family_dir(&entry.family))
            .join(&variant.filename);
        if !file_path.exists() {
            return Ok(None);
        }
        let data = std::fs::read(file_path).context("failed to read cached font")?;
        Ok(Some(data))
    }

    /// Find catalog entry by family name.
    pub fn find_entry(&self, family: &str) -> Option<&GoogleFontEntry> {
        self.catalog.fonts.iter().find(|e| e.family == family)
    }
}

/// Converts family name to directory name (lowercase, spaces to empty).
/// e.g. "Comic Neue" -> "comicneue"
fn normalize_family_dir(family: &str) -> String {
    family.to_lowercase().replace(' ', "")
}

/// Parses a variant query string like "Family:700i" into (family, weight, style).
pub fn parse_variant_query(query: &str) -> (&str, u16, &str) {
    if let Some((family, variant_str)) = query.split_once(':') {
        let weight = variant_str
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u16>()
            .unwrap_or(400);
        let style = if variant_str.contains('i') {
            "italic"
        } else {
            "normal"
        };
        (family, weight, style)
    } else {
        (query, 400, "normal")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_not_empty() {
        let catalog: GoogleFontCatalog = serde_json::from_str(CATALOG_JSON).unwrap();

        assert!(!catalog.fonts.is_empty());
    }

    #[test]
    fn family_names_map_to_google_fonts_directories() {
        assert_eq!(normalize_family_dir("Comic Neue"), "comicneue");
        assert_eq!(normalize_family_dir("Noto Sans JP"), "notosansjp");
    }

    #[test]
    fn cache_uses_runtime_store() {
        let fonts = GoogleFonts::new().unwrap();

        assert_eq!(fonts.cache_dir, STORE_DIR.join("fonts").join("google"));
    }

    #[test]
    fn variant_queries_parse_weight_and_style() {
        assert_eq!(
            parse_variant_query("Comic Neue:700i"),
            ("Comic Neue", 700, "italic")
        );
        assert_eq!(
            parse_variant_query("Comic Neue"),
            ("Comic Neue", 400, "normal")
        );
        assert_eq!(
            parse_variant_query("Comic Neue:bad"),
            ("Comic Neue", 400, "normal")
        );
    }
}
