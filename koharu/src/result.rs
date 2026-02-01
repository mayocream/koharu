#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    File(#[from] std::io::Error),

    #[error(transparent)]
    Parse(#[from] strum::ParseError),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),

    #[error(transparent)]
    Window(#[from] tauri::Error),

    #[error(transparent)]
    Image(#[from] image::ImageError),

    #[error(transparent)]
    Multipart(#[from] axum::extract::multipart::MultipartError),
}

impl serde::Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CommandError>;
