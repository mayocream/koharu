use std::{borrow::Cow, sync::Arc};

use crate::server::http;

struct EmptyAssets;

impl<R: tauri::Runtime> tauri::Assets<R> for EmptyAssets {
    fn get(&self, _key: &tauri::utils::assets::AssetKey) -> Option<Cow<'_, [u8]>> {
        None
    }

    fn iter(&self) -> Box<tauri::utils::assets::AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }

    fn csp_hashes(
        &self,
        _html_path: &tauri::utils::assets::AssetKey,
    ) -> Box<dyn Iterator<Item = tauri::utils::assets::CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

struct SharedAssets<R: tauri::Runtime>(Arc<dyn tauri::Assets<R>>);

impl<R: tauri::Runtime> tauri::Assets<R> for SharedAssets<R> {
    fn get(&self, key: &tauri::utils::assets::AssetKey) -> Option<Cow<'_, [u8]>> {
        self.0.get(key)
    }

    fn iter(&self) -> Box<tauri::utils::assets::AssetsIter<'_>> {
        self.0.iter()
    }

    fn csp_hashes(
        &self,
        html_path: &tauri::utils::assets::AssetKey,
    ) -> Box<dyn Iterator<Item = tauri::utils::assets::CspHash<'_>> + '_> {
        self.0.csp_hashes(html_path)
    }
}

pub(crate) fn share_context_assets<R: tauri::Runtime>(
    context: &mut tauri::Context<R>,
) -> Arc<dyn tauri::Assets<R>> {
    let assets: Arc<dyn tauri::Assets<R>> = context.set_assets(Box::new(EmptyAssets)).into();
    context.set_assets(Box::new(SharedAssets(assets.clone())));
    assets
}

fn resolve_embedded_asset<R: tauri::Runtime>(
    assets: &dyn tauri::Assets<R>,
    path: &str,
) -> Option<http::Asset> {
    let path = path.trim_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    let candidates = [
        path.to_string(),
        format!("{path}.html"),
        format!("{path}/index.html"),
    ];

    candidates.into_iter().find_map(|path| {
        let key = tauri::utils::assets::AssetKey::from(path.as_str());
        let bytes = assets.get(&key)?.into_owned();
        Some(http::Asset {
            mime_type: tauri::utils::mime_type::MimeType::parse(&bytes, &path),
            bytes,
        })
    })
}

pub(crate) fn embedded_asset_resolver<R: tauri::Runtime>(
    assets: Arc<dyn tauri::Assets<R>>,
) -> http::SharedAssetResolver {
    Arc::new(move |path: &str| resolve_embedded_asset(assets.as_ref(), path))
}

pub(crate) fn tauri_asset_resolver<R: tauri::Runtime>(
    resolver: tauri::AssetResolver<R>,
) -> http::SharedAssetResolver {
    let resolver = Arc::new(resolver);
    Arc::new(move |path: &str| {
        let asset = resolver.get(path.to_string())?;
        Some(http::Asset {
            bytes: asset.bytes.to_vec(),
            mime_type: asset.mime_type.clone(),
        })
    })
}
