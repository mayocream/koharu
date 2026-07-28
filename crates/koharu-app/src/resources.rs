use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
};

use anyhow::{Context as _, Result, bail};
use image::imageops::FilterType;
use koharu_desktop::{CustomProtocol, ProtocolRequest, ProtocolResponse};
use koharu_scene::{Asset, BlobId, ProjectId, SceneSession, SceneSnapshot};
use url::Url;

const DEFAULT_WIDTH: u32 = 160;
const MIN_WIDTH: u32 = 32;
const MAX_WIDTH: u32 = 512;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct Resources {
    active: Arc<RwLock<Option<ActiveProject>>>,
    cache: Arc<Mutex<Cache>>,
}

#[derive(Clone)]
struct ActiveProject {
    id: ProjectId,
    path: PathBuf,
    allowed: HashSet<BlobId>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    project: ProjectId,
    blob: BlobId,
    width: u32,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<CacheKey, Arc<Vec<u8>>>,
    order: VecDeque<CacheKey>,
    bytes: usize,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(None)),
            cache: Arc::new(Mutex::new(Cache::default())),
        }
    }

    pub fn protocol(&self) -> CustomProtocol {
        let resources = self.clone();
        CustomProtocol::new("koharu-resource", move |request, responder| {
            let resources = resources.clone();
            rayon::spawn(move || responder.respond(resources.respond(&request)));
        })
    }

    pub fn install(&self, snapshot: &SceneSnapshot, path: &Path) -> Result<()> {
        let mut allowed = HashSet::new();
        const ROLES: &[&str] = &[
            "source",
            "clean",
            "rendered",
            "text-mask-candidate",
            "layout-text-mask",
            "text-mask",
            "coo-mask",
            "bubble-mask",
            "brush-mask",
        ];
        for entity in snapshot.entities() {
            for role in ROLES {
                if let Some(asset) = entity.component::<Asset>(*role)? {
                    allowed.insert(asset.blob);
                }
            }
        }
        *self
            .active
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(ActiveProject {
            id: snapshot.project_id(),
            path: path.to_owned(),
            allowed,
        });
        Ok(())
    }

    pub fn clear(&self) {
        *self
            .active
            .write()
            .unwrap_or_else(|error| error.into_inner()) = None;
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        cache.entries.clear();
        cache.order.clear();
        cache.bytes = 0;
    }

    fn respond(&self, request: &ProtocolRequest) -> ProtocolResponse {
        match self.thumbnail(request) {
            Ok(bytes) => ProtocolResponse::new(200, "image/webp", bytes)
                .with_header("Cache-Control", "private, max-age=31536000, immutable")
                .with_header("Access-Control-Allow-Origin", "*"),
            Err(ResourceError::BadRequest(message)) => {
                ProtocolResponse::new(400, "text/plain; charset=utf-8", message.into_bytes())
            }
            Err(ResourceError::Forbidden) => ProtocolResponse::new(
                403,
                "text/plain; charset=utf-8",
                b"resource is not available".to_vec(),
            ),
            Err(ResourceError::NotFound) => ProtocolResponse::new(
                404,
                "text/plain; charset=utf-8",
                b"resource was not found".to_vec(),
            ),
            Err(ResourceError::Internal(error)) => {
                tracing::warn!(%error, "failed to produce UI resource");
                ProtocolResponse::new(
                    500,
                    "text/plain; charset=utf-8",
                    b"resource could not be produced".to_vec(),
                )
            }
        }
    }

    fn thumbnail(&self, request: &ProtocolRequest) -> std::result::Result<Vec<u8>, ResourceError> {
        if request.method != "GET" {
            return Err(ResourceError::BadRequest("only GET is supported".into()));
        }
        let parsed = parse_request(&request.uri)?;
        let active = self
            .active
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or(ResourceError::NotFound)?;
        if parsed.project != active.id || !active.allowed.contains(&parsed.blob) {
            return Err(ResourceError::Forbidden);
        }
        let key = CacheKey {
            project: parsed.project,
            blob: parsed.blob,
            width: parsed.width,
        };
        if let Some(bytes) = self
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(key)
        {
            return Ok(bytes.as_ref().clone());
        }
        let bytes = encode_thumbnail(&active.path, parsed.blob, parsed.width)
            .map_err(ResourceError::Internal)?;
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(key, Arc::new(bytes.clone()));
        Ok(bytes)
    }
}

struct ParsedRequest {
    project: ProjectId,
    blob: BlobId,
    width: u32,
}

fn parse_request(uri: &str) -> std::result::Result<ParsedRequest, ResourceError> {
    let url =
        Url::parse(uri).map_err(|_| ResourceError::BadRequest("invalid resource URL".into()))?;
    if !matches!(url.host_str(), Some("project" | "koharu-resource.project")) {
        return Err(ResourceError::BadRequest("invalid resource host".into()));
    }
    let segments = url
        .path_segments()
        .ok_or_else(|| ResourceError::BadRequest("invalid resource path".into()))?
        .collect::<Vec<_>>();
    let [project, "blob", blob] = segments.as_slice() else {
        return Err(ResourceError::BadRequest("invalid resource path".into()));
    };
    let project = ProjectId::from_str(project)
        .map_err(|_| ResourceError::BadRequest("invalid project ID".into()))?;
    let blob =
        BlobId::from_str(blob).map_err(|_| ResourceError::BadRequest("invalid blob ID".into()))?;
    let width = url
        .query_pairs()
        .find_map(|(name, value)| (name == "width").then_some(value))
        .map_or(Ok(DEFAULT_WIDTH), |value| {
            value
                .parse::<u32>()
                .map_err(|_| ResourceError::BadRequest("invalid thumbnail width".into()))
        })?;
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return Err(ResourceError::BadRequest(
            "thumbnail width is outside the supported range".into(),
        ));
    }
    Ok(ParsedRequest {
        project,
        blob,
        width,
    })
}

fn encode_thumbnail(path: &Path, blob: BlobId, width: u32) -> Result<Vec<u8>> {
    let session = SceneSession::open(path)
        .with_context(|| format!("failed to open resource project {}", path.display()))?;
    let source = session.snapshot().read_blob(blob)?;
    let image = image::load_from_memory(&source).context("failed to decode thumbnail image")?;
    if image.width() == 0 || image.height() == 0 {
        bail!("thumbnail source is empty");
    }
    let target_width = width.min(image.width());
    let target_height = ((u64::from(image.height()) * u64::from(target_width))
        .div_ceil(u64::from(image.width())))
    .clamp(1, u64::from(MAX_WIDTH)) as u32;
    let resized = image.resize_exact(target_width, target_height, FilterType::Lanczos3);
    let rgba = resized.to_rgba8();
    let encoder = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
    Ok(encoder.encode(80.0).to_vec())
}

impl Cache {
    fn get(&mut self, key: CacheKey) -> Option<Arc<Vec<u8>>> {
        let value = self.entries.get(&key)?.clone();
        self.order.retain(|candidate| *candidate != key);
        self.order.push_back(key);
        Some(value)
    }

    fn insert(&mut self, key: CacheKey, value: Arc<Vec<u8>>) {
        if value.len() > MAX_CACHE_BYTES {
            return;
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(previous.len());
            self.order.retain(|candidate| *candidate != key);
        }
        self.bytes += value.len();
        self.entries.insert(key, value);
        self.order.push_back(key);
        while self.bytes > MAX_CACHE_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(value) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(value.len());
            }
        }
    }
}

enum ResourceError {
    BadRequest(String),
    Forbidden,
    NotFound,
    Internal(anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_id() -> ProjectId {
        SceneSession::memory().unwrap().project_id()
    }

    #[test]
    fn rejects_bad_hosts_and_widths() {
        assert!(matches!(
            parse_request("koharu-resource://files/a/blob/b"),
            Err(ResourceError::BadRequest(_))
        ));
        let project = project_id();
        let blob = BlobId::for_bytes(b"image");
        assert!(matches!(
            parse_request(&format!(
                "koharu-resource://project/{project}/blob/{blob}?width=900"
            )),
            Err(ResourceError::BadRequest(_))
        ));
        assert!(matches!(
            parse_request(&format!(
                "koharu-resource://project/{project}/../blob/{blob}"
            )),
            Err(ResourceError::BadRequest(_))
        ));
        assert!(matches!(
            parse_request(&format!(
                "koharu-resource://project/{project}/blob/{blob}/extra"
            )),
            Err(ResourceError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_unreferenced_blobs_before_opening_project_storage() {
        let resources = Resources::new();
        let project = project_id();
        let blob = BlobId::for_bytes(b"unreferenced");
        *resources
            .active
            .write()
            .expect("resource project lock poisoned") = Some(ActiveProject {
            id: project,
            path: PathBuf::from("does-not-exist.khr"),
            allowed: HashSet::new(),
        });
        let request = ProtocolRequest {
            method: "GET".into(),
            uri: format!("koharu-resource://project/{project}/blob/{blob}"),
        };
        assert!(matches!(
            resources.thumbnail(&request),
            Err(ResourceError::Forbidden)
        ));
    }

    #[test]
    fn cache_is_bounded_and_refreshes_recency() {
        let mut cache = Cache::default();
        let key = CacheKey {
            project: project_id(),
            blob: BlobId::for_bytes(b"one"),
            width: 160,
        };
        cache.insert(key, Arc::new(vec![1, 2, 3]));
        assert_eq!(cache.get(key).unwrap().as_slice(), &[1, 2, 3]);
        assert!(cache.bytes <= MAX_CACHE_BYTES);
        let oversized = CacheKey {
            project: key.project,
            blob: BlobId::for_bytes(b"oversized"),
            width: 512,
        };
        cache.insert(oversized, Arc::new(vec![0; MAX_CACHE_BYTES + 1]));
        assert!(!cache.entries.contains_key(&oversized));
    }
}
