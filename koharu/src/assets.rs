//! Extract Tauri's embedded assets and expose them as an `AssetResolver` for
//! the axum server to fall back on (i.e. serve the bundled UI).

use std::sync::Arc;

use koharu_rpc::server::AssetResolver;

/// Build an `AssetResolver` from the Tauri context. Replaces the context's
/// embedded assets with an empty set so Tauri's own webview points at the
/// axum-served URL instead.
pub fn from_context<R: tauri::Runtime>(context: &mut tauri::Context<R>) -> AssetResolver {
    struct Empty;
    impl<R: tauri::Runtime> tauri::Assets<R> for Empty {
        fn get(&self, _: &tauri::utils::assets::AssetKey) -> Option<std::borrow::Cow<'_, [u8]>> {
            None
        }
        fn iter(&self) -> Box<tauri::utils::assets::AssetsIter<'_>> {
            Box::new(std::iter::empty())
        }
        fn csp_hashes(
            &self,
            _: &tauri::utils::assets::AssetKey,
        ) -> Box<dyn Iterator<Item = tauri::utils::assets::CspHash<'_>> + '_> {
            Box::new(std::iter::empty())
        }
    }

    let assets: Arc<dyn tauri::Assets<R>> = context.set_assets(Box::new(Empty)).into();

    Arc::new(move |path: &str| {
        let key = tauri::utils::assets::AssetKey::from(path);
        let bytes = assets.get(&key)?.into_owned();
        let mime = tauri::utils::mime_type::MimeType::parse(&bytes, path);
        Some((bytes, mime))
    })
}
