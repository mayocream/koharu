//! In-process CEF 151 windowless browser.
//!
//! CEF's browser process shares the winit thread and advances through the
//! external message pump. Renderer/helper processes still dispatch through
//! [`dispatch_cef_process`] before the application initializes.

mod internal;
mod resource;

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use crate::{
    browser::{BrowserFrame, BrowserFrameMailbox, BrowserGpu, InputEvent, WebMessage},
    geometry::WindowMetrics,
};

use internal::{BrowserContext, BrowserDelegate};
use resource::ResourceRoot;

#[derive(Clone, Debug)]
pub struct CefConfig {
    pub distribution: PathBuf,
    pub cache_root: PathBuf,
    pub initial_url: Box<str>,
    pub resources_root: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub enum CefEvent {
    Ready,
    FrameAvailable,
    AcceleratedPaintFailed(Box<str>),
    Cursor(BrowserCursor),
    WebMessage(WebMessage),
    InitializationFailed(Box<str>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserCursor {
    Hidden,
    Icon(winit::window::CursorIcon),
}

pub type CefEventSender = Arc<dyn Fn(CefEvent) + Send + Sync>;

pub struct CefBrowser {
    _app: cef::App,
    config: CefConfig,
    events: CefEventSender,
    resources: Option<ResourceRoot>,
    development_origin: Option<String>,
    frames: BrowserFrameMailbox,
    gpu: Option<BrowserGpu>,
    accelerated: bool,
    context: Option<BrowserContext>,
    delegate: Option<BrowserDelegate>,
    initialized: bool,
}

impl CefBrowser {
    pub fn initialize(config: CefConfig, events: CefEventSender) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        internal::load_macos_framework(false)?;

        let executable = std::env::current_exe()
            .map_err(|error| format!("failed to resolve CEF subprocess executable: {error}"))?;
        #[cfg(not(target_os = "macos"))]
        let resources_dir = cef_resources_dir(&config.distribution);
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        let mut app = internal::browser_app();
        let settings = cef::Settings {
            no_sandbox: 1,
            external_message_pump: 1,
            multi_threaded_message_loop: 0,
            windowless_rendering_enabled: 1,
            #[cfg(not(target_os = "macos"))]
            browser_subprocess_path: executable.to_string_lossy().as_ref().into(),
            #[cfg(target_os = "macos")]
            main_bundle_path: macos_main_bundle(&executable)?
                .to_string_lossy()
                .as_ref()
                .into(),
            root_cache_path: config.cache_root.to_string_lossy().as_ref().into(),
            cache_path: config
                .cache_root
                .join("browser")
                .to_string_lossy()
                .as_ref()
                .into(),
            #[cfg(not(target_os = "macos"))]
            resources_dir_path: resources_dir.to_string_lossy().as_ref().into(),
            #[cfg(not(target_os = "macos"))]
            locales_dir_path: resources_dir
                .join("locales")
                .to_string_lossy()
                .as_ref()
                .into(),
            disable_signal_handlers: 1,
            background_color: 0,
            ..Default::default()
        };
        let args = cef::args::Args::new();
        if cef::initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ) != 1
        {
            return Err("cef_initialize returned false".into());
        }

        let resources = config
            .resources_root
            .as_ref()
            .cloned()
            .map(ResourceRoot::new)
            .transpose()?;
        let development_origin = development_origin(&config.initial_url);
        let frame_events = Arc::clone(&events);
        let frames = BrowserFrameMailbox::new(Arc::new(move || {
            frame_events(CefEvent::FrameAvailable);
        }));
        Ok(Self {
            _app: app,
            config,
            events,
            resources,
            development_origin,
            frames,
            gpu: None,
            accelerated: false,
            context: None,
            delegate: None,
            initialized: true,
        })
    }

    pub(crate) fn create(
        &mut self,
        initial_view: WindowMetrics,
        gpu: BrowserGpu,
    ) -> Result<(), String> {
        if self.context.is_some() {
            return Ok(());
        }
        self.accelerated = gpu.supports_accelerated_osr();
        self.gpu = Some(gpu);
        self.create_context(initial_view)
    }

    fn create_context(&mut self, initial_view: WindowMetrics) -> Result<(), String> {
        cef::do_message_loop_work();
        let accelerated_gpu = self.accelerated.then(|| {
            self.gpu
                .as_ref()
                .expect("browser GPU is installed before context creation")
                .clone()
        });
        let delegate = BrowserDelegate::new(
            Arc::clone(&self.events),
            initial_view,
            self.resources.clone(),
            self.development_origin.clone(),
            self.frames.clone(),
            accelerated_gpu,
        );
        let context =
            BrowserContext::create(delegate.clone(), &self.config.initial_url, self.accelerated)?;
        self.context = Some(context);
        self.delegate = Some(delegate);
        tracing::info!(
            accelerated = self.accelerated,
            "created CEF off-screen browser"
        );
        Ok(())
    }

    pub(crate) fn fall_back_to_software(&mut self, view: WindowMetrics) -> Result<(), String> {
        if !self.accelerated {
            return Ok(());
        }
        self.accelerated = false;
        if let Some(context) = self.context.take() {
            context.close();
        }
        self.delegate.take();
        cef::do_message_loop_work();
        self.create_context(view)
    }

    pub fn pump(&self) {
        if self.initialized {
            cef::do_message_loop_work();
        }
    }

    pub fn resize(&self, metrics: WindowMetrics) {
        if let (Some(context), Some(delegate)) = (&self.context, &self.delegate) {
            context.resize(delegate, metrics);
        }
    }

    pub fn apply_input(&self, event: &InputEvent) {
        if let Some(context) = &self.context {
            context.apply_input(std::slice::from_ref(event));
        }
    }

    pub fn send_web_message(&self, message: WebMessage) {
        if let Some(context) = &self.context {
            context.send_web_message(message);
        }
    }

    pub(crate) fn take_frame(&self) -> Option<BrowserFrame> {
        self.frames.take()
    }

    pub fn shutdown(&mut self) {
        if !self.initialized {
            return;
        }
        if let Some(context) = self.context.take() {
            context.close();
        }
        self.delegate.take();
        cef::do_message_loop_work();
        cef::shutdown();
        self.initialized = false;
    }
}

impl Drop for CefBrowser {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Dispatches Chromium renderer/helper process modes before logging, Tokio, or
/// winit are initialized. The browser process returns `None` and continues.
#[must_use]
pub fn dispatch_cef_process() -> Option<ExitCode> {
    std::env::args()
        .any(|argument| argument.starts_with("--type="))
        .then(internal::execute_helper_process)
}

fn development_origin(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    match url.origin() {
        url::Origin::Tuple(..) => Some(url.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

#[cfg(target_os = "windows")]
fn cef_resources_dir(distribution: &std::path::Path) -> PathBuf {
    // The UI is bundled under `resources/ui`. Windows would otherwise treat
    // that directory as CEF's conventional `Resources` directory.
    distribution.to_path_buf()
}

#[cfg(target_os = "linux")]
fn cef_resources_dir(distribution: &std::path::Path) -> PathBuf {
    let nested = distribution.join("Resources");
    if nested.is_dir() {
        nested
    } else {
        distribution.to_path_buf()
    }
}

#[cfg(target_os = "macos")]
fn macos_main_bundle(executable: &std::path::Path) -> Result<PathBuf, String> {
    executable
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or_else(|| {
            format!(
                "Koharu is not inside the expected application bundle: {}",
                executable.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn cef_resources_ignore_the_ui_resources_directory() {
        let distribution = tempfile::tempdir().unwrap();
        std::fs::create_dir(distribution.path().join("resources")).unwrap();

        assert_eq!(cef_resources_dir(distribution.path()), distribution.path());
    }

    #[test]
    fn development_origin_extracts_tuple_origin_without_policy() {
        assert_eq!(
            development_origin("http://127.0.0.1:5173/editor?q=1").as_deref(),
            Some("http://127.0.0.1:5173")
        );
        assert_eq!(
            development_origin("https://example.com/editor").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(development_origin("koharu://app/"), None);
    }
}
