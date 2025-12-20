use std::path::Path;

use anyhow::Result;
use koharu_ml::font_detector::{FontDetector, TextDirection};

#[tokio::test]
#[ignore]
async fn font_detect_inference_on_dialog_fixture() -> Result<()> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dialog.jpg");
    assert!(
        fixture.exists(),
        "missing dialog fixture at {}",
        fixture.display()
    );

    let detector = FontDetector::load(false).await?;
    let image = image::open(&fixture)?;
    let mut predictions = detector.inference(&[image], 5)?;

    assert_eq!(predictions.len(), 1, "expected a single prediction");

    let pred = predictions.pop().expect("prediction just checked to exist");
    assert!(
        !pred.top_fonts.is_empty(),
        "top fonts should be populated: {:?}",
        pred.top_fonts
    );
    assert!(
        pred.top_fonts.len() <= 5,
        "top fonts should respect requested k=5, got {}",
        pred.top_fonts.len()
    );
    assert!(
        pred.top_fonts.windows(2).all(|w| w[0].1 >= w[1].1),
        "top fonts should be sorted by probability: {:?}",
        pred.top_fonts
    );
    assert!(
        pred.top_fonts
            .iter()
            .all(|(_, p)| p.is_finite() && *p >= 0.0),
        "font probabilities should be finite and non-negative: {:?}",
        pred.top_fonts
    );
    assert!(
        pred.named_fonts
            .iter()
            .any(|nf| nf.index == pred.top_fonts[0].0),
        "top font should have a corresponding label, got: {:?}",
        pred.named_fonts
    );
    assert!(
        matches!(pred.direction, TextDirection::Vertical),
        "direction should be horizontal or vertical"
    );
    assert!(
        pred.font_size_px.is_finite() && pred.font_size_px >= 0.0,
        "font size should be non-negative: {}",
        pred.font_size_px
    );
    assert!(
        pred.stroke_width_px.is_finite() && pred.stroke_width_px >= 0.0,
        "stroke width should be non-negative: {}",
        pred.stroke_width_px
    );
    assert!(
        pred.line_height.is_finite() && pred.line_height > 0.0,
        "line height should be positive: {}",
        pred.line_height
    );
    assert!(
        pred.angle_deg.is_finite(),
        "angle should be a finite value: {}",
        pred.angle_deg
    );

    for (i, &channel) in pred.text_color.iter().enumerate() {
        assert!(
            channel > 0,
            "text color channel {} should be in [0, 255], got {}",
            i,
            channel
        );
    }
    for (i, &channel) in pred.stroke_color.iter().enumerate() {
        assert!(
            channel > 0,
            "stroke color channel {} should be in [0, 255], got {}",
            i,
            channel
        );
    }

    Ok(())
}
