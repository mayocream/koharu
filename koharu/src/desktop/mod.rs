use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tauri::Builder;
use tokio::net::TcpListener;

use crate::bootstrap::BootstrapManager;
use crate::config::ProjectPaths;
use crate::server::{SharedResources, http};

mod assets;
mod cli;
mod resources;
mod windowing;

pub async fn run() -> Result<()> {
    let cli::Cli {
        download,
        cpu,
        port,
        headless,
        debug,
    } = cli::Cli::parse();

    resources::initialize(headless, debug)?;
    let project_paths = ProjectPaths::discover()?;

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port.unwrap_or(0))).await?;
    let api_port = listener.local_addr()?.port();
    let shared: SharedResources = Arc::new(tokio::sync::OnceCell::new());
    let bootstrap_manager = BootstrapManager::new(
        project_paths,
        shared.clone(),
        Arc::new(move |paths| {
            Box::pin(async move {
                resources::build_resources(cpu, headless, &paths.runtime_root, &paths.models_root)
                    .await
                    .map_err(anyhow::Error::from)
            })
        }),
    )?;

    if download {
        bootstrap_manager.initialize().await?;
        return Ok(());
    }

    let mut context = tauri::generate_context!();
    let shared_assets = assets::share_context_assets(&mut context);

    if headless {
        let resolver = http::asset_resolver([assets::embedded_asset_resolver(shared_assets)]);
        tauri::async_runtime::spawn({
            let shared = shared.clone();
            let bootstrap = bootstrap_manager.clone();
            async move {
                if let Err(err) =
                    http::serve_with_listener(listener, shared, bootstrap, resolver).await
                {
                    tracing::error!("Server error: {err:#}");
                }
            }
        });
        bootstrap_manager.initialize().await?;
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    let embedded_resolver = assets::embedded_asset_resolver(shared_assets);

    Builder::default()
        .append_invoke_initialization_script(format!("window.__KOHARU_API_PORT__ = {api_port};"))
        .setup(move |app| {
            let resolver = http::asset_resolver([
                assets::tauri_asset_resolver(app.asset_resolver()),
                embedded_resolver,
            ]);
            tauri::async_runtime::spawn({
                let shared = shared.clone();
                let bootstrap = bootstrap_manager.clone();
                async move {
                    if let Err(err) =
                        http::serve_with_listener(listener, shared, bootstrap, resolver).await
                    {
                        tracing::error!("Server error: {err:#}");
                    }
                }
            });

            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())
                .ok();

            let handle = app.handle().clone();
            let bootstrap_manager = bootstrap_manager.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = bootstrap_manager.maybe_start_on_launch().await {
                    tracing::error!("Bootstrap startup error: {error:#}");
                }
                if let Err(error) =
                    windowing::sync_bootstrap_windows(&handle, &bootstrap_manager.snapshot())
                {
                    tracing::error!("Window bootstrap sync error: {error:#}");
                }

                let mut rx = bootstrap_manager.subscribe();
                loop {
                    match rx.recv().await {
                        Ok(state) => {
                            if let Err(error) = windowing::sync_bootstrap_windows(&handle, &state) {
                                tracing::error!("Window bootstrap sync error: {error:#}");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            Ok(())
        })
        .run(context)?;

    Ok(())
}
