use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use anyhow::{anyhow, Context, Result};
use base64::prelude::*;

use async_trait::async_trait;
use koharu_pipeline::{Pipeline, Request, Operation, Stage, RunStatus};
use koharu_scene::{
    Session, PageDraft, AssetRole, AssetInput, AssetMetadata
};
use koharu_renderer::Renderer;
use koharu_rasterizer::{Rasterizer, RasterOptions};

#[derive(Deserialize, Debug)]
#[serde(tag = "action", content = "payload")]
enum ExtensionRequest {
    #[serde(rename = "UploadChunk")]
    UploadChunk {
        #[serde(rename = "transferId")]
        transfer_id: String,
        index: usize,
        total: usize,
        data: String,
    },
    #[serde(rename = "Process")]
    Process {
        #[serde(rename = "transferId")]
        transfer_id: String,
        stages: Vec<Stage>,
        #[serde(rename = "targetLanguage")]
        target_language: Option<String>,
    },
}

#[derive(Serialize, Debug)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ExtensionResponse {
    ChunkReceived {
        #[serde(rename = "transferId")]
        transfer_id: String,
        index: usize,
    },
    Progress {
        #[serde(rename = "transferId")]
        transfer_id: String,
        stage: String,
        message: String,
    },
    DownloadChunk {
        #[serde(rename = "transferId")]
        transfer_id: String,
        index: usize,
        total: usize,
        data: String,
    },
    Success {
        #[serde(rename = "transferId")]
        transfer_id: String,
        #[serde(rename = "inpaintedImage")]
        inpainted_image: Option<String>, // Base64 png
        texts: Vec<TextOverlay>,
    },
    Error {
        #[serde(rename = "transferId")]
        transfer_id: Option<String>,
        message: String,
    },
}

#[derive(Serialize, Debug, Clone)]
struct TextOverlay {
    text: String,
    points: Vec<Point>,
}

#[derive(Serialize, Debug, Clone)]
struct Point {
    x: f64,
    y: f64,
}

struct UploadSession {
    total: usize,
    chunks: HashMap<usize, Vec<u8>>,
}

struct AppState {
    pipeline: Pipeline,
    renderer: Renderer,
    rasterizer: Rasterizer,
    config_handle: koharu_config::Config<koharu_pipeline::PipelineConfig>,
    uploads: Mutex<HashMap<String, UploadSession>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let home_dir = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_owned());
    let log_path = std::path::PathBuf::from(home_dir).join(".koharu").join("native-host.log");
    std::fs::create_dir_all(log_path.parent().unwrap()).ok();
    let log_file = std::fs::File::create(log_path)?;

    tracing_subscriber::fmt()
        .with_writer(move || log_file.try_clone().unwrap())
        .init();

    tracing::info!("Starting Koharu Chrome Native Messaging Host...");

    koharu_ml::init()
        .await
        .context("Failed to initialize ML runtime")?;

    let device = koharu_ml::device(false);
    tracing::info!(?device, "Initialized compute device");

    let active_config = koharu_pipeline::PipelineConfig::load()?;
    let mut config = {
        let config_read = active_config.read()?;
        tracing::info!(
            detection = ?config_read.detection,
            ocr = ?config_read.ocr,
            translation_model = ?config_read.translation.model,
            translation_target = ?config_read.translation.target_language,
            inpainting = ?config_read.inpainting,
            "Loaded system configuration"
        );
        config_read.clone()
    };

    // Fallback/Force Local (llama.cpp) if using a remote provider that fails or isn't set up
    if config.translation.model.provider != koharu_translator::Provider::Local {
        tracing::warn!(
            current_provider = ?config.translation.model.provider,
            "Translation provider is not Local (llama.cpp). Forcing Local provider default model."
        );
        config.translation.model.provider = koharu_translator::Provider::Local;
        config.translation.model.model = Some("lfm2.5-1.2b-instruct".to_owned());
        config.translation.model.quantization = Some("Q4_K_M".to_owned());
    }

    let config_handle = koharu_config::Config::memory(config);

    let pipeline = Pipeline::from_config(
        config_handle.clone(),
        koharu_translator::ProvidersConfig::load()?,
        device,
    )?;

    let renderer = Renderer::new()?;
    let rasterizer = Rasterizer::new()?;

    let state = Arc::new(AppState {
        pipeline,
        renderer,
        rasterizer,
        config_handle,
        uploads: Mutex::new(HashMap::new()),
    });

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    loop {
        match read_frame(&mut stdin).await {
            Ok(Some(frame)) => {
                let state_clone = state.clone();
                let request: Result<ExtensionRequest, serde_json::Error> = serde_json::from_slice(&frame);
                match request {
                    Ok(req) => {
                        let responses = handle_request(state_clone, req).await;
                        for response in responses {
                            let response_bytes = serde_json::to_vec(&response)?;
                            if let Err(e) = write_frame(&mut stdout, &response_bytes).await {
                                tracing::error!(error = %e, "Failed to write response frame");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let err_resp = ExtensionResponse::Error {
                            transfer_id: None,
                            message: format!("Failed to parse request JSON: {}", e),
                        };
                        let bytes = serde_json::to_vec(&err_resp)?;
                        let _ = write_frame(&mut stdout, &bytes).await;
                    }
                }
            }
            Ok(None) => {
                tracing::info!("Stdin channel closed. Exiting host process.");
                break;
            }
            Err(e) => {
                tracing::error!(error = %e, "Error reading from stdin");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_request(state: Arc<AppState>, request: ExtensionRequest) -> Vec<ExtensionResponse> {
    match request {
        ExtensionRequest::UploadChunk { transfer_id, index, total, data } => {
            let decoded = match BASE64_STANDARD.decode(data.trim()) {
                Ok(bytes) => bytes,
                Err(e) => return vec![ExtensionResponse::Error {
                    transfer_id: Some(transfer_id),
                    message: format!("Invalid base64: {}", e),
                }],
            };

            let mut uploads = state.uploads.lock().await;
            let entry = uploads.entry(transfer_id.clone()).or_insert_with(|| UploadSession {
                total,
                chunks: HashMap::new(),
            });

            entry.chunks.insert(index, decoded);

            vec![ExtensionResponse::ChunkReceived { transfer_id, index }]
        }
        ExtensionRequest::Process { transfer_id, stages, target_language } => {
            let image_bytes = {
                let mut uploads = state.uploads.lock().await;
                if let Some(session) = uploads.remove(&transfer_id) {
                    let mut complete = Vec::new();
                    for i in 0..session.total {
                        if let Some(chunk) = session.chunks.get(&i) {
                            complete.extend_from_slice(chunk);
                        } else {
                            return vec![ExtensionResponse::Error {
                                transfer_id: Some(transfer_id),
                                message: format!("Missing chunk index {}", i),
                            }];
                        }
                    }
                    complete
                } else {
                    return vec![ExtensionResponse::Error {
                        transfer_id: Some(transfer_id),
                        message: "No upload session found for this transferId".to_owned(),
                    }];
                }
            };

            match run_pipeline(state.clone(), &transfer_id, image_bytes, stages, target_language).await {
                Ok(success) => success,
                Err(e) => vec![ExtensionResponse::Error {
                    transfer_id: Some(transfer_id),
                    message: format!("Pipeline execution failed: {:?}", e),
                }],
            }
        }
    }
}

async fn run_pipeline(
    state: Arc<AppState>,
    transfer_id: &str,
    image_bytes: Vec<u8>,
    stages: Vec<Stage>,
    target_language: Option<String>,
) -> Result<Vec<ExtensionResponse>> {
    // 1. Create a scene session
    let mut session = Session::memory().await?;
    let mut edit = session.snapshot().edit();

    // Decode image dimensions to set up page correctly
    let (width, height) = (|| -> Result<(f64, f64), image::ImageError> {
        let reader = image::ImageReader::new(std::io::Cursor::new(&image_bytes))
            .with_guessed_format()?;
        let (w, h) = reader.into_dimensions()?;
        Ok((w as f64, h as f64))
    })()
    .unwrap_or((1000.0, 1000.0));

    // 2. Add page and source image asset
    let page = edit.add_page(
        PageDraft::new("web-capture", width, height),
        koharu_scene::At::End,
    )?;

    edit.set_asset(
        page,
        &AssetRole::new("source")?,
        AssetInput::new(
            Arc::<[u8]>::from(image_bytes),
            "image/png",
            AssetMetadata {
                width: Some(width as u32),
                height: Some(height as u32),
                attributes: std::collections::BTreeMap::new(),
            },
        ),
    )?;

    let patch = edit.finish()?;
    session.commit(patch).await?;

    // Update target language in the live configuration if requested
    if let Some(ref lang_str) = target_language {
        if let Ok(lang) = koharu_translator::Language::try_from(lang_str.as_str()) {
            let mut write_guard = state.config_handle.write()?;
            if write_guard.translation.target_language != lang {
                write_guard.translation.target_language = lang;
            }
        }
    }

    // 3. Prepare execution request
    let request = Request {
        operation: Operation::Stages { stages },
        ..Request::default()
    };

    struct PipelineCommitter {
        session: Session,
    }

    #[async_trait]
    impl koharu_pipeline::Committer for PipelineCommitter {
        async fn commit(&mut self, output: koharu_pipeline::StageOutput) -> Result<koharu_scene::Snapshot> {
            self.session.commit(output.patch).await?;
            Ok(self.session.snapshot())
        }
    }

    let mut committer = PipelineCommitter { session };

    // 4. Run the pipeline
    let report = state.pipeline
        .execute(committer.session.snapshot(), request, &mut committer)
        .await
        .map_err(|e| anyhow!("Pipeline run error: {:?}", e))?;


    if report.status == RunStatus::Stopped {
        return Err(anyhow!("Pipeline execution was stopped."));
    }

    // 5. Render and rasterize the final page (containing both inpainted canvas and translated text)
    let final_snapshot = committer.session.snapshot();
    let frame = state.renderer.render(&final_snapshot, page).await?;
    let raster = state.rasterizer.rasterize(&frame.raster_frame()?, RasterOptions::default())?;

    // Encode to PNG bytes in memory
    let mut png_bytes = Vec::new();
    let mut writer = std::io::Cursor::new(&mut png_bytes);
    raster.image.write_to(&mut writer, image::ImageFormat::Png)?;
    let encoded = BASE64_STANDARD.encode(&png_bytes);

    // Chunk size: 500 KB (base64 characters = 500,000 bytes)
    const CHUNK_SIZE: usize = 500 * 1024;
    let total_chunks = (encoded.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;

    let mut responses = Vec::new();
    for i in 0..total_chunks {
        let start = i * CHUNK_SIZE;
        let end = std::cmp::min(start + CHUNK_SIZE, encoded.len());
        let chunk = encoded[start..end].to_owned();

        responses.push(ExtensionResponse::DownloadChunk {
            transfer_id: transfer_id.to_owned(),
            index: i,
            total: total_chunks,
            data: chunk,
        });
    }

    Ok(responses)
}

async fn read_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>>
where
    R: AsyncReadExt + Unpin,
{
    let mut length_buf = [0u8; 4];
    match reader.read_exact(&mut length_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let length = u32::from_ne_bytes(length_buf) as usize;
    let mut data = vec![0u8; length];
    reader.read_exact(&mut data).await?;
    Ok(Some(data))
}

async fn write_frame<W>(writer: &mut W, data: &[u8]) -> Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let len = data.len() as u32;
    writer.write_all(&len.to_ne_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}
