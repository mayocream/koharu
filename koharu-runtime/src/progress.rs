use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;

static PROGRESS_BAR: Lazy<MultiProgress> = Lazy::new(MultiProgress::new);
const DOWNLOAD_EMIT_INTERVAL: Duration = Duration::from_millis(150);
const DOWNLOAD_EMIT_BYTES: u64 = 512 * 1024;
const DOWNLOAD_EMIT_PERCENT: u64 = 1;

pub fn progress_bar(filename: &str) -> ProgressBar {
    let pb = PROGRESS_BAR.add(indicatif::ProgressBar::new_spinner());
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_style(
        ProgressStyle::with_template(
            "{msg} [{elapsed_precise}] [{wide_bar}] {bytes}/{total_bytes} ({eta})",
        )
        .expect("set progress bar style"),
    );
    pb.set_message(filename.to_string());
    pb
}

#[derive(Debug, Clone, Default)]
pub struct DownloadEventThrottle {
    last_emit_at: Option<Instant>,
    last_downloaded: u64,
    last_percent: Option<u64>,
}

impl DownloadEventThrottle {
    pub fn mark_emitted(&mut self, downloaded: u64, total: Option<u64>) {
        self.last_emit_at = Some(Instant::now());
        self.last_downloaded = downloaded;
        self.last_percent = percent(downloaded, total);
    }

    pub fn should_emit(&mut self, downloaded: u64, total: Option<u64>) -> bool {
        let Some(last_emit_at) = self.last_emit_at else {
            self.mark_emitted(downloaded, total);
            return true;
        };

        let byte_delta = downloaded.saturating_sub(self.last_downloaded);
        let percent_delta = match (percent(downloaded, total), self.last_percent) {
            (Some(current), Some(previous)) => current.saturating_sub(previous),
            (Some(_), None) => DOWNLOAD_EMIT_PERCENT,
            _ => 0,
        };

        let should_emit = byte_delta >= DOWNLOAD_EMIT_BYTES
            || percent_delta >= DOWNLOAD_EMIT_PERCENT
            || last_emit_at.elapsed() >= DOWNLOAD_EMIT_INTERVAL;

        if should_emit {
            self.mark_emitted(downloaded, total);
        }

        should_emit
    }
}

fn percent(downloaded: u64, total: Option<u64>) -> Option<u64> {
    let total = total?;
    if total == 0 {
        return None;
    }
    Some((downloaded.saturating_mul(100) / total).min(100))
}

#[cfg(test)]
mod tests {
    use super::DownloadEventThrottle;

    #[test]
    fn throttle_emits_initial_update() {
        let mut throttle = DownloadEventThrottle::default();
        assert!(throttle.should_emit(1, Some(100)));
    }

    #[test]
    fn throttle_skips_tiny_immediate_updates() {
        let mut throttle = DownloadEventThrottle::default();
        assert!(throttle.should_emit(1, Some(1000)));
        assert!(!throttle.should_emit(2, Some(1000)));
    }

    #[test]
    fn throttle_emits_on_percent_change() {
        let mut throttle = DownloadEventThrottle::default();
        assert!(throttle.should_emit(0, Some(100)));
        assert!(throttle.should_emit(1, Some(100)));
    }
}
