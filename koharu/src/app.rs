//! Binary entry point. Wires `koharu-app::App` to the axum router plus
//! (optionally) Tauri.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use koharu_app::{App, AppConfig, config as app_config};
use koharu_rpc::server;
use koharu_runtime::{ComputePolicy, RuntimeHttpConfig, RuntimeManager};
use tokio::net::TcpListener;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::cli::Cli;

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    // ── Platform & logging ───────────────────────────────────────────
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

    if cli.no_keyring {
        koharu_llm::providers::disable_keyring();
    }

    // ── Config ───────────────────────────────────────────────────────
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

    // ── Runtime + App ────────────────────────────────────────────────
    let runtime = RuntimeManager::new_with_http(config.data.path.as_std_path(), compute, http)?;
    runtime
        .prepare()
        .await
        .context("failed to prepare runtime")?;

    #[cfg(target_os = "windows")]
    crate::windows::register_khr().ok();

    let app = Arc::new(App::new(
        config,
        Arc::new(runtime),
        cli.cpu,
        crate::version::current(),
    )?);
    koharu_llm::suppress_native_logs();
    app.spawn_download_forwarder();
    app.spawn_llm_forwarder();

    // ── Server ───────────────────────────────────────────────────────
    let default_port = if cfg!(debug_assertions) { 9999 } else { 0 };
    let bind_host = cli.host.as_deref().unwrap_or("127.0.0.1");
    let bind_port = cli.port.unwrap_or(default_port);
    let listener: TcpListener = TcpListener::bind((bind_host, bind_port)).await?;
    let port = listener.local_addr()?.port();
    tracing::info!(port, "starting server");

    // Extract the embedded UI assets up-front so both headless and GUI modes
    // can serve them on the HTTP fallback. Headless deployments point a
    // browser at the listener port and get the full app; the GUI build uses
    // the same bundle inside Tauri's webview.
    let mut context = tauri::generate_context!();
    let assets = crate::assets::from_context(&mut context);

    if cli.headless {
        tauri::async_runtime::spawn(async move {
            if let Err(e) = server::serve_with_listener_and_assets(listener, app, assets).await {
                tracing::error!("server error: {e:#}");
            }
        });
        tracing::info!(port, "headless: open http://127.0.0.1:{port}/ in a browser");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    // ── GUI ──────────────────────────────────────────────────────────
    tauri::async_runtime::spawn(async move {
        if let Err(e) = server::serve_with_listener_and_assets(listener, app, assets).await {
            tracing::error!("server error: {e:#}");
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |handle| {
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
