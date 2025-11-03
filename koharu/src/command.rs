use crate::{result::Result, state::Document};

#[tauri::command]
pub fn open_external(url: &str) -> Result<()> {
    open::that(url)?;

    Ok(())
}

#[tauri::command]
pub fn open_documents() -> Result<Vec<Document>> {
    let paths = rfd::FileDialog::new()
        .add_filter("Image Files", &["png", "jpg", "jpeg", "webp"])
        .add_filter("Koharu Document", &["khr"])
        .set_title("Pick Files")
        .pick_files();

    let mut documents = Vec::new();

    if let Some(paths) = paths {
        for path in paths {
            let doc = Document::open(path)?;
            documents.push(doc);
        }
    }

    Ok(documents)
}
