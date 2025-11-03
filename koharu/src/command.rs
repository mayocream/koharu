use tauri::State;

use crate::{
    result::Result,
    state::{AppState, Document},
};

#[tauri::command]
pub fn open_external(url: &str) -> Result<()> {
    open::that(url)?;

    Ok(())
}

#[tauri::command]
pub async fn open_documents(state: State<'_, AppState>) -> Result<Vec<Document>> {
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

    // store documents in app state
    let mut state = state.write().await;
    state.documents = documents.clone();

    // return opened documents as a copy
    Ok(documents)
}
