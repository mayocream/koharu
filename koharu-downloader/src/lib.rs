mod config;
mod download_progress;
mod inventory;
mod state;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::fmt::format::FmtSpan;

pub use config::DownloaderConfig;
pub use inventory::{DownloadInventory, ManagedItemKey, ManagedRootKind};
pub use state::DownloaderApp;

#[derive(Parser)]
#[command(version = env!("CARGO_PKG_VERSION"), about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Doctor,
    Download {
        #[arg(long)]
        item: String,
        #[arg(long)]
        proxy_url: Option<String>,
        #[arg(long)]
        pypi_base_url: Option<String>,
        #[arg(long)]
        github_release_base_url: Option<String>,
    },
    Delete {
        #[arg(long)]
        item: String,
    },
    OpenDir {
        #[arg(value_enum)]
        root: RootArg,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RootArg {
    Runtime,
    Models,
}

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let app = DownloaderApp::new()?;
    match Cli::parse().command {
        Some(Commands::Doctor) => run_doctor(app).await,
        Some(Commands::Download {
            item,
            proxy_url,
            pypi_base_url,
            github_release_base_url,
        }) => {
            let config = DownloaderConfig {
                proxy_url,
                pypi_base_url,
                github_release_base_url,
            };
            if config != DownloaderConfig::default() {
                app.set_config(config).await?;
            }
            let item = ManagedItemKey::parse(&item)?;
            app.start_download(item).await?;
            wait_for_completion(app).await
        }
        Some(Commands::Delete { item }) => {
            app.delete_item(ManagedItemKey::parse(&item)?).await?;
            Ok(())
        }
        Some(Commands::OpenDir { root }) => {
            let root = match root {
                RootArg::Runtime => ManagedRootKind::Runtime,
                RootArg::Models => ManagedRootKind::Model,
            };
            app.open_root(root).await
        }
        None => run_gui(app).await,
    }
}

async fn run_doctor(app: DownloaderApp) -> Result<()> {
    let snapshot = app.snapshot().await;
    println!("Runtime directory: {}", snapshot.runtime_dir);
    println!("Model directory:   {}", snapshot.model_dir);
    println!();
    for item in snapshot.items {
        println!("{:24} {:18?} {:?}", item.id, item.status, item.task.state);
    }
    Ok(())
}

async fn wait_for_completion(app: DownloaderApp) -> Result<()> {
    loop {
        let snapshot = app.snapshot().await;
        let running = snapshot
            .items
            .iter()
            .any(|item| matches!(item.task.state, inventory::TaskState::Running));
        if !running {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

async fn run_gui(app: DownloaderApp) -> Result<()> {
    tauri::Builder::default()
        .manage(app.clone())
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_network_config,
            check_proxy,
            download_item,
            retry_item,
            cancel_active_task,
            delete_item,
            open_runtime_dir,
            open_model_dir
        ])
        .setup(move |handle| {
            app.attach_app_handle(handle.handle().clone());
            tauri::async_runtime::spawn({
                let app = app.clone();
                async move {
                    app.set_config(app.snapshot().await.network).await.ok();
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .context("failed to run downloader tauri app")
}

#[tauri::command]
async fn snapshot(
    app: tauri::State<'_, DownloaderApp>,
) -> std::result::Result<DownloadInventory, String> {
    Ok(app.snapshot().await)
}

#[tauri::command]
async fn set_network_config(
    app: tauri::State<'_, DownloaderApp>,
    config: DownloaderConfig,
) -> std::result::Result<DownloadInventory, String> {
    app.set_config(config).await.map_err(to_string_error)?;
    Ok(app.snapshot().await)
}

#[tauri::command]
async fn download_item(
    app: tauri::State<'_, DownloaderApp>,
    item_id: String,
) -> std::result::Result<DownloadInventory, String> {
    let item = ManagedItemKey::parse(&item_id).map_err(to_string_error)?;
    app.start_download(item).await.map_err(to_string_error)?;
    Ok(app.snapshot().await)
}

#[tauri::command]
async fn retry_item(
    app: tauri::State<'_, DownloaderApp>,
    item_id: String,
) -> std::result::Result<DownloadInventory, String> {
    let item = ManagedItemKey::parse(&item_id).map_err(to_string_error)?;
    app.retry_download(item).await.map_err(to_string_error)?;
    Ok(app.snapshot().await)
}

#[tauri::command]
async fn cancel_active_task(
    app: tauri::State<'_, DownloaderApp>,
) -> std::result::Result<DownloadInventory, String> {
    app.cancel_active_task().await.map_err(to_string_error)?;
    Ok(app.snapshot().await)
}

#[tauri::command]
async fn delete_item(
    app: tauri::State<'_, DownloaderApp>,
    item_id: String,
) -> std::result::Result<DownloadInventory, String> {
    let item = ManagedItemKey::parse(&item_id).map_err(to_string_error)?;
    app.delete_item(item).await.map_err(to_string_error)?;
    Ok(app.snapshot().await)
}

#[tauri::command]
async fn open_runtime_dir(app: tauri::State<'_, DownloaderApp>) -> std::result::Result<(), String> {
    app.open_root(ManagedRootKind::Runtime)
        .await
        .map_err(to_string_error)
}

#[tauri::command]
async fn open_model_dir(app: tauri::State<'_, DownloaderApp>) -> std::result::Result<(), String> {
    app.open_root(ManagedRootKind::Model)
        .await
        .map_err(to_string_error)
}

#[tauri::command]
async fn check_proxy(proxy_url: String) -> std::result::Result<(), String> {
    use std::time::Duration;

    let mut builder = reqwest::Client::builder()
        .user_agent("koharu-downloader/proxy-check")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(10));

    if !proxy_url.trim().is_empty() {
        builder = builder.proxy(
            reqwest::Proxy::all(&proxy_url)
                .map_err(|e| format!("Invalid proxy URL: {e}"))?,
        );
    }

    let client = builder.build().map_err(|e| format!("Failed to build client: {e}"))?;

    client
        .head("https://huggingface.co")
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|e| {
            if e.is_connect() {
                "Connection failed: proxy unreachable or refused".to_string()
            } else if e.is_timeout() {
                "Connection timed out (10s)".to_string()
            } else {
                format!("Request failed: {e}")
            }
        })?;

    Ok(())
}

fn to_string_error(error: impl ToString) -> String {
    error.to_string()
}
