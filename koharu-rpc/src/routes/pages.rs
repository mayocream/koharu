//! Page + page-subresource byte-ingress routes.
//!
//! - `POST /pages`                           — multipart: create pages from N image files
//! - `POST /pages/{id}/image-layers`         — multipart: add one Custom image node
//! - `PUT  /pages/{id}/masks/{role}`         — raw PNG body: upsert a mask node
//!   (role ∈ `segment`, `brushInpaint`)
//!
//! All three do the same server-side dance: read bytes → `blobs.put_bytes`
//! → emit an `Op` on the session history.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Multipart, Path, Query, State};
use image::GenericImageView;
use koharu_app::pipeline::{self, EngineCtx, PipelineRunOptions};
use koharu_core::{
    BlobRef, ImageData, ImageRole, MaskRole, Node, NodeDataPatch, NodeId, NodeKind, Op, Page,
    PageId, Region, Scene, Transform,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct PutMaskParams {
    /// Optional pipeline engine to run after the mask is updated.
    pub pipeline: Option<String>,
    /// Bounding box for the pipeline run.
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(create_pages))
        .routes(routes!(create_pages_from_paths))
        .routes(routes!(add_image_layer))
        .routes(routes!(put_mask))
}

// ---------------------------------------------------------------------------
// POST /pages  — create pages from uploaded image files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePagesResponse {
    pub pages: Vec<PageId>,
}

#[utoipa::path(
    post,
    path = "/pages",
    request_body(content_type = "multipart/form-data"),
    responses((status = 200, body = CreatePagesResponse))
)]
async fn create_pages(
    State(app): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<Json<CreatePagesResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;

    // Collect (filename, bytes) pairs first so we can sort naturally.
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut replace = false;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "replace" {
            let text = field
                .text()
                .await
                .map_err(|e| ApiError::bad_request(format!("{e}")))?;
            replace = text == "true" || text == "1";
            continue;
        }
        let filename = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("page-{}.bin", files.len() + 1));
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request(format!("read file: {e}")))?;
        files.push((filename, bytes.to_vec()));
    }

    files.sort_by(|a, b| natord::compare(&a.0, &b.0));

    // Optionally clear the project first. Emitted as a batch so it's one undo step.
    let starting_index = if replace {
        let scene = session.scene.read();
        let remove_ops: Vec<Op> = scene
            .pages
            .keys()
            .copied()
            .map(|id| Op::RemovePage {
                id,
                prev_page: scene.pages[&id].clone(),
                prev_index: scene.pages.get_index_of(&id).unwrap_or(0),
            })
            .collect();
        drop(scene);
        if !remove_ops.is_empty() {
            app.apply(Op::Batch {
                ops: remove_ops,
                label: "Replace pages (clear)".into(),
            })
            .map_err(ApiError::internal)?;
        }
        0
    } else {
        session.scene.read().pages.len()
    };

    // Decode + hash + write each file in parallel. Image decode is the
    // dominant cost per page (~10–50ms for a typical JPEG/PNG), so a
    // 200-page folder benefits almost linearly from multi-core. The output
    // vector preserves the pre-sorted order because rayon's `par_iter`
    // keeps indices through `collect::<Result<Vec<_>>>()`.
    //
    // `BlobStore::put_bytes` is Send + Sync (stateless beyond disk + blake3),
    // so it's safe to call from the rayon pool.
    //
    // Run the rayon section on a blocking thread so we don't stall the
    // tokio runtime while decoding.
    let blobs = session.blobs.clone();
    let decoded: Vec<(String, u32, u32, BlobRef)> = tokio::task::spawn_blocking(move || {
        files
            .into_par_iter()
            .map(
                |(filename, bytes)| -> ApiResult<(String, u32, u32, BlobRef)> {
                    let img = image::load_from_memory(&bytes)
                        .map_err(|e| ApiError::bad_request(format!("decode `{filename}`: {e}")))?;
                    let (w, h) = img.dimensions();
                    let blob = blobs.put_bytes(&bytes).map_err(ApiError::internal)?;
                    Ok((filename, w, h, blob))
                },
            )
            .collect::<ApiResult<Vec<_>>>()
    })
    .await
    .map_err(|e| ApiError::internal(anyhow::anyhow!("import task panicked: {e}")))??;

    // Build one AddPage batch for the whole import.
    let mut ops = Vec::with_capacity(decoded.len());
    let mut created_ids = Vec::with_capacity(decoded.len());
    for (i, (filename, w, h, blob)) in decoded.into_iter().enumerate() {
        let mut page = Page::new(&filename, w, h);
        let page_id = page.id;
        let source_node_id = NodeId::new();
        page.nodes.insert(
            source_node_id,
            Node {
                id: source_node_id,
                transform: Transform::default(),
                visible: true,
                kind: NodeKind::Image(ImageData {
                    role: ImageRole::Source,
                    blob,
                    opacity: 1.0,
                    natural_width: w,
                    natural_height: h,
                    name: Some(filename),
                }),
            },
        );
        created_ids.push(page_id);
        ops.push(Op::AddPage {
            page,
            at: starting_index + i,
        });
    }

    app.apply(Op::Batch {
        ops,
        label: "Import pages".into(),
    })
    .map_err(ApiError::internal)?;

    Ok(Json(CreatePagesResponse { pages: created_ids }))
}

// ---------------------------------------------------------------------------
// POST /pages/from-paths — Tauri fast-path: import by reading files directly
// from disk, skipping multipart upload entirely
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePagesFromPathsRequest {
    pub paths: Vec<String>,
    #[serde(default)]
    pub replace: bool,
}

/// Create pages by reading image files from absolute paths on the server's
/// filesystem. This is the Tauri desktop import path — the webview picker
/// returns paths, and the backend reads + decodes + hashes them in parallel
/// without a round-trip through JS memory or a multipart upload body.
///
/// Web clients should keep using `POST /pages` with multipart.
#[utoipa::path(
    post,
    path = "/pages/from-paths",
    request_body = CreatePagesFromPathsRequest,
    responses((status = 200, body = CreatePagesResponse))
)]
async fn create_pages_from_paths(
    State(app): State<AppState>,
    Json(req): Json<CreatePagesFromPathsRequest>,
) -> ApiResult<Json<CreatePagesResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;

    // Natural-order sort by filename component so `page-2.png` < `page-10.png`.
    let mut paths = req.paths;
    paths.sort_by(|a, b| {
        let af = std::path::Path::new(a)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(a);
        let bf = std::path::Path::new(b)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(b);
        natord::compare(af, bf)
    });

    let starting_index = if req.replace {
        let scene = session.scene.read();
        let remove_ops: Vec<Op> = scene
            .pages
            .keys()
            .copied()
            .map(|id| Op::RemovePage {
                id,
                prev_page: scene.pages[&id].clone(),
                prev_index: scene.pages.get_index_of(&id).unwrap_or(0),
            })
            .collect();
        drop(scene);
        if !remove_ops.is_empty() {
            app.apply(Op::Batch {
                ops: remove_ops,
                label: "Replace pages (clear)".into(),
            })
            .map_err(ApiError::internal)?;
        }
        0
    } else {
        session.scene.read().pages.len()
    };

    let blobs = session.blobs.clone();
    let decoded: Vec<(String, u32, u32, BlobRef)> = tokio::task::spawn_blocking(move || {
        paths
            .into_par_iter()
            .map(|path| -> ApiResult<(String, u32, u32, BlobRef)> {
                let filename = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "page.bin".to_string());
                let bytes = std::fs::read(&path)
                    .map_err(|e| ApiError::bad_request(format!("read `{filename}`: {e}")))?;
                let img = image::load_from_memory(&bytes)
                    .map_err(|e| ApiError::bad_request(format!("decode `{filename}`: {e}")))?;
                let (w, h) = img.dimensions();
                let blob = blobs.put_bytes(&bytes).map_err(ApiError::internal)?;
                Ok((filename, w, h, blob))
            })
            .collect::<ApiResult<Vec<_>>>()
    })
    .await
    .map_err(|e| ApiError::internal(anyhow::anyhow!("import task panicked: {e}")))??;

    let mut ops = Vec::with_capacity(decoded.len());
    let mut created_ids = Vec::with_capacity(decoded.len());
    for (i, (filename, w, h, blob)) in decoded.into_iter().enumerate() {
        let mut page = Page::new(&filename, w, h);
        let page_id = page.id;
        let source_node_id = NodeId::new();
        page.nodes.insert(
            source_node_id,
            Node {
                id: source_node_id,
                transform: Transform::default(),
                visible: true,
                kind: NodeKind::Image(ImageData {
                    role: ImageRole::Source,
                    blob,
                    opacity: 1.0,
                    natural_width: w,
                    natural_height: h,
                    name: Some(filename),
                }),
            },
        );
        created_ids.push(page_id);
        ops.push(Op::AddPage {
            page,
            at: starting_index + i,
        });
    }

    app.apply(Op::Batch {
        ops,
        label: "Import pages".into(),
    })
    .map_err(ApiError::internal)?;

    Ok(Json(CreatePagesResponse { pages: created_ids }))
}

// ---------------------------------------------------------------------------
// POST /pages/{id}/image-layers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddImageLayerResponse {
    pub node: NodeId,
}

#[utoipa::path(
    post,
    path = "/pages/{id}/image-layers",
    params(("id" = PageId, Path, description = "Page id")),
    request_body(content_type = "multipart/form-data"),
    responses((status = 200, body = AddImageLayerResponse))
)]
async fn add_image_layer(
    State(app): State<AppState>,
    Path(page_id): Path<PageId>,
    mut multipart: Multipart,
) -> ApiResult<Json<AddImageLayerResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let page_node_count = {
        let scene = session.scene.read();
        scene
            .page(page_id)
            .ok_or_else(|| ApiError::not_found(format!("page {page_id}")))?
            .nodes
            .len()
    };

    // The handler only accepts a single image layer per request, so we
    // pull the first multipart field and ignore the rest.
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("multipart: {e}")))?
        .ok_or_else(|| ApiError::bad_request("no file uploaded"))?;
    let filename = field
        .file_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| String::from("layer.png"));
    let bytes = field
        .bytes()
        .await
        .map_err(|e| ApiError::bad_request(format!("read file: {e}")))?
        .to_vec();

    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| ApiError::bad_request(format!("decode: {e}")))?;
    let (w, h) = decoded.dimensions();
    let blob = session
        .blobs
        .put_bytes(&bytes)
        .map_err(ApiError::internal)?;

    // Center-place on the page.
    let (center_x, center_y) = center_on_page(session.scene.read().page(page_id), w, h);
    let node_id = NodeId::new();
    let node = Node {
        id: node_id,
        transform: Transform {
            x: center_x,
            y: center_y,
            width: w as f32,
            height: h as f32,
            rotation_deg: 0.0,
        },
        visible: true,
        kind: NodeKind::Image(ImageData {
            role: ImageRole::Custom,
            blob,
            opacity: 1.0,
            natural_width: w,
            natural_height: h,
            name: Some(filename),
        }),
    };
    app.apply(Op::AddNode {
        page: page_id,
        node,
        at: page_node_count,
    })
    .map_err(ApiError::internal)?;

    Ok(Json(AddImageLayerResponse { node: node_id }))
}

fn center_on_page(page: Option<&koharu_core::Page>, iw: u32, ih: u32) -> (f32, f32) {
    let Some(p) = page else { return (0.0, 0.0) };
    let x = ((p.width as f32) - iw as f32) / 2.0;
    let y = ((p.height as f32) - ih as f32) / 2.0;
    (x.max(0.0), y.max(0.0))
}

#[allow(dead_code)]
fn scene_contains_page(scene: &Scene, id: PageId) -> bool {
    scene.pages.contains_key(&id)
}

// ---------------------------------------------------------------------------
// PUT /pages/{id}/masks/{role}
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PutMaskResponse {
    pub node: NodeId,
    pub blob: BlobRef,
}

/// Upsert the `Mask { role }` node on a page with the raw image bytes in
/// the body. Emits `Op::UpdateNode` if a mask of that role exists, else
/// `Op::AddNode`. Used by the repair-brush / segment-edit flow; the
/// follow-up localized inpaint is a separate `POST /pipelines` call.
#[utoipa::path(
    put,
    path = "/pages/{id}/masks/{role}",
    params(
        ("id"   = PageId,   Path, description = "Page id"),
        ("role" = MaskRole, Path, description = "Mask role (segment|brushInpaint)"),
        PutMaskParams,
    ),
    request_body(content_type = "image/png"),
    responses((status = 200, body = PutMaskResponse))
)]
async fn put_mask(
    State(app): State<AppState>,
    Path((page_id, role)): Path<(PageId, MaskRole)>,
    Query(params): Query<PutMaskParams>,
    body: Bytes,
) -> ApiResult<Json<PutMaskResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    if body.is_empty() {
        return Err(ApiError::bad_request("empty body"));
    }
    // Validate it actually decodes so we don't persist garbage.
    image::load_from_memory(&body)
        .map_err(|e| ApiError::bad_request(format!("decode mask: {e}")))?;

    let blob = session.blobs.put_bytes(&body).map_err(ApiError::internal)?;

    // Find existing mask node of this role, or plan an AddNode.
    let (mut mask_op, node_id) = {
        let scene = session.scene.read();
        let existing = scene
            .page(page_id)
            .ok_or_else(|| ApiError::not_found(format!("page {page_id}")))?
            .nodes
            .iter()
            .find_map(|(id, node)| match &node.kind {
                NodeKind::Mask(m) if m.role == role => Some(*id),
                _ => None,
            });
        match existing {
            Some(id) => {
                let op = Op::UpdateNode {
                    page: page_id,
                    id,
                    patch: koharu_core::NodePatch {
                        data: Some(NodeDataPatch::Mask(koharu_core::MaskDataPatch {
                            blob: Some(blob.clone()),
                        })),
                        transform: None,
                        visible: None,
                    },
                    prev: koharu_core::NodePatch::default(),
                };
                (op, id)
            }
            None => {
                let node_id = NodeId::new();
                let at = scene.page(page_id).map(|p| p.nodes.len()).unwrap_or(0);
                let node = Node {
                    id: node_id,
                    transform: Transform::default(),
                    visible: matches!(role, MaskRole::BrushInpaint),
                    kind: NodeKind::Mask(koharu_core::MaskData {
                        role,
                        blob: blob.clone(),
                    }),
                };
                (
                    Op::AddNode {
                        page: page_id,
                        node,
                        at,
                    },
                    node_id,
                )
            }
        }
    };

    if let Some(engine_id) = params.pipeline.as_ref() {
        // Atomic Batch: Mask Update + Pipeline Run
        let mut ops = vec![mask_op.clone()];

        // 1. Simulate the mask update in a cloned scene so the engine sees it.
        let mut scene = session.scene_snapshot();
        mask_op
            .apply(&mut scene)
            .map_err(|e| ApiError::internal(e.into()))?;

        // 2. Prepare EngineCtx
        let region = Region {
            x: params.x.unwrap_or(0.0) as u32,
            y: params.y.unwrap_or(0.0) as u32,
            width: params.width.unwrap_or(0.0) as u32,
            height: params.height.unwrap_or(0.0) as u32,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let options = PipelineRunOptions {
            region: Some(region),
            ..Default::default()
        };
        let ctx = EngineCtx {
            scene: &scene,
            page: page_id,
            blobs: &session.blobs,
            runtime: &app.runtime,
            cancel: &cancel,
            options: &options,
            llm: &app.llm,
            renderer: &app.renderer,
        };

        // 3. Run Engine (Synchronously for this request)
        let engine_info = pipeline::Registry::find(engine_id)
            .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
        let engine = app
            .registry
            .get(engine_info.id, &app.runtime, app.cpu_only())
            .await
            .map_err(|e| ApiError::internal(anyhow::anyhow!("load engine: {e:#}")))?;

        let engine_ops = engine
            .run(ctx)
            .await
            .map_err(|e| ApiError::internal(anyhow::anyhow!("run engine: {e:#}")))?;

        ops.extend(engine_ops);

        let batch = Op::Batch {
            ops,
            label: format!("Repair Brush ({})", engine_id),
        };
        app.apply(batch).map_err(ApiError::internal)?;
    } else {
        app.apply(mask_op).map_err(ApiError::internal)?;
    }

    Ok(Json(PutMaskResponse {
        node: node_id,
        blob,
    }))
}
