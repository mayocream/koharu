use std::{fs, path::PathBuf};

use anyhow::{Context as _, Result};
use koharu_desktop::DesktopHandle;
use koharu_pipeline::CancellationToken;
use koharu_scene::Session;

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
        let mut session =
            Session::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        for (index, file) in files.into_iter().enumerate() {
            if cancellation.is_cancelled() {
                break;
            }
            let bytes =
                fs::read(&file).with_context(|| format!("failed to read {}", file.display()))?;
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("page")
                .to_owned();
            let mut commands = session.commands();
            let page = commands.add_page(name, bytes)?;
            let changes = session.apply(commands)?;
            pages.push(page);
            revisions.push(changes.to);
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
