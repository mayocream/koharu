use std::io::Cursor;

use crate::{
    Command, Commands, ElementChange, Error, Frame, ModelPrediction, PageAsset, Project, Region,
    RegionKind, Revision, Session, SourceText, TextBlock, TextDirection, TextRole,
};
use image::{DynamicImage, GrayImage, ImageFormat, RgbaImage};

fn rgba_png(color: [u8; 4]) -> Vec<u8> {
    encode(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        8,
        6,
        image::Rgba(color),
    )))
}

fn mask_png(value: u8) -> Vec<u8> {
    encode(DynamicImage::ImageLuma8(GrayImage::from_pixel(
        8,
        6,
        image::Luma([value]),
    )))
}

fn encode(image: DynamicImage) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}

#[test]
fn revision_round_trips_the_public_model() {
    let project = Project::new();
    let bytes = revision::to_vec(&project).unwrap();
    let decoded: Project = revision::from_slice(&bytes).unwrap();
    assert_eq!(decoded, project);
}

#[test]
fn commands_build_a_koharu_page() {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands
        .add_page("001.png", rgba_png([1, 2, 3, 255]))
        .unwrap();
    let text = commands.add_text(page, Frame::new(1.0, 2.0, 3.0, 4.0));
    commands.push(Command::EditElement {
        page,
        element: text,
        edit: ElementChange::Source(Some(SourceText {
            text: "こんにちは".into(),
            language: Some("ja".into()),
            direction: TextDirection::Vertical,
            confidence: Some(0.9),
            lines: Vec::new(),
        })),
    });
    commands.push(Command::EditElement {
        page,
        element: text,
        edit: ElementChange::Translation(Some("Hello".into())),
    });

    let applied = session.apply(commands).unwrap();
    assert_eq!(applied.to, Revision::new(1));
    assert_eq!(
        session
            .page(page)
            .unwrap()
            .text(text)
            .unwrap()
            .translation
            .as_deref(),
        Some("Hello")
    );
}

#[test]
fn typed_regions_and_text_relationships_persist() {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands
        .add_page("relationships.png", rgba_png([1, 2, 3, 255]))
        .unwrap();
    let panel = commands.add_region(
        page,
        Frame::new(0.0, 0.0, 8.0, 6.0),
        Region {
            kind: RegionKind::Panel,
            polygon: Vec::new(),
            mask_id: None,
            reading_order: Some(0),
            predictions: vec![ModelPrediction::new("layout", 0.9)],
        },
    );
    let bubble = commands.add_region(
        page,
        Frame::new(1.0, 1.0, 6.0, 4.0),
        Region {
            kind: RegionKind::Bubble,
            polygon: Vec::new(),
            mask_id: Some(1),
            reading_order: Some(0),
            predictions: vec![ModelPrediction::new("layout", 0.8)],
        },
    );
    let text = commands.add_text_block(
        page,
        Frame::new(2.0, 1.5, 4.0, 3.0),
        TextBlock {
            role: TextRole::Dialogue,
            panel: Some(panel),
            bubble: Some(bubble),
            reading_order: Some(0),
            ..TextBlock::default()
        },
    );
    session.apply(commands).unwrap();

    let bytes = revision::to_vec(session.project()).unwrap();
    let decoded: Project = revision::from_slice(&bytes).unwrap();
    let decoded_text = decoded.pages[0].text(text).unwrap();
    assert_eq!(decoded_text.panel, Some(panel));
    assert_eq!(decoded_text.bubble, Some(bubble));

    let before = session.revision();
    let mut commands = session.commands();
    commands.push(Command::DeleteElement {
        page,
        element: bubble,
    });
    assert!(session.apply(commands).is_err());
    assert_eq!(session.revision(), before);
}

#[test]
fn transferred_commands_revalidate_and_apply_attachments() {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands
        .add_page("shared.png", rgba_png([8, 7, 6, 255]))
        .unwrap();
    commands
        .set_asset(page, PageAsset::TextMask, Some(mask_png(255)))
        .unwrap();

    let parts = commands.into_parts();
    let commands = Commands::from_parts(parts).unwrap();
    session.apply(commands).unwrap();

    assert!(session.page(page).unwrap().assets.text_mask.is_some());
}

#[test]
fn transferred_commands_reject_a_false_attachment_hash() {
    let mut commands = Commands::new(Revision::ZERO);
    commands.add_page("page", rgba_png([1, 2, 3, 255])).unwrap();
    let mut parts = commands.into_parts();
    parts.attachments[0].1 = rgba_png([9, 9, 9, 255]).into();

    assert!(Commands::from_parts(parts).is_err());
}

#[test]
fn fluent_edits_are_commands() {
    let mut session = Session::memory().unwrap();
    let mut edit = session.edit();
    let page = edit.add_page("page", rgba_png([0, 0, 0, 255])).unwrap();
    let text = edit
        .page(page)
        .unwrap()
        .add_text(Frame::new(0.0, 0.0, 40.0, 20.0));
    edit.page(page)
        .unwrap()
        .text(text)
        .unwrap()
        .set_translation(Some("Hi"))
        .set_opacity(0.5);
    edit.commit().unwrap();

    let element = session.element(text).unwrap().1;
    assert_eq!(element.opacity, 0.5);
    assert_eq!(element.text().unwrap().translation.as_deref(), Some("Hi"));
}

#[test]
fn project_reopens_from_sqlite() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.khr");
    let page;
    {
        let mut session = Session::create(&path).unwrap();
        let mut commands = session.commands();
        page = commands.add_page("page", rgba_png([4, 5, 6, 255])).unwrap();
        session.apply(commands).unwrap();
    }
    let session = Session::open(&path).unwrap();
    assert_eq!(session.revision(), Revision::new(1));
    assert_eq!(session.page(page).unwrap().name, "page");
    assert!(
        !session
            .read_blob(session.page(page).unwrap().source)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stale_writers_refresh_instead_of_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("parallel.khr");
    let mut first = Session::create(&path).unwrap();
    let mut second = Session::open(&path).unwrap();

    let mut commands = first.commands();
    let page = commands.add_page("page", rgba_png([0, 0, 0, 255])).unwrap();
    first.apply(commands).unwrap();

    let mut stale = second.commands();
    stale.add_page("stale", rgba_png([0, 0, 0, 255])).unwrap();
    assert!(matches!(
        second.apply(stale),
        Err(Error::RevisionConflict { .. })
    ));
    assert!(second.project().pages.is_empty());
    let changes = second.refresh().unwrap();
    assert_eq!(changes.to, Revision::new(1));
    assert!(second.page(page).is_ok());
}

#[test]
fn refresh_falls_back_to_a_pruned_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pruned.khr");
    let mut writer = Session::create(&path).unwrap();
    let mut reader = Session::open(&path).unwrap();
    let mut commands = writer.commands();
    let page = commands.add_page("page", rgba_png([0, 0, 0, 255])).unwrap();
    writer.apply(commands).unwrap();
    writer
        .prune_history(Revision::new(writer.revision().get() + 1))
        .unwrap();

    reader.refresh().unwrap();
    assert!(reader.page(page).is_ok());
}

#[test]
fn a_failed_batch_leaves_memory_and_sqlite_unchanged() {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands.add_page("page", rgba_png([0, 0, 0, 255])).unwrap();
    let text = commands.add_text(page, Frame::new(0.0, 0.0, 10.0, 10.0));
    commands.push(Command::EditElement {
        page,
        element: text,
        edit: ElementChange::Opacity(2.0),
    });

    assert!(session.apply(commands).is_err());
    assert_eq!(session.revision(), Revision::ZERO);
    assert!(session.project().pages.is_empty());
}

#[test]
fn masks_must_be_single_channel_and_page_sized() {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands.add_page("page", rgba_png([0, 0, 0, 255])).unwrap();
    session.apply(commands).unwrap();

    let mut invalid = session.commands();
    assert!(
        invalid
            .set_asset(page, PageAsset::TextMask, Some(rgba_png([0, 0, 0, 255])))
            .is_err()
    );

    let mut valid = session.commands();
    valid
        .set_asset(page, PageAsset::TextMask, Some(mask_png(255)))
        .unwrap();
    session.apply(valid).unwrap();
    assert!(session.page(page).unwrap().assets.text_mask.is_some());
}

#[test]
fn a_new_page_can_receive_an_asset_in_the_same_batch() {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands.add_page("page", rgba_png([0, 0, 0, 255])).unwrap();
    commands
        .set_asset(page, PageAsset::BubbleMask, Some(mask_png(1)))
        .unwrap();
    session.apply(commands).unwrap();
    assert!(session.page(page).unwrap().assets.bubble_mask.is_some());
}

#[test]
fn retained_changes_are_safely_reverted() {
    let mut session = Session::memory().unwrap();
    let mut add = session.commands();
    let page = add.add_page("page", rgba_png([0, 0, 0, 255])).unwrap();
    let revision = session.apply(add).unwrap().to;
    assert!(session.page(page).is_ok());

    session.revert([revision]).unwrap();
    assert!(session.page(page).is_err());
}

#[test]
fn merge_allows_independent_fields_and_rejects_same_field() {
    let session = Session::memory().unwrap();
    let page = crate::PageId::new();
    let element = crate::ElementId::new();
    let mut left = session.commands();
    left.push(Command::EditElement {
        page,
        element,
        edit: ElementChange::Translation(Some("a".into())),
    });
    let mut right = session.commands();
    right.push(Command::EditElement {
        page,
        element,
        edit: ElementChange::Style(crate::TextStyle::default()),
    });
    left.merge(right).unwrap();

    let mut conflict = session.commands();
    conflict.push(Command::EditElement {
        page,
        element,
        edit: ElementChange::Translation(Some("b".into())),
    });
    assert!(matches!(left.merge(conflict), Err(Error::CommandConflict)));
}

#[test]
fn pruning_all_history_allows_old_blobs_to_be_collected() {
    let mut session = Session::memory().unwrap();
    let mut create = session.commands();
    let page = create.add_page("page", rgba_png([1, 1, 1, 255])).unwrap();
    session.apply(create).unwrap();
    let old = session.page(page).unwrap().source;

    let mut replace = session.commands();
    replace
        .replace_source(page, rgba_png([2, 2, 2, 255]))
        .unwrap();
    session.apply(replace).unwrap();
    let after_head = Revision::new(session.revision().get() + 1);
    let report = session.prune_history(after_head).unwrap();

    assert_eq!(report.blobs, 1);
    assert!(session.read_blob(old).is_err());
}
