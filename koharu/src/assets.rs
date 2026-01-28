use rust_embed::Embed;

#[derive(Embed)]
#[folder = "$CARGO_WORKSPACE_DIR/ui/out"]
#[allow_missing = true]
pub struct EmbeddedUi;

impl EmbeddedUi {
    pub fn get_with_mime(path: &str) -> Option<(Vec<u8>, String)> {
        let asset = Self::get(path)?;
        let mime = asset.metadata.mimetype().to_owned();
        Some((asset.data.into_owned(), mime))
    }
}
