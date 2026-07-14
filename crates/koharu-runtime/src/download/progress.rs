use std::{io::IsTerminal, sync::LazyLock, time::Duration};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

static DOWNLOADS: LazyLock<MultiProgress> =
    LazyLock::new(|| MultiProgress::with_draw_target(draw_target(std::io::stderr().is_terminal())));

fn draw_target(stderr_is_terminal: bool) -> ProgressDrawTarget {
    if stderr_is_terminal {
        // Indicatif 0.18.5 also requires TERM, which is normally unset in PowerShell.
        ProgressDrawTarget::term_like_with_hz(Box::new(console::Term::buffered_stderr()), 20)
    } else {
        ProgressDrawTarget::hidden()
    }
}
static STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template(
        "{spinner:.green} {msg:.bold} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} \
         {bytes_per_sec} ETA {eta}",
    )
    .expect("download progress template is valid")
    .progress_chars("=>-")
});

pub(super) fn new(url: &str) -> ProgressBar {
    let progress = DOWNLOADS.add(ProgressBar::new_spinner());
    progress.set_style(STYLE.clone());
    progress.set_message(name(url));
    progress.enable_steady_tick(Duration::from_millis(100));
    progress
}

fn name(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "download".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_visible_target_for_attended_terminal_without_term() {
        assert!(!draw_target(true).is_hidden());
        assert!(draw_target(false).is_hidden());
    }

    #[test]
    fn derives_display_name_from_url_path() {
        assert_eq!(
            name("https://example.com/models/model.gguf?download=true"),
            "model.gguf"
        );
        assert_eq!(name("https://example.com/"), "download");
    }
}
