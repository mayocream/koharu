use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::inventory::ManagedDownloadFile;

pub(crate) const SNAPSHOT_BROADCAST_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) const LAGGED_LOG_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Default)]
pub(crate) struct FileProgress {
    pub(crate) filename: String,
    pub(crate) downloaded: u64,
    pub(crate) total: Option<u64>,
    pub(crate) state: FileProgressState,
    pub(crate) error: Option<String>,
    pub(crate) last_update_seq: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FileProgressState {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Default)]
pub(crate) struct BroadcastState {
    pub(crate) last_emitted_at: Option<Instant>,
    pub(crate) scheduled: bool,
}

#[derive(Default)]
pub(crate) struct LaggedState {
    pub(crate) skipped_since_log: u64,
    pub(crate) last_logged_at: Option<Instant>,
}

pub(crate) fn planned_filename(file_plan: &[ManagedDownloadFile], file_id: &str) -> Option<String> {
    file_plan
        .iter()
        .find(|file| file.id == file_id)
        .map(|file| file.filename.clone())
}

pub(crate) fn select_focus_file_id(
    file_order: &[String],
    files: &HashMap<String, FileProgress>,
    preferred_file_id: Option<String>,
) -> Option<String> {
    if let Some(file_id) = preferred_file_id
        .as_ref()
        .filter(|file_id| {
            files
                .get(file_id.as_str())
                .is_some_and(|file| file.state == FileProgressState::Running)
        })
        .cloned()
    {
        return Some(file_id);
    }

    if let Some((file_id, _)) = files
        .iter()
        .filter(|(_, file)| file.state == FileProgressState::Running)
        .max_by_key(|(_, file)| file.last_update_seq)
    {
        return Some(file_id.clone());
    }

    if let Some((file_id, _)) = file_order
        .iter()
        .filter_map(|file_id| files.get(file_id).map(|file| (file_id, file)))
        .filter(|(_, file)| {
            matches!(
                file.state,
                FileProgressState::Pending | FileProgressState::Running | FileProgressState::Failed
            )
        })
        .max_by_key(|(_, file)| file.last_update_seq)
    {
        return Some(file_id.clone());
    }

    if !file_order.is_empty() {
        for file_id in file_order {
            let state = files
                .get(file_id)
                .map(|file| file.state)
                .unwrap_or(FileProgressState::Pending);
            if state != FileProgressState::Completed {
                return Some(file_id.clone());
            }
        }
        return file_order.last().cloned();
    }

    files
        .iter()
        .max_by_key(|(_, file)| file.last_update_seq)
        .map(|(file_id, _)| file_id.clone())
        .or_else(|| preferred_file_id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{FileProgress, FileProgressState, planned_filename, select_focus_file_id};
    use crate::inventory::ManagedDownloadFile;

    fn file_progress(
        filename: &str,
        state: FileProgressState,
        last_update_seq: u64,
    ) -> FileProgress {
        FileProgress {
            filename: filename.to_string(),
            downloaded: 0,
            total: None,
            state,
            error: None,
            last_update_seq,
        }
    }

    #[test]
    fn planned_filename_uses_unique_file_key() {
        let file_plan = vec![ManagedDownloadFile {
            id: "hf:owner/repo:config.json".to_string(),
            filename: "config.json".to_string(),
        }];
        assert_eq!(
            planned_filename(&file_plan, "hf:owner/repo:config.json").as_deref(),
            Some("config.json")
        );
    }

    #[test]
    fn select_focus_file_id_prefers_latest_running_file() {
        let file_order = vec![
            "hf:owner/repo:config.json".to_string(),
            "hf:owner/repo:model.safetensors".to_string(),
        ];
        let mut files = HashMap::new();
        files.insert(
            file_order[0].clone(),
            file_progress("config.json", FileProgressState::Running, 1),
        );
        files.insert(
            file_order[1].clone(),
            file_progress("model.safetensors", FileProgressState::Running, 5),
        );

        assert_eq!(
            select_focus_file_id(&file_order, &files, None).as_deref(),
            Some("hf:owner/repo:model.safetensors")
        );
    }

    #[test]
    fn select_focus_file_id_falls_back_to_first_planned_incomplete_file() {
        let file_order = vec![
            "hf:owner/repo:config.json".to_string(),
            "hf:owner/repo:model.safetensors".to_string(),
        ];
        let files = HashMap::new();

        assert_eq!(
            select_focus_file_id(&file_order, &files, None).as_deref(),
            Some("hf:owner/repo:config.json")
        );
    }
}
