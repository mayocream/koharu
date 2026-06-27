use std::path::Path;

use anyhow::{Result, anyhow};
use tokenizers::{AddedToken, Tokenizer, models::wordpiece::WordPiece};

const SPECIAL_TOKENS: [&str; 5] = ["[UNK]", "[SEP]", "[PAD]", "[CLS]", "[MASK]"];

pub fn load_tokenizer(tokenizer_json: Option<&Path>, vocab_path: &Path) -> Result<Tokenizer> {
    if let Some(path) = tokenizer_json
        && path.exists()
    {
        return Tokenizer::from_file(path).map_err(|e| anyhow!(e));
    }

    let model = WordPiece::from_file(vocab_path.to_string_lossy().as_ref())
        .unk_token("[UNK]".to_string())
        .build()
        .map_err(|e| anyhow!(e))?;
    let mut tokenizer = Tokenizer::new(model);

    tokenizer
        .add_special_tokens(
            SPECIAL_TOKENS
                .iter()
                .map(|token| AddedToken::from((*token).to_string(), true))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| anyhow!(e))?;

    Ok(tokenizer)
}
