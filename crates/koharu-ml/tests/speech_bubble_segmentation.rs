use std::path::Path;

use koharu_ml::speech_bubble_segmentation::SpeechBubbleSegmentation;

mod support;

#[tokio::test]
#[ignore = "requires model download and is not critical for CI"]
async fn speech_bubble_segmentation() -> anyhow::Result<()> {
    let runtime = support::cpu_runtime();
    let model = SpeechBubbleSegmentation::load(&runtime, false).await?;
    let image = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/1.jpg"))?;
    let result = model.inference(&image)?;

    assert_eq!(result.image_width, image.width());
    assert_eq!(result.image_height, image.height());
    assert_eq!(
        result.probability_map.values.len(),
        (image.width() * image.height()) as usize
    );
    assert!(
        result.probability_map.max_value() > 0.05,
        "expected non-trivial speech bubble probabilities, max={}",
        result.probability_map.max_value()
    );
    assert!(
        result.regions.iter().any(|region| region.area > 0),
        "expected at least one speech bubble region with mask area"
    );

    let mask = result.probability_map.threshold(0.5)?;
    assert!(mask.pixels().any(|pixel| pixel[0] > 0));

    Ok(())
}
