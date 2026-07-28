use std::sync::Arc;

use crate::{ComponentKey, ComponentRecord, Error, Patch, RecordId, Revision, Session};

fn assert_send_sync<T: Send + Sync>() {}

fn key(name: &str) -> ComponentKey {
    ComponentKey::named(format!("dev.koharu.test.{name}"), "default").unwrap()
}

fn value(bytes: &[u8]) -> ComponentRecord {
    ComponentRecord::new(1, Arc::<[u8]>::from(bytes), [], []).unwrap()
}

#[test]
fn new_document_has_permanent_root() {
    assert_send_sync::<crate::Snapshot>();
    assert_send_sync::<crate::Patch>();
    let session = Session::memory().unwrap();
    let snapshot = session.snapshot();
    assert_eq!(snapshot.revision(), Revision::ZERO);
    assert!(snapshot.contains_record(snapshot.root()));
    assert_eq!(snapshot.records().count(), 1);
}

#[test]
fn stored_reference_arrays_must_be_canonical() {
    let first = RecordId::new();
    let second = RecordId::new();
    let (high, low) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    let stored = crate::component::StoredComponent {
        schema: 1,
        payload: vec![],
        record_refs: vec![high, low],
        blob_refs: vec![],
    };
    assert!(ComponentRecord::from_stored(stored).is_err());
}

#[test]
fn commits_component_and_reopens() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("document.khs");
    let (record, document) = {
        let mut session = Session::create(&path).unwrap();
        let document = session.document_id();
        let mut edit = session.snapshot().edit();
        let record = edit.insert_record().unwrap();
        edit.set_component(record, key("unknown"), value(b"opaque"))
            .unwrap();
        let result = session.commit(edit.finish().unwrap()).unwrap();
        assert_eq!(result.revision, Revision::new(1));
        (record, document)
    };
    let session = Session::open(&path).unwrap();
    assert_eq!(session.document_id(), document);
    assert_eq!(
        session
            .snapshot()
            .component(record, &key("unknown"))
            .unwrap()
            .unwrap()
            .payload(),
        b"opaque"
    );
}

#[test]
fn independent_components_merge_but_same_component_conflicts() {
    let mut session = Session::memory().unwrap();
    let root = session.snapshot().root();
    let base = session.snapshot();
    let left = base
        .patch(|edit| edit.set_component(root, key("left"), value(b"left")))
        .unwrap();
    let right = base
        .patch(|edit| edit.set_component(root, key("right"), value(b"right")))
        .unwrap();
    let merged = Patch::merge([&left, &right]).unwrap();
    let committed = session.commit(merged).unwrap();
    assert_eq!(
        committed
            .snapshot
            .component(root, &key("left"))
            .unwrap()
            .unwrap()
            .payload(),
        b"left"
    );
    assert_eq!(
        committed
            .snapshot
            .component(root, &key("right"))
            .unwrap()
            .unwrap()
            .payload(),
        b"right"
    );

    let base = committed.snapshot;
    let one = base
        .patch(|edit| edit.set_component(root, key("same"), value(b"1")))
        .unwrap();
    let two = base
        .patch(|edit| edit.set_component(root, key("same"), value(b"2")))
        .unwrap();
    assert!(matches!(
        Patch::merge([&one, &two]),
        Err(Error::PatchConflict(_))
    ));
}

#[test]
fn descendant_patch_can_edit_ancestor_record() {
    let mut session = Session::memory().unwrap();
    let base = session.snapshot();
    let mut edit = base.edit();
    let record = edit.insert_record().unwrap();
    let ancestor = edit.finish().unwrap();
    let preview = base.preview([&ancestor]).unwrap();
    let descendant = preview
        .patch(|edit| edit.set_component(record, key("late"), value(b"ready")))
        .unwrap();
    assert!(base.preview([&descendant]).is_err());
    let merged = Patch::merge([&ancestor, &descendant]).unwrap();
    let committed = session.commit(merged).unwrap();
    assert_eq!(
        committed
            .snapshot
            .component(record, &key("late"))
            .unwrap()
            .unwrap()
            .payload(),
        b"ready"
    );
}

#[test]
fn record_references_are_indexed_and_protect_targets() {
    let snapshot = Session::memory().unwrap().snapshot();
    let mut edit = snapshot.edit();
    let target = edit.insert_record().unwrap();
    let owner = edit.insert_record().unwrap();
    edit.set_component(
        owner,
        key("reference"),
        ComponentRecord::new(1, vec![], [target], []).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        edit.remove_record(target),
        Err(Error::RecordReferenced { record, .. }) if record == target
    ));
    edit.remove_component(owner, &key("reference")).unwrap();
    edit.remove_record(target).unwrap();
}

#[test]
fn blobs_are_content_addressed_lazy_and_pinned() {
    let mut session = Session::memory().unwrap();
    let root = session.snapshot().root();
    let old_snapshot = session.snapshot();
    let mut edit = old_snapshot.edit();
    let blob = edit.attach_blob(Arc::<[u8]>::from(&b"blob bytes"[..]));
    edit.set_component(
        root,
        key("asset"),
        ComponentRecord::new(1, vec![], [], [blob]).unwrap(),
    )
    .unwrap();
    let committed = session.commit(edit.finish().unwrap()).unwrap();
    assert_eq!(&*committed.snapshot.read_blob(blob).unwrap(), b"blob bytes");

    let patch = committed
        .snapshot
        .patch(|edit| edit.remove_component(root, &key("asset")))
        .unwrap();
    session.commit(patch).unwrap();
    // The committed snapshot still pins the blob.
    assert_eq!(session.gc().unwrap().blobs, 0);
    drop(committed);
    // Reversible history still pins it until pruned.
    session
        .prune_history(session.revision().next().unwrap())
        .unwrap();
    assert!(!session.snapshot().has_blob(blob));
}

#[test]
fn blob_pins_are_shared_across_sessions_for_one_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pins.khs");
    let mut first = Session::create(&path).unwrap();
    let root = first.snapshot().root();
    let mut edit = first.snapshot().edit();
    let blob = edit.attach_blob(Arc::<[u8]>::from(&b"shared pin"[..]));
    edit.set_component(
        root,
        key("shared-asset"),
        ComponentRecord::new(1, vec![], [], [blob]).unwrap(),
    )
    .unwrap();
    first.commit(edit.finish().unwrap()).unwrap();

    let mut second = Session::open(&path).unwrap();
    let patch = second
        .snapshot()
        .patch(|edit| edit.remove_component(root, &key("shared-asset")))
        .unwrap();
    second.commit(patch).unwrap();
    second
        .prune_history(second.revision().next().unwrap())
        .unwrap();
    assert!(second.snapshot().has_blob(blob));

    drop(first);
    assert_eq!(second.gc().unwrap().blobs, 1);
    assert!(!second.snapshot().has_blob(blob));
}

#[test]
fn undo_is_a_new_revision_and_restores_unknown_bytes() {
    let mut session = Session::memory().unwrap();
    let root = session.snapshot().root();
    let first = session
        .snapshot()
        .patch(|edit| edit.set_component(root, key("opaque"), value(b"one")))
        .unwrap();
    let first = session.commit(first).unwrap();
    let second = first
        .snapshot
        .patch(|edit| edit.set_component(root, key("opaque"), value(b"two")))
        .unwrap();
    let second = session.commit(second).unwrap();
    let undone = session.undo(second.revision).unwrap();
    assert_eq!(undone.revision, Revision::new(3));
    assert_eq!(
        undone
            .snapshot
            .component(root, &key("opaque"))
            .unwrap()
            .unwrap()
            .payload(),
        b"one"
    );
}

#[test]
fn stale_writer_never_rebases() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shared.khs");
    let mut first = Session::create(&path).unwrap();
    let mut second = Session::open(&path).unwrap();
    let root = first.snapshot().root();
    let patch = first
        .snapshot()
        .patch(|edit| edit.set_component(root, key("a"), value(b"a")))
        .unwrap();
    first.commit(patch).unwrap();
    let stale = second
        .snapshot()
        .patch(|edit| edit.set_component(root, key("b"), value(b"b")))
        .unwrap();
    assert!(matches!(
        second.commit(stale),
        Err(Error::RevisionConflict { expected, actual })
            if expected == Revision::ZERO && actual == Revision::new(1)
    ));
    let changes = second.refresh().unwrap();
    assert_eq!(changes.to, Revision::new(1));
}

#[test]
fn backup_is_a_valid_independent_document() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.khs");
    let backup = directory.path().join("backup.khs");
    let mut session = Session::create(&source).unwrap();
    let root = session.snapshot().root();
    let patch = session
        .snapshot()
        .patch(|edit| edit.set_component(root, key("x"), value(b"x")))
        .unwrap();
    session.commit(patch).unwrap();
    session.backup(&backup).unwrap();
    let reopened = Session::open(&backup).unwrap();
    assert_eq!(reopened.revision(), Revision::new(1));
    assert_eq!(
        reopened
            .snapshot()
            .component(root, &key("x"))
            .unwrap()
            .unwrap()
            .payload(),
        b"x"
    );
}

#[test]
fn insert_then_remove_is_an_empty_patch() {
    let snapshot = Session::memory().unwrap().snapshot();
    let mut edit = snapshot.edit();
    edit.insert_record_with_id(RecordId::new()).unwrap();
    // Use a second ID so the test also exercises explicit identity insertion.
    let id = RecordId::new();
    edit.insert_record_with_id(id).unwrap();
    edit.remove_record(id).unwrap();
    // The first explicit record remains, so this is not empty yet.
    assert!(!edit.finish().unwrap().is_empty());

    let mut edit = snapshot.edit();
    let id = edit.insert_record().unwrap();
    edit.remove_record(id).unwrap();
    assert!(edit.finish().unwrap().is_empty());
}

#[test]
fn patch_identity_effects_and_exact_ancestry_are_stable() {
    let session = Session::memory().unwrap();
    let base = session.snapshot();
    let root = base.root();
    let patch = base
        .patch(|edit| edit.set_component(root, key("identity"), value(b"one")))
        .unwrap();
    assert!(patch.has_exact_input(&base));
    assert!(!patch.effects().changes_record_lifecycle());
    assert_eq!(patch.effects().components().count(), 1);
    assert_eq!(patch.fingerprint(), patch.clone().fingerprint());
    assert_ne!(
        patch.fingerprint(),
        patch.clone().with_label("different").fingerprint()
    );

    let preview = base.preview([&patch]).unwrap();
    assert!(!patch.has_exact_input(&preview));
    let descendant = preview
        .patch(|edit| edit.set_component(root, key("descendant"), value(b"two")))
        .unwrap();
    assert!(descendant.has_exact_input(&preview));
    assert!(!descendant.has_exact_input(&base));
}

#[test]
fn merged_change_set_reports_net_effects_without_scanning_semantics() {
    let mut session = Session::memory().unwrap();
    let base = session.snapshot();
    let mut ancestor_edit = base.edit();
    let record = ancestor_edit.insert_record().unwrap();
    ancestor_edit
        .set_component(record, key("value"), value(b"one"))
        .unwrap();
    let ancestor = ancestor_edit.finish().unwrap();
    let preview = base.preview([&ancestor]).unwrap();
    let descendant = preview
        .patch(|edit| edit.set_component(record, key("value"), value(b"two")))
        .unwrap();
    let merged = Patch::merge([&ancestor, &descendant]).unwrap();
    let committed = session.commit(merged).unwrap();
    assert_eq!(
        committed.changes.records,
        vec![crate::RecordChange::Inserted(record)]
    );
    assert_eq!(committed.changes.components.len(), 1);
    assert_eq!(
        committed.changes.components[0].kind,
        crate::ValueChangeKind::Inserted
    );
    assert_eq!(
        committed
            .snapshot
            .component(record, &key("value"))
            .unwrap()
            .unwrap()
            .payload(),
        b"two"
    );
}
