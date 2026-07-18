use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, mpsc},
};

use image::GrayImage;
use koharu_renderer::BubbleIndex;
use koharu_scene::BlobId;
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ResourceKind {
    Color,
    Gray,
    Bubble,
}

#[derive(Clone)]
struct CacheEntry<T> {
    value: T,
    bytes: usize,
    used: u64,
}

pub(crate) enum ResourceEvent {
    Ready {
        id: BlobId,
        kind: ResourceKind,
    },
    Failed {
        id: BlobId,
        kind: ResourceKind,
        message: String,
    },
}

enum Decoded {
    Color(ImageData, usize),
    Gray(Arc<GrayImage>, usize),
    Bubble(Arc<BubbleIndex>, usize),
}

struct DecodeResult {
    id: BlobId,
    kind: ResourceKind,
    result: std::result::Result<Decoded, String>,
}

pub(crate) struct Resources {
    color: HashMap<BlobId, CacheEntry<ImageData>>,
    gray: HashMap<BlobId, CacheEntry<Arc<GrayImage>>>,
    bubbles: HashMap<BlobId, CacheEntry<Arc<BubbleIndex>>>,
    loading: HashSet<(BlobId, ResourceKind)>,
    sender: mpsc::Sender<DecodeResult>,
    receiver: mpsc::Receiver<DecodeResult>,
    wake: Arc<dyn Fn() + Send + Sync>,
    max_bytes: usize,
    bytes: usize,
    clock: u64,
}

impl Resources {
    pub fn new(max_bytes: usize, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            color: HashMap::new(),
            gray: HashMap::new(),
            bubbles: HashMap::new(),
            loading: HashSet::new(),
            sender,
            receiver,
            wake,
            max_bytes,
            bytes: 0,
            clock: 0,
        }
    }

    pub fn request(&mut self, id: BlobId, kind: ResourceKind, bytes: Arc<[u8]>) {
        let ready = match kind {
            ResourceKind::Color => self.color.contains_key(&id),
            ResourceKind::Gray => self.gray.contains_key(&id),
            ResourceKind::Bubble => self.bubbles.contains_key(&id),
        };
        if ready || !self.loading.insert((id, kind)) {
            return;
        }
        let sender = self.sender.clone();
        let wake = Arc::clone(&self.wake);
        rayon::spawn(move || {
            let result = decode(kind, &bytes).map_err(|error| error.to_string());
            let _ = sender.send(DecodeResult { id, kind, result });
            wake();
        });
    }

    pub fn drain(&mut self, active: &HashSet<BlobId>) -> Vec<ResourceEvent> {
        let mut events = Vec::new();
        while let Ok(decoded) = self.receiver.try_recv() {
            self.loading.remove(&(decoded.id, decoded.kind));
            match decoded.result {
                Ok(Decoded::Color(image, bytes)) => {
                    self.clock = self.clock.wrapping_add(1);
                    if let Some(previous) = self.color.insert(
                        decoded.id,
                        CacheEntry {
                            value: image,
                            bytes,
                            used: self.clock,
                        },
                    ) {
                        self.bytes = self.bytes.saturating_sub(previous.bytes);
                    }
                    self.bytes = self.bytes.saturating_add(bytes);
                    events.push(ResourceEvent::Ready {
                        id: decoded.id,
                        kind: decoded.kind,
                    });
                }
                Ok(Decoded::Gray(image, bytes)) => {
                    self.clock = self.clock.wrapping_add(1);
                    if let Some(previous) = self.gray.insert(
                        decoded.id,
                        CacheEntry {
                            value: image,
                            bytes,
                            used: self.clock,
                        },
                    ) {
                        self.bytes = self.bytes.saturating_sub(previous.bytes);
                    }
                    self.bytes = self.bytes.saturating_add(bytes);
                    events.push(ResourceEvent::Ready {
                        id: decoded.id,
                        kind: decoded.kind,
                    });
                }
                Ok(Decoded::Bubble(index, bytes)) => {
                    self.clock = self.clock.wrapping_add(1);
                    if let Some(previous) = self.bubbles.insert(
                        decoded.id,
                        CacheEntry {
                            value: index,
                            bytes,
                            used: self.clock,
                        },
                    ) {
                        self.bytes = self.bytes.saturating_sub(previous.bytes);
                    }
                    self.bytes = self.bytes.saturating_add(bytes);
                    events.push(ResourceEvent::Ready {
                        id: decoded.id,
                        kind: decoded.kind,
                    });
                }
                Err(message) => events.push(ResourceEvent::Failed {
                    id: decoded.id,
                    kind: decoded.kind,
                    message,
                }),
            }
        }
        self.evict(active);
        events
    }

    pub fn color(&mut self, id: BlobId) -> Option<ImageData> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.color.get_mut(&id)?;
        entry.used = self.clock;
        Some(entry.value.clone())
    }

    pub fn gray(&mut self, id: BlobId) -> Option<Arc<GrayImage>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.gray.get_mut(&id)?;
        entry.used = self.clock;
        Some(Arc::clone(&entry.value))
    }

    pub fn bubble(&mut self, id: BlobId) -> Option<Arc<BubbleIndex>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.bubbles.get_mut(&id)?;
        entry.used = self.clock;
        Some(Arc::clone(&entry.value))
    }

    pub fn contains(&self, id: BlobId, kind: ResourceKind) -> bool {
        match kind {
            ResourceKind::Color => self.color.contains_key(&id),
            ResourceKind::Gray => self.gray.contains_key(&id),
            ResourceKind::Bubble => self.bubbles.contains_key(&id),
        }
    }

    fn evict(&mut self, active: &HashSet<BlobId>) {
        while self.bytes > self.max_bytes {
            let color = self
                .color
                .iter()
                .filter(|(id, _)| !active.contains(id))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(id, entry)| (*id, entry.used, ResourceKind::Color));
            let gray = self
                .gray
                .iter()
                .filter(|(id, _)| !active.contains(id))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(id, entry)| (*id, entry.used, ResourceKind::Gray));
            let bubble = self
                .bubbles
                .iter()
                .filter(|(id, _)| !active.contains(id))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(id, entry)| (*id, entry.used, ResourceKind::Bubble));
            let candidate = [color, gray, bubble]
                .into_iter()
                .flatten()
                .min_by_key(|(_, used, _)| *used);
            let Some((id, _, kind)) = candidate else {
                break;
            };
            let removed = match kind {
                ResourceKind::Color => self.color.remove(&id).map(|entry| entry.bytes),
                ResourceKind::Gray => self.gray.remove(&id).map(|entry| entry.bytes),
                ResourceKind::Bubble => self.bubbles.remove(&id).map(|entry| entry.bytes),
            };
            if let Some(bytes) = removed {
                self.bytes = self.bytes.saturating_sub(bytes);
            }
        }
    }
}

fn decode(kind: ResourceKind, bytes: &[u8]) -> image::ImageResult<Decoded> {
    let image = image::load_from_memory(bytes)?;
    Ok(match kind {
        ResourceKind::Color => {
            let rgba = image.into_rgba8();
            let width = rgba.width();
            let height = rgba.height();
            let pixels = rgba.into_raw();
            let bytes = pixels.len();
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(pixels);
            Decoded::Color(
                ImageData {
                    data: Blob::new(data),
                    format: ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::Alpha,
                    width,
                    height,
                },
                bytes,
            )
        }
        ResourceKind::Gray => {
            let gray = Arc::new(image.into_luma8());
            let bytes = gray.len();
            Decoded::Gray(gray, bytes)
        }
        ResourceKind::Bubble => {
            let gray = image.into_luma8();
            let bytes = gray.len();
            Decoded::Bubble(Arc::new(BubbleIndex::new(gray)), bytes)
        }
    })
}
