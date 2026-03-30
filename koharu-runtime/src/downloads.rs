use std::sync::Arc;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use koharu_core::events::{DownloadProgress, DownloadStatus};
use tokio::sync::broadcast;

#[derive(Clone)]
pub(crate) struct TransferHub {
    tx: broadcast::Sender<DownloadProgress>,
    progress: Arc<MultiProgress>,
}

impl TransferHub {
    pub(crate) fn new() -> Self {
        Self {
            tx: broadcast::channel(256).0,
            progress: Arc::new(MultiProgress::new()),
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<DownloadProgress> {
        self.tx.subscribe()
    }

    pub(crate) fn begin(&self, label: &str) -> TransferReporter {
        let bar = self.progress.add(ProgressBar::new_spinner());
        bar.enable_steady_tick(Duration::from_millis(120));
        bar.set_style(
            ProgressStyle::with_template(
                "{msg} [{elapsed_precise}] [{wide_bar}] {bytes}/{total_bytes} ({eta})",
            )
            .expect("progress style"),
        );
        bar.set_message(label.to_string());

        TransferReporter {
            tx: self.tx.clone(),
            bar,
            filename: label.to_string(),
            downloaded: 0,
            total: None,
        }
    }
}

pub(crate) struct TransferReporter {
    tx: broadcast::Sender<DownloadProgress>,
    bar: ProgressBar,
    filename: String,
    downloaded: u64,
    total: Option<u64>,
}

impl TransferReporter {
    pub(crate) fn start(&mut self, total: Option<u64>) {
        self.total = total;
        self.downloaded = 0;
        self.bar.set_length(total.unwrap_or(0));
        self.bar.set_position(0);
        self.emit(DownloadStatus::Started);
    }

    pub(crate) fn advance(&mut self, delta: usize) {
        self.downloaded += delta as u64;
        self.bar.inc(delta as u64);
        self.emit(DownloadStatus::Downloading);
    }

    pub(crate) fn finish(&mut self) {
        self.bar.finish_and_clear();
        self.emit(DownloadStatus::Completed);
    }

    pub(crate) fn fail(&mut self, error: &anyhow::Error) {
        self.bar.finish_and_clear();
        self.emit(DownloadStatus::Failed(error.to_string()));
    }

    fn emit(&self, status: DownloadStatus) {
        let _ = self.tx.send(DownloadProgress {
            filename: self.filename.clone(),
            downloaded: self.downloaded,
            total: self.total,
            status,
        });
    }
}
