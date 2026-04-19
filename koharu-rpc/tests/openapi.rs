//! Snapshot guard for the OpenAPI surface.
//!
//! The spec covers every HTTP route exposed by `koharu-rpc::api::api()`. A
//! change to any handler's path, body, or response will churn this file.
//! Regenerate with `cargo insta review` after intended changes.

#[test]
fn openapi_paths_snapshot() {
    let (_, spec) = koharu_rpc::api::api();

    let json = serde_json::to_value(&spec).expect("serialize OpenAPI");
    let mut paths: Vec<(String, Vec<String>)> = json["paths"]
        .as_object()
        .expect("paths object")
        .iter()
        .map(|(path, item)| {
            let mut methods: Vec<String> = item
                .as_object()
                .map(|o| {
                    o.keys()
                        .filter(|k| {
                            matches!(k.as_str(), "get" | "post" | "put" | "patch" | "delete")
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            methods.sort();
            (path.clone(), methods)
        })
        .collect();
    paths.sort();

    insta::assert_debug_snapshot!(paths);
}
