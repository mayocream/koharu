use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Started,
    Downloading,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub filename: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PipelineStep {
    Detect,
    Ocr,
    Inpaint,
    LlmGenerate,
    Render,
}

impl PipelineStep {
    pub const ALL: &[PipelineStep] = &[
        PipelineStep::Detect,
        PipelineStep::Ocr,
        PipelineStep::Inpaint,
        PipelineStep::LlmGenerate,
        PipelineStep::Render,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PipelineStatus {
    Running,
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineProgress {
    pub status: PipelineStatus,
    pub step: Option<PipelineStep>,
    pub current_document: usize,
    pub total_documents: usize,
    pub current_step_index: usize,
    pub total_steps: usize,
    pub overall_percent: u8,
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::*;

    fn round_trip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned,
    {
        let encoded = serde_json::to_vec(value).expect("serialize");
        let decoded: T = serde_json::from_slice(&encoded).expect("deserialize");
        let original = serde_json::to_value(value).expect("serialize to value");
        let restored = serde_json::to_value(decoded).expect("serialize decoded to value");
        assert_eq!(original, restored);
    }

    #[test]
    fn event_dtos_round_trip() {
        round_trip(&DownloadProgress {
            filename: "model.bin".to_string(),
            downloaded: 123,
            total: Some(456),
            status: DownloadStatus::Downloading,
        });
        round_trip(&DownloadProgress {
            filename: "model.bin".to_string(),
            downloaded: 123,
            total: Some(456),
            status: DownloadStatus::Failed("network".to_string()),
        });
        round_trip(&PipelineProgress {
            status: PipelineStatus::Running,
            step: Some(PipelineStep::Inpaint),
            current_document: 1,
            total_documents: 3,
            current_step_index: 2,
            total_steps: 5,
            overall_percent: 40,
        });
        round_trip(&PipelineProgress {
            status: PipelineStatus::Failed("boom".to_string()),
            step: Some(PipelineStep::Render),
            current_document: 2,
            total_documents: 3,
            current_step_index: 4,
            total_steps: 5,
            overall_percent: 90,
        });
    }
}
