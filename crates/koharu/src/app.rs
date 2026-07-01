//! Binary entry point. Wires `koharu-app::App` to the axum router plus
//! (optionally) Tauri.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use koharu_app::{App, AppConfig, config as app_config};
use koharu_rpc::{BootstrapManager, server};
use koharu_runtime::{ComputePolicy, RuntimeHttpConfig, RuntimeManager};
use tokio::net::TcpListener;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::cli::Cli;

async fn bootstrap_app(
    state: Arc<BootstrapManager>,
    config: AppConfig,
    cpu_only: bool,
) -> Result<()> {
    let runtime = state.runtime();
    runtime
        .prepare()
        .await
        .context("failed to prepare runtime")?;

    let app = Arc::new(App::new_with_shared_state(
        config,
        runtime,
        cpu_only,
        state.shared_state(),
        crate::version::current(),
    )?);
    koharu_llm::suppress_native_logs();
    app.spawn_llm_forwarder();
    state
        .set_app(app)
        .map_err(|_| anyhow::anyhow!("app already initialized"))?;
    Ok(())
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    #[cfg(target_os = "windows")]
    {
        let attached = crate::windows::attach_parent_console();
        if !attached && (cli.headless || cli.debug) {
            crate::windows::create_console_window();
        }
        crate::windows::enable_ansi_support().ok();
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .with(crate::sentry::tracing_layer())
        .with(crate::tracing::TimingLayer::new())
        .init();

    let config: AppConfig = app_config::load()?;
    let http = RuntimeHttpConfig {
        connect_timeout_secs: config.http.connect_timeout.max(1),
        read_timeout_secs: config.http.read_timeout.max(1),
        max_retries: config.http.max_retries,
    };
    let compute = if cli.cpu {
        ComputePolicy::CpuOnly
    } else {
        ComputePolicy::PreferGpu
    };

    if cli.download {
        return RuntimeManager::new_with_http(config.data.path.as_std_path(), compute, http)?
            .prepare()
            .await
            .context("failed to download runtime packages");
    }

    let state = BootstrapManager::new(Arc::new(RuntimeManager::new_with_http(
        config.data.path.as_std_path(),
        compute,
        http,
    )?));
    state.spawn_download_forwarder();

    #[cfg(target_os = "windows")]
    crate::windows::register_khr().ok();

    let bind_host = cli.host.as_deref().unwrap_or("127.0.0.1");
    let bind_port = cli.port.unwrap_or(4000);
    let listener: TcpListener = if cfg!(debug_assertions) || cli.port.is_some() {
        TcpListener::bind((bind_host, bind_port)).await?
    } else {
        let mut port = bind_port;
        loop {
            match TcpListener::bind((bind_host, port)).await {
                Ok(listener) => break listener,
                Err(err) if err.kind() == std::io::ErrorKind::AddrInUse && port < u16::MAX => {
                    port += 1;
                }
                Err(err) => return Err(err.into()),
            }
        }
    };
    let port = listener.local_addr()?.port();
    tracing::info!(port, "starting server");

    let mut context = tauri::generate_context!();
    let assets = crate::assets::from_context(&mut context);
    let server_state = state.clone();
    tauri::async_runtime::spawn(async move {
        server::serve_with_listener_and_assets(listener, server_state, assets)
            .await
            .expect("failed to start server");
    });

    if cli.headless {
        tracing::info!(port, "headless: open http://127.0.0.1:{port}/ in a browser");
        bootstrap_app(state, config, cli.cpu).await?;
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |handle| {
            tauri::async_runtime::spawn(async move {
                bootstrap_app(state, config, cli.cpu)
                    .await
                    .expect("failed to bootstrap app");
            });

            let cfg = handle.config();
            let url: tauri::Url = if cfg!(debug_assertions) {
                cfg.build
                    .dev_url
                    .as_ref()
                    .expect("dev_url must be set in dev mode")
                    .as_str()
                    .parse()?
            } else {
                format!("http://127.0.0.1:{port}").parse()?
            };
            let wc = cfg
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .expect("main window config not found");
            tauri::webview::WebviewWindowBuilder::from_config(handle, wc)?
                .build()?
                .navigate(url)?;

            Ok(())
        })
        .run(context)?;

    Ok(())
}
