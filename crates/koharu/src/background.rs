use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use crate::protocol::{ExportFormat, PipelineScope, PipelineStages, RequestId};
use anyhow::{Context as _, Result, anyhow};
use koharu_config::Config;
use koharu_desktop::DesktopHandle;
use koharu_pipeline::{
    CancellationToken, PipelineConfig, Progress, Scope, StageSelection, selected_stages,
};
use koharu_psd::{
    PsdDocument, PsdExportOptions, PsdShaderEffect, PsdTextAlign, PsdTextBlock, PsdTextDirection,
    PsdTextStyle, ResolvedDocument, export_document,
};
use koharu_renderer::{PageRenderOptions, Renderer};
use koharu_scene::{
    ElementKind, FontSlant, Page, PageId, Revision, Session, TextDirection, WritingMode,
};
use koharu_worker::{
    Event as WorkerEvent, Outcome as WorkerOutcome, Pool as WorkerPool, Request as WorkerRequest,
};

pub enum NativeEvent {
    Download(koharu_runtime::download::Event),
    PipelineProgress {
        job: RequestId,
        progress: koharu_pipeline::Progress,
    },
    ImportProgress {
        job: RequestId,
        completed: usize,
        total: usize,
    },
    ExportProgress {
        job: RequestId,
        completed: usize,
        total: usize,
    },
    ProjectAdvanced {
        job: RequestId,
    },
    Finished {
        job: RequestId,
        revisions: Vec<Revision>,
        pages: Vec<PageId>,
        cancelled: bool,
        error: Option<String>,
    },
}

pub struct PipelineRequest {
    pub id: RequestId,
    pub path: PathBuf,
    pub scope: PipelineScope,
    pub stages: PipelineStages,
    pub target_language: Option<String>,
    pub instructions: Option<String>,
}

pub struct ExportRequest {
    pub id: RequestId,
    pub path: PathBuf,
    pub directory: PathBuf,
    pub pages: Vec<PageId>,
    pub format: ExportFormat,
}

enum Job {
    Pipeline {
        request: PipelineRequest,
        cancellation: CancellationToken,
        desktop: DesktopHandle<NativeEvent>,
    },
    Import {
        id: RequestId,
        path: PathBuf,
        files: Vec<PathBuf>,
        cancellation: CancellationToken,
        desktop: DesktopHandle<NativeEvent>,
    },
    Export {
        request: ExportRequest,
        cancellation: CancellationToken,
        desktop: DesktopHandle<NativeEvent>,
    },
    Shutdown,
}

pub struct Background {
    sender: mpsc::Sender<Job>,
    worker: Option<thread::JoinHandle<()>>,
    download_stop: Arc<AtomicBool>,
    download_worker: Option<thread::JoinHandle<()>>,
}

impl Background {
    pub fn new(config: Config<PipelineConfig>) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("koharu-background".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("koharu-background")
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        tracing::error!(%error, "failed to create the background runtime");
                        return;
                    }
                };
                runtime.block_on(worker(receiver, config));
            })
            .context("failed to start the background runtime")?;
        Ok(Self {
            sender,
            worker: Some(worker),
            download_stop: Arc::new(AtomicBool::new(false)),
            download_worker: None,
        })
    }

    pub fn subscribe_downloads(&mut self, desktop: DesktopHandle<NativeEvent>) -> Result<()> {
        if self.download_worker.is_some() {
            return Ok(());
        }

        let mut events = koharu_runtime::download::subscribe();
        let stop = self.download_stop.clone();
        let worker = thread::Builder::new()
            .name("koharu-download-events".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    match events.try_recv() {
                        Ok(event) => {
                            if desktop.send_event(NativeEvent::Download(event)).is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                            thread::park_timeout(Duration::from_millis(50));
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "download event subscriber fell behind");
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
                    }
                }
            })
            .context("failed to start the download event subscriber")?;
        self.download_worker = Some(worker);
        Ok(())
    }

    pub fn run_pipeline(
        &self,
        request: PipelineRequest,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<CancellationToken> {
        let cancellation = CancellationToken::default();
        self.sender
            .send(Job::Pipeline {
                request,
                cancellation: cancellation.clone(),
                desktop,
            })
            .map_err(|_| anyhow!("background runtime has stopped"))?;
        Ok(cancellation)
    }

    pub fn import(
        &self,
        id: RequestId,
        path: PathBuf,
        files: Vec<PathBuf>,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<CancellationToken> {
        let cancellation = CancellationToken::default();
        self.sender
            .send(Job::Import {
                id,
                path,
                files,
                cancellation: cancellation.clone(),
                desktop,
            })
            .map_err(|_| anyhow!("background runtime has stopped"))?;
        Ok(cancellation)
    }

    pub fn export(
        &self,
        request: ExportRequest,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<CancellationToken> {
        let cancellation = CancellationToken::default();
        self.sender
            .send(Job::Export {
                request,
                cancellation: cancellation.clone(),
                desktop,
            })
            .map_err(|_| anyhow!("background runtime has stopped"))?;
        Ok(cancellation)
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.download_stop.store(true, Ordering::Release);
        if let Some(worker) = self.download_worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
        let _ = self.sender.send(Job::Shutdown);
        // Inference may be inside an uninterruptible FFI call. Detach instead of
        // freezing the Winit shutdown path; the process owns this worker.
        self.worker.take();
    }
}

async fn worker(receiver: mpsc::Receiver<Job>, config: Config<PipelineConfig>) {
    let mut renderer = None;
    let mut workers = WorkerPool::new();
    while let Ok(job) = receiver.recv() {
        match job {
            Job::Pipeline {
                request,
                cancellation,
                desktop,
            } => {
                run_pipeline(&config, &mut workers, request, cancellation, desktop).await;
            }
            Job::Import {
                id,
                path,
                files,
                cancellation,
                desktop,
            } => import(id, path, files, cancellation, desktop),
            Job::Export {
                request,
                cancellation,
                desktop,
            } => export(&mut renderer, request, cancellation, desktop),
            Job::Shutdown => break,
        }
    }
    workers.shutdown().await;
}

async fn run_pipeline(
    config: &Config<PipelineConfig>,
    workers: &mut WorkerPool,
    request: PipelineRequest,
    cancellation: CancellationToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let PipelineRequest {
        id,
        path,
        scope,
        stages,
        target_language,
        instructions,
    } = request;
    let mut revisions = Vec::new();
    let mut error = None;
    let prepared = (|| -> Result<_> {
        let config = config.read()?.clone();
        let selection = match stages {
            PipelineStages::All => StageSelection::All,
            PipelineStages::Through { stage } => StageSelection::Through(stage),
            PipelineStages::Only { stage } => StageSelection::Only(stage),
        };
        let stages = selected_stages(&config, selection)?;
        let scope = match scope {
            PipelineScope::Project => Scope::Project,
            PipelineScope::Pages { pages } => Scope::Pages(pages),
            PipelineScope::Region { page, frame } => Scope::Region { page, frame },
            PipelineScope::Elements { elements } => Scope::Elements(elements),
        };
        Ok((config, stages, scope))
    })();

    match prepared {
        Err(source) => error = Some(source.to_string()),
        Ok((config, stages, scope)) => {
            let total = stages.len();
            for (index, stage) in stages.into_iter().enumerate() {
                if cancellation.is_cancelled() {
                    break;
                }
                let request =
                    WorkerRequest::new(path.clone(), config.clone(), stage, scope.clone())
                        .target_language(target_language.clone())
                        .instructions(instructions.clone());
                let before = match Session::open(&path) {
                    Ok(session) => session.revision(),
                    Err(source) => {
                        error = Some(format!("failed to inspect {}: {source}", path.display()));
                        break;
                    }
                };
                let event_handle = desktop.clone();
                let outcome = workers
                    .execute(&request, &cancellation, move |event| match event {
                        WorkerEvent::Progress { stage, model, .. } => {
                            let _ =
                                event_handle.send_event(NativeEvent::ProjectAdvanced { job: id });
                            let _ = event_handle.send_event(NativeEvent::PipelineProgress {
                                job: id,
                                progress: Progress {
                                    stage,
                                    model,
                                    completed: index + 1,
                                    total,
                                },
                            });
                        }
                        WorkerEvent::Download(event) => {
                            let _ = event_handle.send_event(NativeEvent::Download(event));
                        }
                        WorkerEvent::Finished(_) | WorkerEvent::Failed(_) => {
                            unreachable!("terminal worker events are returned as outcomes")
                        }
                    })
                    .await;
                match outcome {
                    Ok(WorkerOutcome::Finished(report)) => revisions.extend(report.revisions),
                    Ok(WorkerOutcome::Failed(failure)) => {
                        revisions.extend(failure.revisions);
                        error = Some(failure.error);
                        break;
                    }
                    Ok(WorkerOutcome::Cancelled) => {
                        if let Err(source) = append_revisions_after(&path, before, &mut revisions) {
                            error = Some(source.to_string());
                        }
                        break;
                    }
                    Err(source) => {
                        if let Err(recovery) = append_revisions_after(&path, before, &mut revisions)
                        {
                            error = Some(format!("{source}; {recovery}"));
                            break;
                        }
                        error = Some(source.to_string());
                        break;
                    }
                }
            }
        }
    }

    let _ = desktop.send_event(NativeEvent::Finished {
        job: id,
        revisions,
        pages: Vec::new(),
        cancelled: cancellation.is_cancelled(),
        error,
    });
}

fn append_revisions_after(
    path: &Path,
    from: Revision,
    revisions: &mut Vec<Revision>,
) -> Result<()> {
    let head = Session::open(path)
        .with_context(|| format!("failed to recover the revision of {}", path.display()))?
        .revision();
    for revision in from.get().saturating_add(1)..=head.get() {
        revisions.push(Revision::new(revision));
    }
    Ok(())
}

fn import(
    id: RequestId,
    path: PathBuf,
    files: Vec<PathBuf>,
    cancellation: CancellationToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let total = files.len();
    let mut revisions = Vec::new();
    let mut pages = Vec::new();
    let result = (|| -> Result<()> {
        let mut session =
            Session::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        for (index, file) in files.into_iter().enumerate() {
            if cancellation.is_cancelled() {
                break;
            }
            let bytes =
                fs::read(&file).with_context(|| format!("failed to read {}", file.display()))?;
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("page")
                .to_owned();
            let mut commands = session.commands();
            let page = commands.add_page(name, bytes)?;
            let changes = session.apply(commands)?;
            pages.push(page);
            revisions.push(changes.to);
            let _ = desktop.send_event(NativeEvent::ProjectAdvanced { job: id });
            let _ = desktop.send_event(NativeEvent::ImportProgress {
                job: id,
                completed: index + 1,
                total,
            });
        }
        Ok(())
    })();
    let _ = desktop.send_event(NativeEvent::Finished {
        job: id,
        revisions,
        pages,
        cancelled: cancellation.is_cancelled(),
        error: result.err().map(|error| error.to_string()),
    });
}

fn export(
    renderer: &mut Option<Renderer>,
    request: ExportRequest,
    cancellation: CancellationToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let ExportRequest {
        id,
        path,
        directory,
        pages,
        format,
    } = request;
    let total = pages.len();
    let result = (|| -> Result<()> {
        let session =
            Session::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        if renderer.is_none() {
            *renderer = Some(Renderer::new().context("failed to initialize the export renderer")?);
        }
        let renderer = renderer.as_ref().expect("renderer initialized above");
        for (index, page_id) in pages.into_iter().enumerate() {
            if cancellation.is_cancelled() {
                break;
            }
            let page = session.page(page_id)?;
            let base_id = page.assets.clean.unwrap_or(page.source);
            let base = image::load_from_memory(&session.read_blob(base_id)?)
                .with_context(|| format!("failed to decode page {}", page.name))?;
            let bubble = page
                .assets
                .bubble_mask
                .map(|blob| session.read_blob(blob))
                .transpose()?
                .map(|bytes| image::load_from_memory(&bytes))
                .transpose()
                .with_context(|| format!("failed to decode bubble mask for {}", page.name))?;
            let rendered = renderer.composite_page(
                &base,
                bubble.as_ref(),
                page,
                |blob| session.read_blob(blob).map_err(Into::into),
                &PageRenderOptions::default(),
            )?;
            let stem = format!("{:04}_{}", index + 1, safe_name(&page.name));
            match format {
                ExportFormat::Png => rendered
                    .image
                    .save(directory.join(format!("{stem}.png")))
                    .with_context(|| format!("failed to export {}", page.name))?,
                ExportFormat::Psd => {
                    let bytes = export_psd(&session, renderer, page, rendered.image)?;
                    fs::write(directory.join(format!("{stem}.psd")), bytes)
                        .with_context(|| format!("failed to export {}", page.name))?;
                }
            }
            let _ = desktop.send_event(NativeEvent::ExportProgress {
                job: id,
                completed: index + 1,
                total,
            });
        }
        Ok(())
    })();
    let _ = desktop.send_event(NativeEvent::Finished {
        job: id,
        revisions: Vec::new(),
        pages: Vec::new(),
        cancelled: cancellation.is_cancelled(),
        error: result.err().map(|error| error.to_string()),
    });
}

fn export_psd(
    session: &Session,
    renderer: &Renderer,
    page: &Page,
    rendered: image::RgbaImage,
) -> Result<Vec<u8>> {
    let mut document = PsdDocument {
        width: page.size.width,
        height: page.size.height,
        ..PsdDocument::default()
    };
    for element in &page.elements {
        let ElementKind::Text(text) = &element.kind else {
            continue;
        };
        let value = text.translation.as_deref().unwrap_or_default();
        let post_script = renderer.resolve_post_script_name(&text.style, Some(value))?;
        let font_index = document
            .fonts
            .iter()
            .position(|font| font == &post_script)
            .unwrap_or_else(|| {
                document.fonts.push(post_script);
                document.fonts.len() - 1
            });
        let source_direction = text
            .source
            .as_ref()
            .and_then(|source| match source.direction {
                TextDirection::Horizontal => Some(PsdTextDirection::Horizontal),
                TextDirection::Vertical => Some(PsdTextDirection::Vertical),
                TextDirection::Auto => None,
            });
        let rendered_direction = match text.layout.writing_mode {
            WritingMode::Horizontal => Some(PsdTextDirection::Horizontal),
            WritingMode::VerticalRightToLeft | WritingMode::VerticalLeftToRight => {
                Some(PsdTextDirection::Vertical)
            }
            WritingMode::Auto => source_direction,
        };
        document.text_blocks.push(PsdTextBlock {
            id: element.id.to_string(),
            x: element.frame.x,
            y: element.frame.y,
            width: element.frame.width,
            height: element.frame.height,
            translation: text.translation.clone(),
            style: Some(PsdTextStyle {
                font_families: text.style.font_families.clone(),
                font_size: Some(text.style.font_size),
                color: text.style.color,
                effect: Some(PsdShaderEffect {
                    italic: !matches!(text.style.font_slant, FontSlant::Normal),
                    bold: text.style.font_weight >= 600,
                }),
                text_align: Some(match text.layout.horizontal_align {
                    koharu_scene::TextAlign::Start | koharu_scene::TextAlign::Justify => {
                        PsdTextAlign::Left
                    }
                    koharu_scene::TextAlign::Center => PsdTextAlign::Center,
                    koharu_scene::TextAlign::End => PsdTextAlign::Right,
                }),
            }),
            rotation_deg: Some(element.frame.angle_degrees + text.style.angle_degrees),
            source_direction,
            rendered_direction,
            detected_font_size_px: Some(text.style.font_size),
            font_index: Some(font_index),
            ..PsdTextBlock::default()
        });
    }

    let source = image::load_from_memory(&session.read_blob(page.source)?)?;
    let clean = page
        .assets
        .clean
        .map(|blob| session.read_blob(blob))
        .transpose()?
        .map(|bytes| image::load_from_memory(&bytes))
        .transpose()?;
    let text_mask = page
        .assets
        .text_mask
        .map(|blob| session.read_blob(blob))
        .transpose()?
        .map(|bytes| image::load_from_memory(&bytes))
        .transpose()?;
    let rendered = image::DynamicImage::ImageRgba8(rendered);
    let resolved = ResolvedDocument {
        document: &document,
        source: &source,
        segment: text_mask.as_ref(),
        inpainted: clean.as_ref(),
        rendered: Some(&rendered),
        brush_layer: None,
        block_images: &HashMap::new(),
    };
    export_document(
        &resolved,
        &PsdExportOptions {
            include_brush_layer: false,
            ..PsdExportOptions::default()
        },
    )
    .map_err(Into::into)
}

fn safe_name(name: &str) -> String {
    let value = name
        .trim()
        .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
    let value = value.rsplit_once('.').map_or(value, |(stem, _)| stem);
    let value = value
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "page".into()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_names_cannot_escape_the_selected_directory() {
        assert_eq!(safe_name("../chapter:01.png"), ".._chapter_01");
        assert_eq!(safe_name(".png"), "page");
    }
}
