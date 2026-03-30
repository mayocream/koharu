use crate::http::http_client;

#[derive(Debug, Clone, Copy)]
pub struct HeadInfo {
    pub content_length: u64,
    pub supports_ranges: bool,
}

pub async fn head(url: &str) -> anyhow::Result<HeadInfo> {
    let response = http_client().head(url).send().await?.error_for_status()?;

    let content_length = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("missing Content-Length header"))?;

    let supports_ranges = response
        .headers()
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);

    Ok(HeadInfo {
        content_length,
        supports_ranges,
    })
}

pub async fn head_content_length(url: &str) -> anyhow::Result<u64> {
    Ok(head(url).await?.content_length)
}

pub async fn get_range(url: &str, start: u64, end_inclusive: u64) -> anyhow::Result<Vec<u8>> {
    let response = http_client()
        .get(url)
        .header(
            reqwest::header::RANGE,
            format!("bytes={start}-{end_inclusive}"),
        )
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        anyhow::bail!("server did not honor range: {}", response.status());
    }

    Ok(response.bytes().await?.to_vec())
}

pub async fn get_tail(url: &str, nbytes: usize) -> anyhow::Result<Vec<u8>> {
    let length = head_content_length(url).await?;
    let start = length.saturating_sub(nbytes as u64);
    get_range(url, start, length.saturating_sub(1)).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use tokio::sync::oneshot;

    use super::{get_range, get_tail, head};

    #[derive(Clone)]
    struct TestState {
        bytes: Arc<Vec<u8>>,
        supports_ranges: bool,
    }

    async fn head_handler(State(state): State<TestState>) -> impl IntoResponse {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&state.bytes.len().to_string()).expect("valid content length"),
        );
        if state.supports_ranges {
            headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        }
        (StatusCode::OK, headers)
    }

    fn parse_range(headers: &HeaderMap, len: usize) -> Option<(usize, usize)> {
        let range = headers.get(header::RANGE)?.to_str().ok()?;
        let suffix = range.strip_prefix("bytes=")?;
        let (start, end) = suffix.split_once('-')?;
        let start = start.parse::<usize>().ok()?;
        let end = end.parse::<usize>().ok()?.min(len.saturating_sub(1));
        if start > end || start >= len {
            return None;
        }
        Some((start, end))
    }

    async fn get_handler(State(state): State<TestState>, headers: HeaderMap) -> impl IntoResponse {
        if state.supports_ranges
            && let Some((start, end)) = parse_range(&headers, state.bytes.len())
        {
            let chunk = state.bytes[start..=end].to_vec();
            return (StatusCode::PARTIAL_CONTENT, chunk).into_response();
        }
        (StatusCode::OK, state.bytes.to_vec()).into_response()
    }

    async fn start_server(bytes: Vec<u8>, supports_ranges: bool) -> (String, oneshot::Sender<()>) {
        let state = TestState {
            bytes: Arc::new(bytes),
            supports_ranges,
        };
        let app = Router::new()
            .route("/file", get(get_handler).head(head_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("get local addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("serve test app");
        });

        (format!("http://{addr}/file"), shutdown_tx)
    }

    #[tokio::test]
    async fn range_and_tail_helpers_work() {
        let (url, shutdown) = start_server(b"0123456789abcdef".to_vec(), true).await;
        let info = head(&url).await.expect("head should succeed");
        assert_eq!(info.content_length, 16);
        assert!(info.supports_ranges);

        let chunk = get_range(&url, 2, 5).await.expect("range should succeed");
        assert_eq!(chunk, b"2345");

        let tail = get_tail(&url, 4).await.expect("tail should succeed");
        assert_eq!(tail, b"cdef");
        let _ = shutdown.send(());
    }

    #[tokio::test]
    async fn get_range_fails_when_server_ignores_range() {
        let (url, shutdown) = start_server(b"abcdef".to_vec(), false).await;
        let err = get_range(&url, 0, 2)
            .await
            .expect_err("range must fail without partial content");
        assert!(
            err.to_string().contains("server did not honor range"),
            "unexpected error: {err:#}"
        );
        let _ = shutdown.send(());
    }
}
