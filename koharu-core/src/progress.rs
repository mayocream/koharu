use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;

static PROGRESS_BAR: Lazy<MultiProgress> = Lazy::new(MultiProgress::new);

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
