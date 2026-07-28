use std::{fs, path::PathBuf, sync::Arc};

use anyhow::{Context as _, Result};
use koharu_desktop::DesktopHandle;
use koharu_pipeline::CancellationToken;
use koharu_scene::{AssetInput, AssetMetadata, AssetRole, At, PageDraft, SceneSession};

use super::{JobOutcome, NativeEvent, finish_job};
use crate::protocol::RequestId;

pub(super) fn run(
    id: RequestId,
    path: PathBuf,
    files: Vec<PathBuf>,
    cancellation: CancellationToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let total = files.len();
    let mut revisions = Vec::new();
    let mut pages = Vec::new();
    let result = (|| -> Result<()> {
        let mut session = SceneSession::open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        for (index, file) in files.into_iter().enumerate() {
            if cancellation.is_cancelled() {
                break;
            }
            let bytes =
                fs::read(&file).with_context(|| format!("failed to read {}", file.display()))?;
            let format = image::guess_format(&bytes)
                .with_context(|| format!("failed to identify {}", file.display()))?;
            let image = image::load_from_memory_with_format(&bytes, format)
                .with_context(|| format!("failed to decode {}", file.display()))?;
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("page")
                .to_owned();
            let mut page = None;
            let patch = session.snapshot().patch(|edit| {
                let id = edit.add_page(
                    PageDraft::new(name, f64::from(image.width()), f64::from(image.height())),
                    At::End,
                )?;
                edit.set_asset(
                    id,
                    &AssetRole::new("source")?,
                    AssetInput::new(
                        Arc::<[u8]>::from(bytes),
                        media_type(format),
                        AssetMetadata {
                            width: Some(image.width()),
                            height: Some(image.height()),
                            attributes: Default::default(),
                        },
                    ),
                )?;
                page = Some(id);
                Ok(())
            })?;
            let commit = session.commit(patch)?;
            let page = page.expect("import page ID was captured");
            pages.push(page);
            revisions.push(commit.revision);
            let _ = desktop.send_event(NativeEvent::ProjectAdvanced { job: id });
            let _ = desktop.send_event(NativeEvent::ImportProgress {
                job: id,
                completed: index + 1,
                total,
            });
        }
        Ok(())
    })();
    finish_job(
        &desktop,
        id,
        &cancellation,
        JobOutcome {
            revisions,
            pages,
            error: result.err().map(|error| error.to_string()),
        },
    );
}

fn media_type(format: image::ImageFormat) -> &'static str {
    match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::Tiff => "image/tiff",
        image::ImageFormat::Bmp => "image/bmp",
        image::ImageFormat::Ico => "image/x-icon",
        image::ImageFormat::Avif => "image/avif",
        _ => "application/octet-stream",
    }
}
