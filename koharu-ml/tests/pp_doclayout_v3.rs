use std::path::Path;

use koharu_ml::pp_doclayout_v3::PPDocLayoutV3;

mod support;

fn is_textlike_label(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    label == "content" || label.contains("text") || label.contains("title")
}

#[tokio::test]
#[ignore]
async fn pp_doclayout_v3_detects_textlike_regions_on_manga_fixture() -> anyhow::Result<()> {
    let runtime = support::cpu_runtime();
    let model = PPDocLayoutV3::load(&runtime, false).await?;
    let image = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let result = model.inference_one(&image, 0.25)?;

    assert!(
        result
            .regions
            .iter()
            .any(|region| is_textlike_label(&region.label)),
        "expected at least one text-like layout region, got labels={:?}",
        result
            .regions
            .iter()
            .map(|region| region.label.as_str())
            .collect::<Vec<_>>()
    );

    Ok(())
}
