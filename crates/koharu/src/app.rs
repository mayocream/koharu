use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, Result};
use tauri::{AppHandle, Cef, Manager as _, WindowEvent};
use tokio::sync::Mutex;

use crate::commands::{
    agent::AgentState,
    canvas::{CanvasChannel, CanvasView},
    lifecycle::{
        Download, DownloadChannel, DownloadState, Initialization, ModelResources, ProjectChannel,
        ResourceChannel,
    },
    processing::{JobChannel, Processing},
    project::{CurrentProject, ProjectLibrary},
};

pub(crate) async fn initialize(handle: AppHandle<Cef>) -> Result<()> {
    koharu_ml::init()
        .await
        .context("failed to initialize the ML runtime")?;
    let pipeline = koharu_pipeline::Pipeline::load(koharu_ml::device(false))?;
    handle.manage(pipeline.clone());

    let mut resources = pipeline.subscribe_resources();
    let resource_handle = handle.clone();
    drop(tauri::async_runtime::spawn(async move {
        while resources.changed().await.is_ok() {
            let snapshot = resources.borrow_and_update().clone();
            let resources = resource_handle.state::<ResourceChannel>();
            let mut channel = resources.channel.lock();
            if let Some(current) = channel.as_ref()
                && current.send(ModelResources::from(snapshot)).is_err()
            {
                channel.take();
            }
        }
    }));

    let project = handle
        .state::<CurrentProject>()
        .project
        .lock()
        .await
        .as_ref()
        .map(|project| (project.snapshot(), project.active_page()));
    let canvas_view = handle.state::<CanvasView>();
    let desktop = handle.state::<crate::desktop::Desktop>();
    if let Some((snapshot, page)) = project {
        desktop.show_page(&snapshot, page).await?;
        canvas_view.fitted.store(true, Ordering::Release);
    } else {
        desktop.clear().await;
    }
    Ok(())
}

pub fn run(context: tauri::Context<Cef>) -> Result<()> {
    let builder = tauri::Builder::<Cef>::default();
    #[cfg(debug_assertions)]
    let builder = builder.command_line_args([("remote-debugging-port", Some("4000"))]);
    builder
        .plugin(tauri_plugin_single_instance::init(|handle, _, _| {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::FULLSCREEN,
                )
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(crate::commands::bindings().invoke_handler())
        .setup(move |application| {
            application.manage(CurrentProject {
                project: Mutex::new(None),
            });
            application.manage(ProjectLibrary::new()?);
            application.manage(CanvasView {
                fitted: AtomicBool::new(true),
            });
            application.manage(Processing::default());
            application.manage(CanvasChannel::default());
            application.manage(JobChannel::default());
            application.manage(DownloadChannel::default());
            application.manage(ResourceChannel::default());
            application.manage(ProjectChannel::default());
            application.manage(Initialization::default());

            let handle = application.handle().clone();
            application.manage(crate::desktop::Desktop::new(handle.clone())?);
            application.manage(AgentState::new(handle.clone())?);

            let window = application
                .get_webview_window("main")
                .context("the main Tauri webview window is unavailable")?;
            tauri::async_runtime::block_on(crate::desktop::attach(window.clone()))
                .context("failed to attach the desktop runtime")?;
            window.show().context("failed to show the main window")?;
            window
                .set_focus()
                .context("failed to focus the main window")?;
            let initialization_handle = handle.clone();
            drop(tauri::async_runtime::spawn(async move {
                initialize(initialization_handle.clone())
                    .await
                    .expect("failed to initialize the desktop runtime");
                initialization_handle.state::<Initialization>().ready();
            }));

            let mut downloads = koharu_runtime::downloads::subscribe();
            let download_handle = handle.clone();
            drop(tauri::async_runtime::spawn(async move {
                loop {
                    match downloads.recv().await {
                        Ok(event) => {
                            let download = match event {
                                koharu_runtime::downloads::Event::Started { id, name } => {
                                    Download {
                                        id,
                                        state: DownloadState::Running,
                                        name: Some(name),
                                        completed: 0,
                                        total: 0,
                                        error: None,
                                    }
                                }
                                koharu_runtime::downloads::Event::Progress {
                                    id,
                                    name,
                                    completed,
                                    total,
                                } => Download {
                                    id,
                                    state: DownloadState::Running,
                                    name: Some(name),
                                    completed,
                                    total,
                                    error: None,
                                },
                                koharu_runtime::downloads::Event::Finished { id } => Download {
                                    id,
                                    state: DownloadState::Finished,
                                    name: None,
                                    completed: 0,
                                    total: 0,
                                    error: None,
                                },
                                koharu_runtime::downloads::Event::Failed { id, name, error } => {
                                    Download {
                                        id,
                                        state: DownloadState::Failed,
                                        name: Some(name),
                                        completed: 0,
                                        total: 0,
                                        error: Some(error),
                                    }
                                }
                            };
                            let downloads = download_handle.state::<DownloadChannel>();
                            let mut channel = downloads.channel.lock();
                            if let Some(current) = channel.as_ref()
                                && current.send(download).is_err()
                            {
                                channel.take();
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "download channel fell behind");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }));

            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(
                event,
                WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
            ) {
                let processing = window.state::<Processing>();
                for stop in processing.stops.lock().values() {
                    stop.stop();
                }
                processing.stops.lock().clear();
                processing.jobs.lock().clear();
                window.state::<AgentState>().cancel_all();
            }
        })
        .run(context)?;
    Ok(())
}
