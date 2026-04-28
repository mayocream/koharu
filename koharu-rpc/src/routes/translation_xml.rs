//! Translation XML import/export routes.

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use koharu_app::translation_xml::{
    TranslationXmlExportItem, export_translation_xml, parse_translation_xml,
};
use koharu_core::{NodeDataPatch, NodeId, NodeKind, NodePatch, Op, PageId, TextDataPatch};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(export_translation_xml_route))
        .routes(routes!(import_translation_xml_route))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportTranslationXmlRequest {
    pub xml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportTranslationXmlResponse {
    pub updated: usize,
}

#[utoipa::path(
    get,
    path = "/translations/xml",
    responses((status = 200, content_type = "application/xml"))
)]
async fn export_translation_xml_route(State(app): State<AppState>) -> ApiResult<Response> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let scene = session.scene.read();
    let items = scene
        .pages
        .iter()
        .flat_map(|(page_id, page)| {
            page.nodes.iter().filter_map(|(node_id, node)| match &node.kind {
                NodeKind::Text(text) => Some(TranslationXmlExportItem {
                    page_id: page_id.to_string(),
                    node_id: node_id.to_string(),
                    text: text.translation.clone().unwrap_or_default(),
                }),
                _ => None,
            })
        })
        .collect::<Vec<_>>();

    let project_name = sanitize_filename(&scene.project.name);
    let xml = export_translation_xml(&items);
    Ok(xml_response(
        xml,
        &format!("{project_name}-translations.xml"),
    ))
}

#[utoipa::path(
    post,
    path = "/translations/xml",
    request_body = ImportTranslationXmlRequest,
    responses((status = 200, body = ImportTranslationXmlResponse))
)]
async fn import_translation_xml_route(
    State(app): State<AppState>,
    Json(req): Json<ImportTranslationXmlRequest>,
) -> ApiResult<Json<ImportTranslationXmlResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let parsed =
        parse_translation_xml(&req.xml).map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let targets = translation_targets(&session);
    let mut ops = Vec::with_capacity(parsed.len());

    for entry in parsed {
        let (page, node) = if let (Some(pid_str), Some(nid_str)) = (entry.page_id, entry.node_id) {
            let pid = pid_str
                .parse::<Uuid>()
                .map(PageId)
                .map_err(|_| ApiError::bad_request(format!("invalid page uuid: {pid_str}")))?;
            let nid = nid_str
                .parse::<Uuid>()
                .map(NodeId)
                .map_err(|_| ApiError::bad_request(format!("invalid node uuid: {nid_str}")))?;
            (pid, nid)
        } else if let Some(id) = entry.id {
            *targets.get(id - 1).ok_or_else(|| {
                ApiError::bad_request(format!("legacy translation id {} is out of range", id))
            })?
        } else {
            continue;
        };

        ops.push(Op::UpdateNode {
            page,
            id: node,
            patch: NodePatch {
                data: Some(NodeDataPatch::Text(TextDataPatch {
                    translation: Some(Some(entry.text)),
                    ..Default::default()
                })),
                transform: None,
                visible: None,
            },
            prev: NodePatch::default(),
        });
    }

    let updated = ops.len();
    if !ops.is_empty() {
        app.apply(Op::Batch {
            ops,
            label: "Import translation XML".into(),
        })
        .map_err(ApiError::internal)?;
    }

    Ok(Json(ImportTranslationXmlResponse { updated }))
}

fn translation_targets(session: &koharu_app::ProjectSession) -> Vec<(PageId, NodeId)> {
    let scene = session.scene.read();
    scene
        .pages
        .iter()
        .flat_map(|(page_id, page)| {
            page.nodes.iter().filter_map(|(node_id, node)| {
                matches!(node.kind, NodeKind::Text(_)).then_some((*page_id, *node_id))
            })
        })
        .collect()
}

fn xml_response(xml: String, filename: &str) -> Response {
    let mut response = Response::new(Body::from(xml));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response.into_response()
}

fn sanitize_filename(name: &str) -> String {
    let out = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect::<String>();
    if out.is_empty() {
        "project".to_string()
    } else {
        out
    }
}
