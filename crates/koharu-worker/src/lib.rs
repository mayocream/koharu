use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use koharu_config::Config;
use koharu_pipeline::{
    CancellationToken, InpaintingModel, OcrModel, Pipeline, PipelineConfig, ProgressSink,
    RunReport, Scope, Stage, TranslationModel,
};
use koharu_scene::Revision;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    process::Command,
};

const WORKER_ARGUMENT: &str = "--worker";

const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub path: PathBuf,
    pub config: PipelineConfig,
    pub stage: Stage,
    pub scope: Scope,
    pub target_language: Option<String>,
    pub instructions: Option<String>,
}

impl Request {
    #[must_use]
    pub fn new(path: PathBuf, config: PipelineConfig, stage: Stage, scope: Scope) -> Self {
        Self {
            path,
            config,
            stage,
            scope,
            target_language: None,
            instructions: None,
        }
    }

    #[must_use]
    pub fn target_language(mut self, target_language: Option<String>) -> Self {
        self.target_language = target_language;
        self
    }

    #[must_use]
    pub fn instructions(mut self, instructions: Option<String>) -> Self {
        self.instructions = instructions;
        self
    }

    fn runtime(&self) -> RuntimeKind {
        RuntimeKind::for_stage(&self.config, self.stage)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RuntimeKind {
    Torch,
    Llama,
    Diffusion,
}

impl RuntimeKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Torch => "torch",
            Self::Llama => "llama",
            Self::Diffusion => "diffusion",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "torch" => Ok(Self::Torch),
            "llama" => Ok(Self::Llama),
            "diffusion" => Ok(Self::Diffusion),
            _ => bail!("unknown model worker runtime '{value}'"),
        }
    }

    fn for_stage(config: &PipelineConfig, stage: Stage) -> Self {
        match stage {
            Stage::Ocr if matches!(config.ocr, OcrModel::PaddleOcrVl1_6(_)) => Self::Llama,
            Stage::Translation if matches!(config.translation, TranslationModel::Local(_)) => {
                Self::Llama
            }
            Stage::Inpainting if matches!(config.inpainting, InpaintingModel::Flux2Klein(_)) => {
                Self::Diffusion
            }
            _ => Self::Torch,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Progress {
        stage: Stage,
        model: String,
        completed: usize,
        total: usize,
    },
    Download(koharu_runtime::download::Event),
    Finished(Report),
    Failed(Failure),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub revisions: Vec<Revision>,
    pub processors: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    pub revisions: Vec<Revision>,
    pub error: String,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Finished(Report),
    Failed(Failure),
    Cancelled,
}

#[derive(Default)]
pub struct Pool {
    config: Option<PipelineConfig>,
    clients: HashMap<RuntimeKind, Client>,
}

impl Pool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn execute<F>(
        &mut self,
        request: &Request,
        cancellation: &CancellationToken,
        on_event: F,
    ) -> Result<Outcome>
    where
        F: FnMut(Event),
    {
        if self.config.as_ref() != Some(&request.config) {
            self.shutdown().await;
            self.config = Some(request.config.clone());
        }
        let runtime = request.runtime();
        if !self.clients.contains_key(&runtime) {
            self.clients.insert(runtime, Client::spawn(runtime).await?);
        }

        let result = self
            .clients
            .get_mut(&runtime)
            .expect("worker client was inserted above")
            .execute(request, cancellation, on_event)
            .await;
        if result.is_err() || matches!(&result, Ok(Outcome::Cancelled)) {
            self.clients.remove(&runtime);
        }
        result
    }

    pub async fn shutdown(&mut self) {
        let clients = std::mem::take(&mut self.clients);
        for client in clients.into_values() {
            client.stop().await;
        }
    }
}

struct Client {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
}

impl Client {
    async fn spawn(runtime: RuntimeKind) -> Result<Self> {
        let executable =
            std::env::current_exe().context("failed to locate the Koharu executable")?;
        let mut command = Command::new(executable);
        command
            .arg(WORKER_ARGUMENT)
            .arg(runtime.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        hide_window(&mut command);

        let mut child = command
            .spawn()
            .context("failed to start the model worker")?;
        let stdin = child
            .stdin
            .take()
            .context("model worker stdin was not available")?;
        let stdout = child
            .stdout
            .take()
            .context("model worker stdout was not available")?;
        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    async fn execute<F>(
        &mut self,
        request: &Request,
        cancellation: &CancellationToken,
        mut on_event: F,
    ) -> Result<Outcome>
    where
        F: FnMut(Event),
    {
        if let Err(error) = write_async_frame(&mut self.stdin, request).await {
            self.stop_in_place().await;
            return Err(error).context("failed to send the model worker request");
        }

        let cancelled = async {
            while !cancellation.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        };
        tokio::pin!(cancelled);
        let mut active_downloads = HashMap::new();

        let outcome = loop {
            let event = tokio::select! {
                () = &mut cancelled => {
                    self.stop_in_place().await;
                    break Ok(Outcome::Cancelled);
                }
                event = read_async_frame::<_, Event>(&mut self.stdout) => event,
            };

            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    self.stop_in_place().await;
                    break Err(error).context("model worker stopped before returning a result");
                }
            };
            match event {
                Event::Finished(report) => break Ok(Outcome::Finished(report)),
                Event::Failed(failure) => break Ok(Outcome::Failed(failure)),
                Event::Download(event) => {
                    track_download(&mut active_downloads, &event);
                    on_event(Event::Download(event));
                }
                event => on_event(event),
            }
        };
        fail_active_downloads(&mut active_downloads, &mut on_event);
        outcome
    }

    async fn stop(mut self) {
        self.stop_in_place().await;
    }

    async fn stop_in_place(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

pub fn serve(expected_runtime: &str) -> Result<()> {
    serve_stdio(expected_runtime)
}

fn track_download(active: &mut HashMap<u64, String>, event: &koharu_runtime::download::Event) {
    match event {
        koharu_runtime::download::Event::Started { id, name }
        | koharu_runtime::download::Event::Progress { id, name, .. } => {
            active.insert(*id, name.clone());
        }
        koharu_runtime::download::Event::Finished { id }
        | koharu_runtime::download::Event::Failed { id, .. } => {
            active.remove(id);
        }
    }
}

fn fail_active_downloads<F>(active: &mut HashMap<u64, String>, on_event: &mut F)
where
    F: FnMut(Event),
{
    for (id, name) in active.drain() {
        on_event(Event::Download(koharu_runtime::download::Event::Failed {
            id,
            name,
            error: "Model worker stopped before the download finished.".into(),
        }));
    }
}

fn serve_stdio(expected_runtime: &str) -> Result<()> {
    let expected_runtime = RuntimeKind::parse(expected_runtime)?;
    let mut input = std::io::stdin().lock();
    let output = Arc::new(Mutex::new(std::io::BufWriter::new(std::io::stdout())));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("koharu-model-worker")
        .build()
        .context("failed to create the model worker runtime")?;
    let mut state = None;
    while let Some(request) = read_optional_frame::<_, Request>(&mut input)
        .context("failed to read the model worker request")?
    {
        if expected_runtime != request.runtime() {
            bail!("model worker received a request for an unexpected runtime");
        }
        runtime.block_on(run(request, output.clone(), &mut state))?;
    }
    Ok(())
}

async fn run(
    request: Request,
    output: Arc<Mutex<std::io::BufWriter<std::io::Stdout>>>,
    state: &mut Option<WorkerState>,
) -> Result<()> {
    let mut downloads = koharu_runtime::download::subscribe();
    let pipeline = run_pipeline(request, output.clone(), state);
    tokio::pin!(pipeline);
    let mut downloads_open = true;
    let result = loop {
        tokio::select! {
            result = &mut pipeline => break result,
            event = downloads.recv(), if downloads_open => match event {
                Ok(event) => write_event(&output, &Event::Download(event))?,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    bail!("model worker download relay missed {skipped} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => downloads_open = false,
            },
        }
    };
    loop {
        match downloads.try_recv() {
            Ok(event) => write_event(&output, &Event::Download(event))?,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                bail!("model worker download relay missed {skipped} events");
            }
            Err(
                tokio::sync::broadcast::error::TryRecvError::Empty
                | tokio::sync::broadcast::error::TryRecvError::Closed,
            ) => break,
        }
    }
    let event = match result {
        Ok(report) => Event::Finished(report),
        Err(failure) => Event::Failed(failure),
    };
    write_event(&output, &event)
}

struct WorkerState {
    config: PipelineConfig,
    pipeline: Pipeline,
}

async fn run_pipeline(
    request: Request,
    output: Arc<Mutex<std::io::BufWriter<std::io::Stdout>>>,
    state: &mut Option<WorkerState>,
) -> std::result::Result<Report, Failure> {
    let prepare = (|| -> Result<()> {
        if let Some(state) = state.as_ref() {
            if state.config != request.config {
                bail!("model worker received a different pipeline configuration");
            }
        } else {
            let config = Config::memory(request.config.clone());
            *state = Some(WorkerState {
                config: request.config.clone(),
                pipeline: Pipeline::new(config),
            });
        }
        Ok(())
    })();
    if let Err(error) = prepare {
        return Err(Failure {
            revisions: Vec::new(),
            error: format!("{error:#}"),
        });
    }
    let pipeline = &state
        .as_ref()
        .expect("model worker state was prepared above")
        .pipeline;
    let mut session = koharu_scene::Session::open(&request.path).map_err(|error| Failure {
        revisions: Vec::new(),
        error: format!("failed to open {}: {error}", request.path.display()),
    })?;
    let progress_output = output.clone();
    let progress: ProgressSink = Arc::new(move |progress| {
        let _ = write_event(
            &progress_output,
            &Event::Progress {
                stage: progress.stage,
                model: progress.model,
                completed: progress.completed,
                total: progress.total,
            },
        );
    });
    let mut run = pipeline
        .run(&mut session)
        .only(request.stage)
        .progress(progress);
    run = match request.scope {
        Scope::Project => run,
        Scope::Pages(pages) => run.pages(pages),
        Scope::Region { page, frame } => run.region(page, frame),
        Scope::Elements(elements) => run.elements(elements),
    };
    if let Some(language) = request.target_language {
        run = run.target_language(language);
    }
    if let Some(instructions) = request.instructions {
        run = run.instructions(instructions);
    }
    let result = run.execute().await;

    match result {
        Ok(RunReport {
            revisions,
            processors,
        }) => Ok(Report {
            revisions,
            processors,
        }),
        Err(error) => Err(Failure {
            revisions: error.committed_revisions.clone(),
            error: format!("{error:#}"),
        }),
    }
}

fn write_event(
    output: &Arc<Mutex<std::io::BufWriter<std::io::Stdout>>>,
    event: &Event,
) -> Result<()> {
    let mut output = output
        .lock()
        .map_err(|_| anyhow!("model worker output lock is poisoned"))?;
    write_frame(&mut *output, event)?;
    output
        .flush()
        .context("failed to flush model worker output")
}

fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let payload = rmp_serde::to_vec_named(value).context("failed to encode model worker frame")?;
    if payload.len() > MAX_FRAME_SIZE {
        bail!("model worker frame exceeds the size limit");
    }
    let length = u32::try_from(payload.len()).context("model worker frame is too large")?;
    let mut frame = Vec::with_capacity(size_of::<u32>() + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_frame<T: DeserializeOwned>(payload: &[u8]) -> Result<T> {
    rmp_serde::from_slice(payload).context("failed to decode model worker frame")
}

fn frame_length(bytes: [u8; 4]) -> Result<usize> {
    let length = usize::try_from(u32::from_le_bytes(bytes))?;
    if length > MAX_FRAME_SIZE {
        bail!("model worker frame exceeds the size limit");
    }
    Ok(length)
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    writer.write_all(&encode_frame(value)?)?;
    Ok(())
}

fn read_optional_frame<R, T>(reader: &mut R) -> Result<Option<T>>
where
    R: Read,
    T: DeserializeOwned,
{
    let mut length = [0_u8; 4];
    if reader.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..])?;
    let mut payload = vec![0; frame_length(length)?];
    reader.read_exact(&mut payload)?;
    decode_frame(&payload).map(Some)
}

async fn write_async_frame<W, T>(writer: &mut W, value: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    writer.write_all(&encode_frame(value)?).await?;
    Ok(())
}

async fn read_async_frame<R, T>(reader: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length).await?;
    let mut payload = vec![0; frame_length(length)?];
    reader.read_exact(&mut payload).await?;
    decode_frame(&payload)
}

#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_command: &mut Command) {}
