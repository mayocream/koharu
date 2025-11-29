use std::path::Path;

use anyhow::{Result, anyhow};
use tokenizers::{AddedToken, Tokenizer, models::wordpiece::WordPiece};

use super::load_json;

pub fn load_tokenizer(
    tokenizer_json: Option<&Path>,
    vocab_path: &Path,
    special_tokens_path: &Path,
) -> Result<Tokenizer> {
    if let Some(path) = tokenizer_json {
        if path.exists() {
            return Tokenizer::from_file(path).map_err(|e| anyhow!(e));
        }
    }

    let model = WordPiece::from_file(vocab_path.to_string_lossy().as_ref())
        .unk_token("[UNK]".to_string())
        .build()
        .map_err(|e| anyhow!(e))?;
    let mut tokenizer = Tokenizer::new(model);

    let specials: serde_json::Value = load_json(special_tokens_path)?;
    let mut added = Vec::new();
    if let Some(obj) = specials.as_object() {
        for value in obj.values() {
            if let Some(token) = value.as_str() {
                added.push(AddedToken::from(token.to_string(), true));
            }
        }
    }
    if !added.is_empty() {
        tokenizer.add_special_tokens(&added);
    }

    Ok(tokenizer)
}
