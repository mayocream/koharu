//! Small subprocess RPC transport with file-backed shared memory.

mod shared;

use std::{
    ffi::OsStr,
    future::Future,
    marker::PhantomData,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    process::Command,
    sync::mpsc,
};

pub use shared::{ArenaDescriptor, ArenaFile, MappedArena, SharedBytes, SharedSlice};

const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CallMetrics {
    pub generation: u64,
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub round_trip: std::time::Duration,
}

#[derive(Debug)]
pub struct CallResult<T> {
    pub value: T,
    pub metrics: CallMetrics,
}

#[derive(Debug, Error)]
pub enum CallError {
    #[error("worker request was cancelled")]
    Cancelled,
    #[error("worker returned an error: {0}")]
    Remote(String),
    #[error("worker process stopped: {0}")]
    Crashed(String),
}

#[derive(Serialize, Deserialize)]
enum WorkerMessage<E, T> {
    Event(E),
    Finished(T),
    Failed(String),
}

pub struct Client {
    generation: u64,
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
}

impl Client {
    pub async fn spawn(argument: impl AsRef<OsStr>) -> Result<Self> {
        let executable =
            std::env::current_exe().context("failed to locate the Koharu executable")?;
        Self::spawn_executable(executable, argument).await
    }

    pub async fn spawn_executable(
        executable: impl AsRef<OsStr>,
        argument: impl AsRef<OsStr>,
    ) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .arg(argument)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        hide_window(&mut command);

        let mut child = command
            .spawn()
            .context("failed to start the worker process")?;
        let stdin = child
            .stdin
            .take()
            .context("worker stdin was not available")?;
        let stdout = child
            .stdout
            .take()
            .context("worker stdout was not available")?;
        Ok(Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            child,
            stdin,
            stdout,
        })
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub async fn call<Request, Response, Event, Cancel, OnEvent>(
        &mut self,
        request: &Request,
        cancelled: Cancel,
        mut on_event: OnEvent,
    ) -> std::result::Result<CallResult<Response>, CallError>
    where
        Request: Serialize,
        Response: DeserializeOwned,
        Event: DeserializeOwned,
        Cancel: Future<Output = ()>,
        OnEvent: FnMut(Event),
    {
        let request_bytes = match write_async_frame(&mut self.stdin, request).await {
            Ok(bytes) => bytes,
            Err(error) => {
                self.stop_in_place().await;
                return Err(CallError::Crashed(format!(
                    "failed to send request: {error:#}"
                )));
            }
        };
        let started = Instant::now();
        let generation = self.generation;
        let mut response_bytes = 0_usize;
        tokio::pin!(cancelled);

        loop {
            enum Wait<T> {
                Cancelled,
                Read(Result<T>),
            }
            let outcome = {
                let read = read_async_frame::<_, WorkerMessage<Event, Response>>(&mut self.stdout);
                tokio::pin!(read);
                tokio::select! {
                    biased;
                    () = &mut cancelled => Wait::Cancelled,
                    result = &mut read => Wait::Read(result),
                }
            };
            let (message, bytes) = match outcome {
                Wait::Cancelled => {
                    self.stop_in_place().await;
                    return Err(CallError::Cancelled);
                }
                Wait::Read(Ok(message)) => message,
                Wait::Read(Err(error)) => {
                    let status = self.exit_description();
                    self.stop_in_place().await;
                    return Err(CallError::Crashed(format!(
                        "{status}; failed to read response: {error:#}"
                    )));
                }
            };
            response_bytes = response_bytes.saturating_add(bytes);

            match message {
                WorkerMessage::Event(event) => on_event(event),
                WorkerMessage::Finished(value) => {
                    return Ok(CallResult {
                        value,
                        metrics: CallMetrics {
                            generation,
                            request_bytes,
                            response_bytes,
                            round_trip: started.elapsed(),
                        },
                    });
                }
                WorkerMessage::Failed(error) => return Err(CallError::Remote(error)),
            }
        }
    }

    pub async fn shutdown(mut self) {
        self.stop_in_place().await;
    }

    async fn stop_in_place(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    fn exit_description(&mut self) -> String {
        match self.child.try_wait() {
            Ok(Some(status)) => format!("exit status {status}"),
            Ok(None) => "output pipe closed while the process was running".into(),
            Err(error) => format!("exit status unavailable: {error}"),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[async_trait]
pub trait Handler: Send {
    type Request: DeserializeOwned + Send;
    type Response: Serialize + Send;
    type Event: Serialize + Send + Sync + 'static;

    async fn handle(
        &mut self,
        request: Self::Request,
        events: Emitter<Self::Event>,
    ) -> Result<Self::Response>;
}

pub struct Emitter<E> {
    output: mpsc::UnboundedSender<Vec<u8>>,
    marker: PhantomData<fn(E)>,
}

impl<E> Clone for Emitter<E> {
    fn clone(&self) -> Self {
        Self {
            output: self.output.clone(),
            marker: PhantomData,
        }
    }
}

impl<E: Serialize> Emitter<E> {
    pub fn emit(&self, event: E) -> Result<()> {
        let frame = encode_frame(&WorkerMessage::<E, ()>::Event(event))?;
        self.output
            .send(frame)
            .map_err(|_| anyhow!("worker output has closed"))
    }
}

pub async fn serve<H: Handler>(mut handler: H) -> Result<()> {
    let mut input = tokio::io::stdin();
    let (output, receiver) = mpsc::unbounded_channel();
    let writer = tokio::spawn(write_output(receiver));

    while let Some(request) = read_optional_async_frame::<_, H::Request>(&mut input)
        .await
        .context("failed to read worker request")?
    {
        let emitter = Emitter {
            output: output.clone(),
            marker: PhantomData,
        };
        let terminal: WorkerMessage<H::Event, H::Response> =
            match handler.handle(request, emitter).await {
                Ok(response) => WorkerMessage::Finished(response),
                Err(error) => WorkerMessage::Failed(format!("{error:#}")),
            };
        output
            .send(encode_frame(&terminal)?)
            .map_err(|_| anyhow!("worker output has closed"))?;
    }
    drop(output);
    writer.await.context("worker output task stopped")??;
    Ok(())
}

fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let payload = rmp_serde::to_vec_named(value).context("failed to encode worker frame")?;
    if payload.len() > MAX_FRAME_SIZE {
        bail!("worker frame exceeds the size limit");
    }
    let length = u32::try_from(payload.len()).context("worker frame is too large")?;
    let mut frame = Vec::with_capacity(size_of::<u32>() + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_frame<T: DeserializeOwned>(payload: &[u8]) -> Result<T> {
    rmp_serde::from_slice(payload).context("failed to decode worker frame")
}

fn frame_length(bytes: [u8; 4]) -> Result<usize> {
    let length = usize::try_from(u32::from_le_bytes(bytes))?;
    if length > MAX_FRAME_SIZE {
        bail!("worker frame exceeds the size limit");
    }
    Ok(length)
}

async fn write_output(mut receiver: mpsc::UnboundedReceiver<Vec<u8>>) -> Result<()> {
    let mut output = tokio::io::stdout();
    while let Some(frame) = receiver.recv().await {
        output.write_all(&frame).await?;
        output.flush().await?;
    }
    Ok(())
}

async fn read_optional_async_frame<R, T>(reader: &mut R) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut length = [0_u8; 4];
    if reader.read(&mut length[..1]).await? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..]).await?;
    let mut payload = vec![0; frame_length(length)?];
    reader.read_exact(&mut payload).await?;
    decode_frame(&payload).map(Some)
}

async fn write_async_frame<W, T>(writer: &mut W, value: &T) -> Result<usize>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let frame = encode_frame(value)?;
    let bytes = frame.len();
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(bytes)
}

async fn read_async_frame<R, T>(reader: &mut R) -> Result<(T, usize)>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length).await?;
    let length = frame_length(length)?;
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).await?;
    Ok((decode_frame(&payload)?, size_of::<u32>() + length))
}

#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    enum TestEvent {
        Ready,
    }

    #[test]
    fn event_frames_do_not_depend_on_the_response_type() {
        let encoded =
            encode_frame(&WorkerMessage::<TestEvent, ()>::Event(TestEvent::Ready)).unwrap();
        let decoded: WorkerMessage<TestEvent, String> =
            decode_frame(&encoded[size_of::<u32>()..]).unwrap();

        assert!(matches!(decoded, WorkerMessage::Event(TestEvent::Ready)));
    }
}
