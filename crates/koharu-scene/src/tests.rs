use std::{collections::BTreeMap, sync::Arc};

use crate::*;

fn page() -> PageDraft {
    PageDraft::new("page", 1200.0, 1800.0)
}

fn assert_send_sync<T: Send + Sync>() {}

fn source(text: &str) -> SourceText {
    SourceText {
        text: Authored::user(text.to_owned()),
        language: Some(LanguageTag::new("ja").unwrap()),
    }
}

#[test]
fn fresh_scene_has_an_empty_project_hierarchy() {
    assert_send_sync::<SceneSnapshot>();
    assert_send_sync::<ScenePatch>();
    let session = SceneSession::memory().unwrap();
    let snapshot = session.snapshot();
    assert_eq!(snapshot.revision(), Revision::new(1));
    assert_eq!(snapshot.pages().len(), 0);
}

#[test]
fn stale_disjoint_patches_can_rebase_without_hiding_conflicts() {
    let mut session = SceneSession::memory().unwrap();
    let mut entities = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            entities = Some((
                edit.add_entity(page, At::End)?,
                edit.add_entity(page, At::End)?,
            ));
            Ok(())
        })
        .unwrap();
    let base = session.commit(create).unwrap().snapshot;
    let (left, right) = entities.unwrap();
    let left_patch = base
        .patch(|edit| edit.set(left, "default", &Geometry::rectangle(0.0, 0.0, 10.0, 10.0)))
        .unwrap();
    let right_patch = base
        .patch(|edit| edit.set(right, "default", &Geometry::rectangle(1.0, 1.0, 10.0, 10.0)))
        .unwrap();
    let conflicting = base
        .patch(|edit| edit.set(left, "default", &Geometry::rectangle(2.0, 2.0, 10.0, 10.0)))
        .unwrap();

    let current = session.commit(left_patch).unwrap().snapshot;
    let current = session
        .commit(right_patch.rebase_on(&current).unwrap())
        .unwrap()
        .snapshot;
    assert!(conflicting.rebase_on(&current).is_err());
}

#[test]
fn observed_page_subtree_guards_pipeline_rebase_inputs() {
    let mut session = SceneSession::memory().unwrap();
    let mut ids = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let left_page = edit.add_page(PageDraft::new("left", 100.0, 100.0), At::End)?;
            let left_entity = edit.add_entity(left_page, At::End)?;
            let right_page = edit.add_page(PageDraft::new("right", 100.0, 100.0), At::End)?;
            ids = Some((left_page, left_entity, right_page));
            Ok(())
        })
        .unwrap();
    let base = session.commit(create).unwrap().snapshot;
    let (left_page, left_entity, right_page) = ids.unwrap();

    let observed = base
        .patch(|edit| {
            edit.observe_subtree(left_page)?;
            edit.set_source_text(left_entity, source("derived"))
        })
        .unwrap();
    let unrelated = base
        .patch(|edit| edit.set_page(right_page, PageDraft::new("right changed", 100.0, 100.0)))
        .unwrap();
    let current = session.commit(unrelated).unwrap().snapshot;
    assert!(observed.rebase_on(&current).is_ok());

    let observed = current
        .patch(|edit| {
            edit.observe_subtree(left_page)?;
            edit.set_source_text(left_entity, source("stale"))
        })
        .unwrap();
    let changed_input = current
        .patch(|edit| edit.set_page(left_page, PageDraft::new("left changed", 100.0, 100.0)))
        .unwrap();
    let current = session.commit(changed_input).unwrap().snapshot;
    assert!(observed.rebase_on(&current).is_err());
}

#[test]
fn project_and_relation_metadata_use_typed_components() {
    let mut session = SceneSession::memory().unwrap();
    let settings = ProjectSettings {
        source_locale: Some(LanguageTag::new("ja").unwrap()),
        target_locales: vec![LanguageTag::new("en").unwrap()],
    };
    let kind = RelationKind::new("dev.koharu.test.link").unwrap();
    let mut relation = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            edit.set_project("default", &settings)?;
            let page = edit.add_page(page(), At::End)?;
            let left = edit.add_entity(page, At::End)?;
            let right = edit.add_entity(page, At::End)?;
            let id = edit.add_relation(kind, left, right)?;
            edit.set_relation(
                id,
                "default",
                &Visibility {
                    origin: Origin::User,
                    visible: true,
                    opacity: 0.5,
                },
            )?;
            relation = Some(id);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    assert_eq!(
        snapshot
            .project_component::<ProjectSettings>("default")
            .unwrap(),
        Some(settings)
    );
    assert_eq!(
        snapshot
            .relation(relation.unwrap())
            .unwrap()
            .component::<Visibility>("default")
            .unwrap()
            .unwrap()
            .opacity,
        0.5
    );
}

#[test]
fn producer_reruns_respect_component_ownership() {
    let mut session = SceneSession::memory().unwrap();
    let mut entities = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let user = edit.add_entity(page, At::End)?;
            edit.set_source_text(user, source("user"))?;
            let generated = edit.add_entity(page, At::End)?;
            entities = Some((user, generated));
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    let (user, generated) = entities.unwrap();
    let producer = ProducerId::new("dev.koharu.pipeline.ocr").unwrap();
    let generation = Generation::new(producer.clone());
    let mut edit = snapshot.edit_as(generation.clone());
    assert!(matches!(
        edit.set_source_text(user, source("overwrite")),
        Err(Error::Authorship(_))
    ));
    edit.set_source_text(generated, source("generated"))
        .unwrap();
    let snapshot = session.commit(edit.finish().unwrap()).unwrap().snapshot;

    let mut rerun = snapshot.edit_as(generation);
    rerun
        .set_source_text(generated, source("generated again"))
        .unwrap();
    let snapshot = session.commit(rerun.finish().unwrap()).unwrap().snapshot;
    let other = Generation::new(ProducerId::new("dev.koharu.pipeline.other-ocr").unwrap());
    let mut other_edit = snapshot.edit_as(other);
    assert!(matches!(
        other_edit.set_source_text(generated, source("wrong owner")),
        Err(Error::Authorship(_))
    ));
}

#[test]
fn pipeline_removal_respects_entity_lifecycle_owner() {
    let mut session = SceneSession::memory().unwrap();
    let mut page_id = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            page_id = Some(edit.add_page(page(), At::End)?);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    let page = page_id.unwrap();
    let owner = Generation::new(ProducerId::new("dev.koharu.pipeline.detector").unwrap());
    let mut edit = snapshot.edit_as(owner.clone());
    let generated = edit.add_entity(page, At::End).unwrap();
    let snapshot = session.commit(edit.finish().unwrap()).unwrap().snapshot;

    let mut other = snapshot.edit_as(Generation::new(
        ProducerId::new("dev.koharu.pipeline.other-detector").unwrap(),
    ));
    assert!(matches!(
        other.remove_entity(generated, RemovePolicy::Cascade),
        Err(Error::Authorship(_))
    ));

    let mut owner_edit = snapshot.edit_as(owner);
    owner_edit
        .remove_entity(generated, RemovePolicy::Cascade)
        .unwrap();
    let snapshot = session
        .commit(owner_edit.finish().unwrap())
        .unwrap()
        .snapshot;
    assert!(snapshot.entity(generated).is_err());
}

#[test]
fn typed_page_entity_and_components_round_trip() {
    let mut session = SceneSession::memory().unwrap();
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let text = edit.add_entity(page, At::End)?;
            edit.set(text, "default", &source("こんにちは"))?;
            edit.set(
                text,
                "default",
                &Geometry::rectangle(10.0, 20.0, 100.0, 40.0),
            )?;
            Ok(())
        })
        .unwrap();
    let commit = session.commit(patch).unwrap();
    let page = commit.snapshot.pages().next().unwrap();
    assert_eq!(page.page().unwrap().label, "page");
    let text = commit.snapshot.children(page.id()).unwrap().next().unwrap();
    assert_eq!(
        commit
            .snapshot
            .component::<SourceText>(text, "default")
            .unwrap()
            .unwrap()
            .text
            .value,
        "こんにちは"
    );
}

#[test]
fn hierarchy_move_and_promote_are_ordered() {
    let mut session = SceneSession::memory().unwrap();
    let mut ids = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let group = edit.add_entity(page, At::End)?;
            let child = edit.add_entity(group, At::End)?;
            let sibling = edit.add_entity(page, At::End)?;
            ids = Some((page, group, child, sibling));
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    let (page, group, child, sibling) = ids.unwrap();
    let patch = snapshot
        .patch(|edit| {
            edit.move_entity(sibling, Some(group), At::Start)?;
            edit.remove_entity(group, RemovePolicy::PromoteChildren)
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    assert_eq!(
        snapshot.children(page).unwrap().collect::<Vec<_>>(),
        vec![sibling, child]
    );
    assert_eq!(snapshot.parent(child).unwrap(), Some(page));
    assert!(matches!(
        snapshot.entity(group),
        Err(Error::EntityNotFound(_))
    ));
}

#[test]
fn relations_are_records_with_typed_adjacency() {
    let mut session = SceneSession::memory().unwrap();
    let kind = RelationKind::new("dev.koharu.test.reading-order").unwrap();
    let mut ids = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let first = edit.add_entity(page, At::End)?;
            let second = edit.add_entity(page, At::End)?;
            let relation = edit.add_relation(kind.clone(), first, second)?;
            ids = Some((first, second, relation));
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    let (first, second, relation) = ids.unwrap();
    assert_eq!(
        snapshot
            .relations_from(first, Some(&kind))
            .next()
            .unwrap()
            .id(),
        relation
    );
    assert_eq!(
        snapshot
            .relations_to(second, None)
            .next()
            .unwrap()
            .value()
            .source,
        first
    );
    assert!(matches!(
        snapshot.patch(|edit| edit.remove_entity(first, RemovePolicy::RejectNonEmpty)),
        Err(Error::IncidentRelations(id)) if id == first
    ));
    let patch = snapshot
        .patch(|edit| edit.remove_entity(first, RemovePolicy::Cascade))
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    assert!(snapshot.relation(relation).is_err());
}

#[test]
fn translation_requires_source_text() {
    let session = SceneSession::memory().unwrap();
    let result = session.snapshot().patch(|edit| {
        let page = edit.add_page(page(), At::End)?;
        let entity = edit.add_entity(page, At::End)?;
        edit.set(
            entity,
            "en",
            &Translation {
                text: Authored::user("hello".to_owned()),
            },
        )
    });
    assert!(matches!(result, Err(Error::Invalid(_))));
}

#[test]
fn independent_pipeline_components_rebase() {
    let mut session = SceneSession::memory().unwrap();
    let mut entity = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let id = edit.add_entity(page, At::End)?;
            edit.set(id, "default", &source("source"))?;
            entity = Some(id);
            Ok(())
        })
        .unwrap();
    let base = session.commit(create).unwrap().snapshot;
    let entity = entity.unwrap();
    let translation = base
        .patch(|edit| {
            edit.set_translation(
                entity,
                &LanguageTag::new("en").unwrap(),
                Translation {
                    text: Authored::user("translation".to_owned()),
                },
            )
        })
        .unwrap();
    let typography = base
        .patch(|edit| {
            edit.set(
                entity,
                "default",
                &Typography {
                    origin: Origin::User,
                    preferred_font: Some("Inter".to_owned()),
                    size: Some(24.0),
                    alignment: Some(TextAlignment::Center),
                    writing_mode: None,
                    extensions: BTreeMap::new(),
                },
            )
        })
        .unwrap();
    let current = session.commit(translation).unwrap().snapshot;
    let typography = typography.rebase_on(&current).unwrap();
    let snapshot = session.commit(typography).unwrap().snapshot;
    assert!(
        snapshot
            .component::<Translation>(entity, "en")
            .unwrap()
            .is_some()
    );
    assert!(
        snapshot
            .component::<Typography>(entity, "default")
            .unwrap()
            .is_some()
    );
}

#[test]
fn descendant_scene_patch_rebases_after_ancestor_commit() {
    let mut session = SceneSession::memory().unwrap();
    let base = session.snapshot();
    let mut edit = base.edit();
    let page = edit.add_page(page(), At::End).unwrap();
    let ancestor = edit.finish().unwrap();
    let preview = base.preview([&ancestor]).unwrap();
    let descendant = preview
        .patch(|edit| {
            let entity = edit.add_entity(page, At::End)?;
            edit.set(entity, "default", &source("late"))
        })
        .unwrap();
    assert!(base.preview([&ancestor, &descendant]).is_ok());
    let current = session.commit(ancestor).unwrap().snapshot;
    let descendant = descendant.rebase_on(&current).unwrap();
    let snapshot = session.commit(descendant).unwrap().snapshot;
    assert_eq!(snapshot.children(page).unwrap().len(), 1);
}

#[test]
fn assets_attach_bytes_without_decoding() {
    let mut session = SceneSession::memory().unwrap();
    let role = AssetRole::new("source").unwrap();
    let mut entity = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            entity = Some(page);
            edit.set_asset(
                page,
                &role,
                AssetInput::new(
                    Arc::<[u8]>::from(&b"encoded image"[..]),
                    "image/test",
                    AssetMetadata {
                        width: Some(10),
                        height: Some(20),
                        attributes: BTreeMap::new(),
                    },
                ),
            )
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    let asset = snapshot
        .component::<Asset>(entity.unwrap(), "source")
        .unwrap()
        .unwrap();
    assert_eq!(&*snapshot.read_blob(asset.blob).unwrap(), b"encoded image");
    let batch = snapshot.read_blobs([asset.blob, asset.blob]).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(&**batch.get(asset.blob).unwrap(), b"encoded image");
}

#[test]
fn disk_scene_reopens_and_validates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scene.khs");
    {
        let mut session = SceneSession::create(&path).unwrap();
        let patch = session
            .snapshot()
            .patch(|edit| edit.add_page(page(), At::End).map(|_| ()))
            .unwrap();
        session.commit(patch).unwrap();
        session.checkpoint().unwrap();
    }
    let session = SceneSession::open(&path).unwrap();
    assert_eq!(session.snapshot().pages().len(), 1);
}

#[test]
fn component_only_refresh_updates_the_existing_scene_index() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("refresh.khr");
    let mut writer = SceneSession::create(&path).unwrap();
    let mut reader = SceneSession::open(&path).unwrap();
    let mut entity = None;
    let create = writer
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            entity = Some(edit.add_entity(page, At::End)?);
            Ok(())
        })
        .unwrap();
    writer.commit(create).unwrap();
    reader.refresh().unwrap();
    let marker = reader.snapshot().index.build_marker();
    let entity = entity.unwrap();

    let update = writer
        .snapshot()
        .patch(|edit| edit.set_source_text(entity, source("refreshed")))
        .unwrap();
    writer.commit(update).unwrap();
    reader.refresh().unwrap();

    let snapshot = reader.snapshot();
    assert_eq!(snapshot.index.build_marker(), marker);
    assert_eq!(
        snapshot
            .component::<SourceText>(entity, "default")
            .unwrap()
            .unwrap()
            .text
            .value,
        "refreshed"
    );
}

#[test]
fn pipeline_component_removal_and_relation_lifecycle_respect_ownership() {
    let mut session = SceneSession::memory().unwrap();
    let role = AssetRole::new("source").unwrap();
    let relation_kind = RelationKind::new("dev.koharu.test.association").unwrap();
    let mut ids = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let left = edit.add_entity(page, At::End)?;
            let right = edit.add_entity(page, At::End)?;
            edit.set_source_text(left, source("user text"))?;
            edit.set_asset(
                left,
                &role,
                AssetInput::new(
                    Arc::<[u8]>::from(&b"user asset"[..]),
                    "image/test",
                    AssetMetadata {
                        width: Some(1),
                        height: Some(1),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            let relation = edit.add_relation(relation_kind.clone(), left, right)?;
            ids = Some((left, relation));
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(create).unwrap().snapshot;
    let (entity, relation) = ids.unwrap();
    let generation = Generation::new(ProducerId::new("dev.koharu.pipeline.detector").unwrap());
    let mut pipeline = snapshot.edit_as(generation.clone());
    assert!(matches!(
        pipeline.remove::<SourceText>(entity, "default"),
        Err(Error::Authorship(_))
    ));
    assert!(matches!(
        pipeline.remove::<Asset>(entity, role.as_str()),
        Err(Error::Authorship(_))
    ));
    assert!(matches!(
        pipeline.remove_relation(relation),
        Err(Error::Authorship(_))
    ));

    let analysis = DetectionAnalysis {
        origin: Origin::User,
        labels: vec![DetectionLabel {
            kind: RegionKind::new("dev.koharu.region.text").unwrap(),
            confidence: 0.9,
        }],
    };
    pipeline
        .set(entity, "default", &analysis)
        .expect("new pipeline analysis is stamped by the edit context");
    let snapshot = session.commit(pipeline.finish().unwrap()).unwrap().snapshot;
    let stored = snapshot
        .component::<DetectionAnalysis>(entity, "default")
        .unwrap()
        .unwrap();
    assert!(matches!(stored.origin, Origin::Generated(_)));

    let mut other = snapshot.edit_as(Generation::new(
        ProducerId::new("dev.koharu.pipeline.other-detector").unwrap(),
    ));
    assert!(matches!(
        other.set(entity, "default", &analysis),
        Err(Error::Authorship(_))
    ));
    assert!(matches!(
        other.remove::<DetectionAnalysis>(entity, "default"),
        Err(Error::Authorship(_))
    ));
}

#[test]
fn pipeline_queries_and_analysis_components_are_semantic_and_ordered() {
    let mut session = SceneSession::memory().unwrap();
    let mut ids = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let first = edit.add_entity(page, At::End)?;
            let nested = edit.add_entity(first, At::End)?;
            let second = edit.add_entity(page, At::End)?;
            edit.set_source_text(first, source("first"))?;
            edit.set_source_text(nested, source("nested"))?;
            ids = Some((page, first, nested, second));
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(create).unwrap().snapshot;
    let (page, first, nested, second) = ids.unwrap();
    assert_eq!(
        snapshot.entities().map(EntityRef::id).collect::<Vec<_>>(),
        vec![page, first, nested, second]
    );
    assert_eq!(
        snapshot
            .entities_with::<SourceText>("default")
            .unwrap()
            .map(EntityRef::id)
            .collect::<Vec<_>>(),
        vec![first, nested]
    );
    assert_eq!(
        snapshot
            .subtree(first)
            .unwrap()
            .map(EntityRef::id)
            .collect::<Vec<_>>(),
        vec![first, nested]
    );
    assert_eq!(
        snapshot
            .descendants(first)
            .unwrap()
            .map(EntityRef::id)
            .collect::<Vec<_>>(),
        vec![nested]
    );

    let generation = Generation::new(ProducerId::new("dev.koharu.pipeline.ocr").unwrap());
    let mut edit = snapshot.edit_as(generation);
    edit.set(
        first,
        "default",
        &OcrAnalysis {
            origin: Origin::User,
            direction: TextDirection::Vertical,
            confidence: Some(0.95),
            line_boundaries: vec![[
                Point { x: 0.0, y: 0.0 },
                Point { x: 1.0, y: 0.0 },
                Point { x: 1.0, y: 1.0 },
                Point { x: 0.0, y: 1.0 },
            ]],
        },
    )
    .unwrap();
    edit.set(
        first,
        "default",
        &ReadingOrder {
            origin: Origin::User,
            index: 3,
        },
    )
    .unwrap();
    let snapshot = session.commit(edit.finish().unwrap()).unwrap().snapshot;
    assert!(matches!(
        snapshot
            .component::<OcrAnalysis>(first, "default")
            .unwrap()
            .unwrap()
            .origin,
        Origin::Generated(_)
    ));
    assert_eq!(
        snapshot
            .component::<ReadingOrder>(first, "default")
            .unwrap()
            .unwrap()
            .index,
        3
    );
}

#[test]
fn scene_patch_exposes_stable_identity_and_base() {
    let mut session = SceneSession::memory().unwrap();
    let mut entity = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            entity = Some(edit.add_entity(page, At::End)?);
            Ok(())
        })
        .unwrap();
    let base = session.commit(create).unwrap().snapshot;
    let entity = entity.unwrap();
    let patch = base
        .patch(|edit| edit.set_source_text(entity, source("identity")))
        .unwrap();
    assert_eq!(patch.project_id(), base.project_id());
    assert_eq!(patch.base_revision(), base.revision());
    assert_eq!(patch.fingerprint(), patch.clone().fingerprint());
    assert_ne!(
        patch.fingerprint(),
        patch
            .clone()
            .with_label("different commit label")
            .fingerprint()
    );
}

#[test]
fn component_only_pipeline_work_reuses_scene_indexes() {
    let mut session = SceneSession::memory().unwrap();
    let mut entity = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            entity = Some(edit.add_entity(page, At::End)?);
            Ok(())
        })
        .unwrap();
    let base = session.commit(create).unwrap().snapshot;
    let entity = entity.unwrap();
    let build_marker = base.index.build_marker();

    let patch = base
        .patch(|edit| edit.set_source_text(entity, source("cached")))
        .unwrap();
    let cached_index = patch.result_index.clone().unwrap();
    assert_eq!(cached_index.build_marker(), build_marker);
    let preview = base.preview([&patch]).unwrap();
    assert!(Arc::ptr_eq(&preview.index, &cached_index));
    assert_eq!(
        preview
            .entities_with::<SourceText>("default")
            .unwrap()
            .len(),
        1
    );
    let committed = session.commit(patch).unwrap().snapshot;
    assert!(Arc::ptr_eq(&committed.index, &cached_index));
    let first = session.snapshot();
    let second = session.snapshot();
    assert!(Arc::ptr_eq(&first.index, &second.index));
}

#[test]
fn user_promotion_protects_generated_entity_and_relation_lifecycle() {
    let mut session = SceneSession::memory().unwrap();
    let mut endpoints = None;
    let create = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(page(), At::End)?;
            let left = edit.add_entity(page, At::End)?;
            let right = edit.add_entity(page, At::End)?;
            endpoints = Some((left, right));
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(create).unwrap().snapshot;
    let (left, right) = endpoints.unwrap();
    let generation = Generation::new(ProducerId::new("dev.koharu.pipeline.detector").unwrap());
    let mut generated = snapshot.edit_as(generation.clone());
    let entity = generated.add_entity(left, At::End).unwrap();
    let relation = generated
        .add_relation(
            RelationKind::new("dev.koharu.test.generated-link").unwrap(),
            entity,
            right,
        )
        .unwrap();
    let snapshot = session
        .commit(generated.finish().unwrap())
        .unwrap()
        .snapshot;

    let promoted = snapshot
        .patch(|edit| {
            edit.promote_entity_to_user(entity)?;
            edit.promote_relation_to_user(relation)
        })
        .unwrap();
    let snapshot = session.commit(promoted).unwrap().snapshot;
    let mut rerun = snapshot.edit_as(generation);
    assert!(matches!(
        rerun.remove_relation(relation),
        Err(Error::Authorship(_))
    ));
    assert!(matches!(
        rerun.remove_entity(entity, RemovePolicy::Cascade),
        Err(Error::Authorship(_))
    ));
}
