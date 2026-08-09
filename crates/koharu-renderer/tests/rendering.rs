use std::collections::BTreeMap;

use anyhow::Result;
use koharu_renderer::{LayerKind, RasterOptions, Renderer};
use koharu_scene::{
    At, Authored, Geometry, LanguageTag, Origin, PageDraft, Session, SourceText, TextLayout,
    TextLayoutKind, Translation, Typography,
};

fn authored_text_page() -> Result<(koharu_scene::Snapshot, koharu_scene::EntityId)> {
    let mut session = Session::memory()?;
    let mut page_id = None;
    let patch = session.snapshot().patch(|edit| {
        let page = edit.add_page(PageDraft::new("Text", 320.0, 180.0), At::End)?;
        let content = edit.add_text_content(page, At::End)?;
        edit.set(
            content,
            &SourceText {
                text: Authored::user("source".to_owned()),
                language: Some(LanguageTag::new("en")?),
            },
        )?;
        edit.set(
            content,
            &Translation {
                text: Authored::user("Hello, retained typography.".to_owned()),
                language: Some(LanguageTag::new("en")?),
            },
        )?;
        let layer = edit.add_text_layer(
            page,
            At::End,
            content,
            &TextLayout {
                origin: Origin::User,
                kind: TextLayoutKind::Paragraph,
                insets: [8.0; 4],
                ..TextLayout::default()
            },
        )?;
        edit.set(layer, &Geometry::rectangle(20.0, 30.0, 280.0, 120.0))?;
        edit.set(
            layer,
            &Typography {
                origin: Origin::User,
                font_families: vec!["Arial".to_owned()],
                auto_fit: false,
                size: 24.0,
                minimum_size: 9.0,
                extensions: BTreeMap::new(),
                ..Typography::default()
            },
        )?;
        page_id = Some(page);
        Ok(())
    })?;
    let snapshot = session.commit(patch)?.snapshot;
    Ok((snapshot, page_id.expect("page was authored")))
}

#[tokio::test]
async fn composition_exposes_authored_text_metadata() -> Result<()> {
    let (snapshot, page) = authored_text_page()?;
    let composition = Renderer::new()?.compose(&snapshot, page).await?;
    assert_eq!(composition.layers().len(), 1);
    let LayerKind::Text(metadata) = composition.layers()[0].kind() else {
        panic!("expected text layer");
    };
    assert_eq!(metadata.text, "Hello, retained typography.");
    assert_eq!(metadata.font_size, 24.0);
    assert_eq!(metadata.color, [0, 0, 0, 255]);
    assert!(!metadata.post_script_fonts.is_empty());
    Ok(())
}

#[tokio::test]
#[ignore = "requires a Vello-compatible GPU"]
async fn rasterizes_the_same_composition_used_for_vector_access() -> Result<()> {
    let (snapshot, page) = authored_text_page()?;
    let renderer = Renderer::new()?;
    let composition = renderer.compose(&snapshot, page).await?;
    let raster = renderer
        .rasterize(&composition, &RasterOptions::default())
        .await?;
    assert_eq!(raster.image.dimensions(), composition.size());
    assert!(raster.image.pixels().any(|pixel| pixel.0[3] != 0));
    Ok(())
}
