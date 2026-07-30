use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, mpsc},
};

use koharu_canvas::{MaskCommit, MaskPlane, PixelRect};
use koharu_scene::PageId;

type Key = (PageId, MaskPlane);

#[derive(Debug)]
pub struct EncodedMask {
    pub page: PageId,
    pub plane: MaskPlane,
    pub dirty: PixelRect,
    pub generation: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
#[error("failed to encode {plane:?} mask generation {generation} for page {page}: {message}")]
pub struct MaskEncodingError {
    pub page: PageId,
    pub plane: MaskPlane,
    pub generation: u64,
    pub message: String,
}

#[derive(Debug)]
pub enum MaskEncodingResult {
    Ready(EncodedMask),
    Failed(MaskEncodingError),
}

pub(crate) struct MaskEncoder {
    active: HashSet<Key>,
    waiting: HashMap<Key, VecDeque<MaskCommit>>,
    sender: mpsc::Sender<(Key, MaskEncodingResult)>,
    receiver: mpsc::Receiver<(Key, MaskEncodingResult)>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl MaskEncoder {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            active: HashSet::new(),
            waiting: HashMap::new(),
            sender,
            receiver,
            wake,
        }
    }

    pub fn submit(&mut self, commit: MaskCommit) {
        let key = (commit.page, commit.plane);
        if self.active.insert(key) {
            self.spawn(key, commit);
        } else {
            self.waiting.entry(key).or_default().push_back(commit);
        }
    }

    pub fn drain(&mut self) -> Vec<MaskEncodingResult> {
        let mut completed = Vec::new();
        while let Ok((key, result)) = self.receiver.try_recv() {
            completed.push(result);
            if let Some(next) = self.waiting.get_mut(&key).and_then(VecDeque::pop_front) {
                self.spawn(key, next);
            } else {
                self.waiting.remove(&key);
                self.active.remove(&key);
            }
        }
        completed
    }

    fn spawn(&self, key: Key, commit: MaskCommit) {
        let sender = self.sender.clone();
        let wake = Arc::clone(&self.wake);
        rayon::spawn(move || {
            let result = match commit.encode_png() {
                Ok(bytes) => MaskEncodingResult::Ready(EncodedMask {
                    page: commit.page,
                    plane: commit.plane,
                    dirty: commit.dirty,
                    generation: commit.generation,
                    bytes,
                }),
                Err(error) => MaskEncodingResult::Failed(MaskEncodingError {
                    page: commit.page,
                    plane: commit.plane,
                    generation: commit.generation,
                    message: error.to_string(),
                }),
            };
            let _ = sender.send((key, result));
            wake();
        });
    }
}
