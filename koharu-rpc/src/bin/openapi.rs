fn main() {
    let (_, mut spec) = koharu_rpc::api::api();

    // Register schemas that are referenced via IntoParams but not auto-collected
    let extras = utoipa::openapi::OpenApiBuilder::new()
        .components(Some(
            utoipa::openapi::ComponentsBuilder::new()
                .schema_from::<koharu_core::ImportMode>()
                .schema_from::<koharu_core::ExportLayer>()
                .build(),
        ))
        .build();
    spec.merge(extras);

    println!("{}", spec.to_pretty_json().unwrap());
}
