use crate::AppResources;
use koharu_core::ExportProjectResult;
use anyhow::Result;
use rfd::FileDialog;

pub async fn export_project(state: AppResources) -> Result<ExportProjectResult> {
    let Some(output_path) = pick_output_khr(&state).await? else {
        return Ok(ExportProjectResult {
            success: false,
            filename: String::new(),
        });
    };

    let filename = output_path.file_name().unwrap_or_default().to_string_lossy().to_string();

    state.storage.export_khr(&output_path).await?;

    Ok(ExportProjectResult {
        success: true,
        filename,
    })
}

async fn pick_output_khr(state: &AppResources) -> Result<Option<std::path::PathBuf>> {
    let project_name = state.storage.with_project(|p| p.name.clone()).await;

    Ok(tokio::task::spawn_blocking(move || {
        FileDialog::new()
            .set_file_name(&format!("{}.khr", project_name))
            .add_filter("Koharu Project", &["khr"])
            .save_file()
    })
    .await?)
}
