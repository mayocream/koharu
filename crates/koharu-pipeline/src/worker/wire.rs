use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, bail};
use koharu_scene::{BlobId, Command, CommandParts, Commands, Page, Revision};
use koharu_worker::{ArenaDescriptor, ArenaFile, MappedArena, SharedSlice};
use serde::{Deserialize, Serialize};

use crate::{
    BlobBytes, CancellationToken, Context, Scope, WorkerState,
    context::{ContextOptions, TranslationOptions},
    plan::ConfiguredModel,
};

#[derive(Deserialize, Serialize)]
pub(super) struct ModelRequest {
    pub(super) model: ConfiguredModel,
    pub(super) device: koharu_ml::Device,
    pub(super) shared_root: PathBuf,
    pub(super) context: WireContext,
    pub(super) blobs: SharedBlobs,
}

#[derive(Deserialize, Serialize)]
pub(super) struct ModelResponse {
    pub(super) commands: WireCommands,
    pub(super) attachments: SharedBlobs,
    pub(super) load_micros: Option<u64>,
    pub(super) processor_micros: u64,
    pub(super) input_bytes: usize,
    pub(super) output_bytes: usize,
}

#[derive(Deserialize, Serialize)]
pub(super) enum ModelEvent {
    State(WorkerState),
    Download(koharu_runtime::download::Event),
}

#[derive(Deserialize, Serialize)]
pub(super) struct WireContext {
    revision: Revision,
    scope: Scope,
    pages: Vec<Page>,
    target_language: String,
    instructions: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct WireCommands {
    base: Revision,
    ops: Vec<Command>,
}

#[derive(Default, Deserialize, Serialize)]
pub(super) struct SharedBlobs {
    pub(super) arena: Option<ArenaDescriptor>,
    pub(super) entries: Vec<(BlobId, SharedSlice)>,
}

impl WireContext {
    pub(super) fn from_context(context: &Context) -> Self {
        Self {
            revision: context.revision(),
            scope: context.scope().clone(),
            pages: context.pages().to_vec(),
            target_language: context.target_language().to_owned(),
            instructions: context.instructions().map(str::to_owned),
        }
    }

    pub(super) fn into_context(self, blobs: &SharedBlobs, root: &Path) -> Result<Context> {
        Ok(Context::new(
            self.revision,
            self.scope,
            self.pages,
            blobs.share(root)?,
            Arc::new(Mutex::new(HashMap::new())),
            ContextOptions {
                translation: TranslationOptions {
                    target_language: self.target_language,
                    instructions: self.instructions,
                },
                cancellation: CancellationToken::default(),
                events: None,
                measurements: Arc::new(Mutex::new(Vec::new())),
            },
        ))
    }
}

impl WireCommands {
    pub(super) fn from_commands(
        commands: Commands,
        root: &Path,
    ) -> Result<(Self, SharedBlobs, usize)> {
        let CommandParts {
            base,
            ops,
            attachments,
        } = commands.into_parts();
        let (attachments, output_bytes) = SharedBlobs::persist(root, attachments)?;
        Ok((Self { base, ops }, attachments, output_bytes))
    }

    pub(super) fn into_commands(self, attachments: SharedBlobs, root: &Path) -> Result<Commands> {
        Ok(Commands::from_parts(CommandParts {
            base: self.base,
            ops: self.ops,
            attachments: attachments.copy(root)?,
        })?)
    }
}

impl SharedBlobs {
    pub(super) fn byte_len(&self) -> Result<usize> {
        self.entries.iter().try_fold(0_usize, |total, (_, slice)| {
            total
                .checked_add(usize::try_from(slice.length)?)
                .context("shared blob size overflowed")
        })
    }

    fn open(&self, root: &Path, delete_on_drop: bool) -> Result<Option<MappedArena>> {
        match (self.entries.is_empty(), self.arena.as_ref()) {
            (true, None) => Ok(None),
            (true, Some(_)) => bail!("shared blob transfer contains an unused arena"),
            (false, None) => bail!("shared blob transfer omitted its arena"),
            (false, Some(arena)) => MappedArena::open(arena, root, delete_on_drop).map(Some),
        }
    }

    fn share(&self, root: &Path) -> Result<HashMap<BlobId, BlobBytes>> {
        let Some(arena) = self.open(root, false)? else {
            return Ok(HashMap::new());
        };
        self.entries
            .iter()
            .map(|(id, slice)| {
                arena
                    .slice(*slice)
                    .map(|bytes| (*id, BlobBytes::Shared(bytes)))
            })
            .collect()
    }

    fn copy(&self, root: &Path) -> Result<Vec<(BlobId, Arc<[u8]>)>> {
        let Some(arena) = self.open(root, true)? else {
            return Ok(Vec::new());
        };
        self.entries
            .iter()
            .map(|(id, slice)| {
                let bytes = arena.slice(*slice)?;
                Ok((*id, Arc::from(bytes.as_ref())))
            })
            .collect()
    }

    fn persist(root: &Path, blobs: Vec<(BlobId, Arc<[u8]>)>) -> Result<(Self, usize)> {
        let output_bytes = blobs.iter().try_fold(0_usize, |total, (_, bytes)| {
            total
                .checked_add(bytes.len())
                .context("shared blob size overflowed")
        })?;
        if blobs.is_empty() {
            return Ok((Self::default(), output_bytes));
        }
        let (file, slices) =
            ArenaFile::create(root, blobs.iter().map(|(_, bytes)| bytes.as_ref()))?;
        let arena = file.persist()?;
        let entries = blobs.into_iter().map(|(id, _)| id).zip(slices).collect();
        Ok((
            Self {
                arena: Some(arena),
                entries,
            },
            output_bytes,
        ))
    }
}
