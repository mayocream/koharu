//! Test-harness bootstrap. One `TestApp::spawn()` per test.
//!
//! **Shared runtime cache.** The llama.cpp dylibs and any future runtime
//! packages are heavy to download + unpack, so they live at
//! `integration-tests/.cache` and are prepared exactly once per `cargo test`
//! invocation (via `tokio::sync::OnceCell`). Every `TestApp` reuses the
//! resulting `Arc<RuntimeManager>`.
//!
//! **Per-test data dir.** Each `TestApp` still gets its own tempdir for
//! `AppConfig.data.path` — so project state, blobs, and any transient caches
//! stay isolated and are wiped when the test finishes.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use koharu_app::{App, AppConfig};
use koharu_client::apis::configuration::Configuration;
use koharu_rpc::server;
use koharu_runtime::{ComputePolicy, RuntimeHttpConfig, RuntimeManager};
use tokio::net::TcpListener;
use tokio::sync::OnceCell;

/// Path to the shared runtime cache. Relative to the workspace root so it's
/// a stable, reusable location across `cargo test` invocations.
fn cache_root() -> Utf8PathBuf {
    let manifest_dir =
        Utf8PathBuf::from_path_buf(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
            .expect("manifest dir UTF-8");
    manifest_dir.join(".cache")
}

/// Lazily-built shared runtime. First access prepares it; all subsequent
/// `TestApp::spawn()` calls reuse the same `Arc<RuntimeManager>`.
static SHARED_RUNTIME: OnceCell<Arc<RuntimeManager>> = OnceCell::const_new();

async fn shared_runtime() -> Result<Arc<RuntimeManager>> {
    SHARED_RUNTIME
        .get_or_try_init(|| async {
            let root = cache_root();
            std::fs::create_dir_all(root.as_std_path()).context("create runtime cache root")?;
            let http = RuntimeHttpConfig {
                connect_timeout_secs: 30,
                read_timeout_secs: 600,
                max_retries: 2,
            };
            let runtime =
                RuntimeManager::new_with_http(root.as_std_path(), ComputePolicy::CpuOnly, http)?;
            runtime.prepare().await.context("prepare runtime")?;
            Ok::<_, anyhow::Error>(Arc::new(runtime))
        })
        .await
        .cloned()
}

/// Owned bundle tying together the tempdir, the running App, and the server
/// task.
pub struct TestApp {
    pub app: Arc<App>,
    pub addr: SocketAddr,
    pub base_url: String,
    pub client_config: Configuration,
    _server: tokio::task::JoinHandle<Result<()>>,
    _data_dir: tempfile::TempDir,
}

impl TestApp {
    /// Boot an App on a fresh tempdir + ephemeral port.
    pub async fn spawn() -> Result<Self> {
        Self::spawn_with(|_| {}).await
    }

    /// Boot with a closure to tweak `AppConfig` before construction.
    pub async fn spawn_with(tweak: impl FnOnce(&mut AppConfig)) -> Result<Self> {
        let data_dir = tempfile::tempdir()?;
        let data_root = Utf8PathBuf::from_path_buf(data_dir.path().to_path_buf())
            .map_err(|p| anyhow::anyhow!("tempdir not UTF-8: {}", p.display()))?;

        // Redirect `default_app_data_root()` to the per-test tempdir so
        // routes like `config::save` / `config::config_path` don't clobber
        // the user's real `~/AppData/Local/Koharu/config.toml`. Set
        // unconditionally — last writer wins, but each `App` instance only
        // needs its own `ArcSwap<AppConfig>` which is read at construction.
        // SAFETY: set_var on a single-process test harness is fine.
        unsafe {
            std::env::set_var("KOHARU_DATA_ROOT", data_root.as_str());
        }

        let mut config = AppConfig::default();
        config.data.path = data_root;
        tweak(&mut config);

        // Shared runtime (prepared once per process).
        let runtime = shared_runtime().await?;
        let app = Arc::new(App::new(config, runtime, true, "test")?);
        app.spawn_llm_forwarder();

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let base_url = format!("http://{addr}/api/v1");
        let server = tokio::spawn({
            let app = app.clone();
            async move { server::serve_with_listener(listener, app).await }
        });

        let client_config = Configuration {
            base_path: base_url.clone(),
            user_agent: Some("koharu-integration-tests".to_string()),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            ..Default::default()
        };

        Ok(Self {
            app,
            addr,
            base_url,
            client_config,
            _server: server,
            _data_dir: data_dir,
        })
    }

    /// Create a fresh project under a sub-directory of the tempdir and open it.
    pub async fn open_fresh_project(&self, name: &str) -> Result<Utf8PathBuf> {
        let dir = Utf8PathBuf::from_path_buf(self._data_dir.path().join(format!("{name}.khrproj")))
            .map_err(|p| anyhow::anyhow!("project path not UTF-8: {}", p.display()))?;
        self.app
            .open_project(dir.clone(), Some(name.to_string()))
            .await?;
        Ok(dir)
    }

    /// Build a tiny in-memory PNG for image-based tests.
    pub fn tiny_png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(w, h, image::Rgba(color))
            .write_to(&mut buf, image::ImageFormat::Png)
            .expect("encode png");
        buf.into_inner()
    }
}
