//! Font routes.
//!
//! - `GET /fonts` — combined system + Google Fonts catalog.
//! - `GET /google-fonts` — the Google Fonts catalog as a standalone list.
//! - `POST /google-fonts/{family}/fetch` — download and cache a family.
//! - `GET /google-fonts/{family}/{file}` — serve the cached TTF/WOFF file.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header::CONTENT_TYPE};
use axum::response::{IntoResponse, Json, Response};
use koharu_core::{FontFaceInfo, GoogleFontCatalog};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(list_fonts))
        .routes(routes!(get_google_fonts_catalog))
        .routes(routes!(fetch_google_font))
        .routes(routes!(get_google_font_file))
}

#[utoipa::path(get, path = "/fonts", responses((status = 200, body = Vec<FontFaceInfo>)))]
async fn list_fonts(State(app): State<AppState>) -> ApiResult<Json<Vec<FontFaceInfo>>> {
    let fonts = app.renderer.available_fonts().map_err(ApiError::internal)?;
    Ok(Json(fonts))
}

#[utoipa::path(
    get,
    path = "/google-fonts",
    responses((status = 200, body = GoogleFontCatalog))
)]
async fn get_google_fonts_catalog(
    State(app): State<AppState>,
) -> ApiResult<Json<GoogleFontCatalog>> {
    Ok(Json(app.renderer.google_fonts.catalog().clone()))
}

#[utoipa::path(
    post,
    path = "/google-fonts/{family}/fetch",
    params(("family" = String, Path, description = "Google Fonts family name")),
    responses((status = 204))
)]
async fn fetch_google_font(
    State(app): State<AppState>,
    Path(family): Path<String>,
) -> ApiResult<StatusCode> {
    let http = app.runtime.http_client();
    app.renderer
        .google_fonts
        .fetch_family(&family, &http)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/google-fonts/{family}/{file}",
    params(
        ("family" = String, Path, description = "Google Fonts family name"),
        ("file" = String, Path, description = "Font filename"),
    ),
    responses((status = 200, content_type = "font/ttf"))
)]
async fn get_google_font_file(
    State(app): State<AppState>,
    Path((family, _file)): Path<(String, String)>,
) -> ApiResult<Response> {
    let bytes = app
        .renderer
        .google_fonts
        .read_cached_file(&family)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("font {family} not cached")))?;
    let mut resp = Response::new(Body::from(bytes));
    resp.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("font/ttf"));
    Ok(resp.into_response())
}
