use anyhow::{Context, Result};
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

use crate::bootstrap::{BootstrapPhase, BootstrapSnapshot};

pub(crate) fn sync_bootstrap_windows(app: &AppHandle, state: &BootstrapSnapshot) -> Result<()> {
    let main_exists = app.get_webview_window("main").is_some();

    if matches!(state.phase, BootstrapPhase::Ready) {
        show_window(app, "main")?;
        close_window(app, "onboarding");
        close_window(app, "splashscreen");
        return Ok(());
    }

    if main_exists {
        return Ok(());
    }

    let onboarding_exists = app.get_webview_window("onboarding").is_some();

    match pending_window_target(state.phase, onboarding_exists) {
        PendingWindowTarget::Onboarding => {
            show_window(app, "onboarding")?;
            close_window(app, "splashscreen");
        }
        PendingWindowTarget::Splashscreen => {
            show_window(app, "splashscreen")?;
            close_window(app, "onboarding");
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingWindowTarget {
    Onboarding,
    Splashscreen,
}

fn pending_window_target(phase: BootstrapPhase, onboarding_exists: bool) -> PendingWindowTarget {
    match phase {
        BootstrapPhase::NeedsOnboarding | BootstrapPhase::Failed => PendingWindowTarget::Onboarding,
        BootstrapPhase::Loading => {
            if onboarding_exists {
                PendingWindowTarget::Onboarding
            } else {
                PendingWindowTarget::Splashscreen
            }
        }
        BootstrapPhase::Ready => PendingWindowTarget::Splashscreen,
    }
}

fn show_window(app: &AppHandle, label: &str) -> Result<()> {
    ensure_window(app, label)?;
    if let Some(window) = app.get_webview_window(label) {
        window.show().ok();
        if label != "splashscreen" {
            window.set_focus().ok();
        }
    }
    Ok(())
}

fn ensure_window(app: &AppHandle, label: &str) -> Result<()> {
    if app.get_webview_window(label).is_some() {
        return Ok(());
    }

    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .with_context(|| format!("window config `{label}` not found"))?;

    WebviewWindowBuilder::from_config(app, config)
        .with_context(|| format!("failed to build `{label}` window"))?
        .build()
        .with_context(|| format!("failed to create `{label}` window"))?;
    Ok(())
}

fn close_window(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        window.close().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::{PendingWindowTarget, pending_window_target};
    use crate::bootstrap::BootstrapPhase;

    #[test]
    fn startup_loading_prefers_splashscreen_until_onboarding_exists() {
        assert_eq!(
            pending_window_target(BootstrapPhase::Loading, false),
            PendingWindowTarget::Splashscreen
        );
        assert_eq!(
            pending_window_target(BootstrapPhase::Loading, true),
            PendingWindowTarget::Onboarding
        );
    }

    #[test]
    fn missing_or_failed_dependencies_show_onboarding() {
        assert_eq!(
            pending_window_target(BootstrapPhase::NeedsOnboarding, false),
            PendingWindowTarget::Onboarding
        );
        assert_eq!(
            pending_window_target(BootstrapPhase::Failed, false),
            PendingWindowTarget::Onboarding
        );
    }
}
