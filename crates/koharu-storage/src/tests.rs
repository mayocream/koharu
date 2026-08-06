use std::{collections::BTreeSet, sync::Arc};

use tempfile::tempdir;

use crate::{Blob, Commit, DocumentId, Error, Options, Revision, Session};

fn commit(document: DocumentId, parent: Revision, forward: &[u8]) -> Commit {
    Commit::new(document, parent, forward.to_vec(), b"inverse".to_vec())
}

#[test]
fn project_reopens_with_history_and_blobdb_payloads() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("project.khrproj");
    let document = DocumentId::new();
    let bytes = vec![0x5a; 128 * 1024];
    let blob = Blob::new(Arc::<[u8]>::from(bytes.clone()));
    let id = blob.id();
    {
        let mut session = Session::create(&path, document, b"initial".to_vec()).unwrap();
        let mut change = commit(document, Revision::ZERO, b"forward");
        change.attach(blob);
        session.commit(change, None).unwrap();
        session
            .compact(Revision::ZERO, b"current".to_vec(), BTreeSet::from([id]))
            .unwrap();
    }

    assert!(
        std::fs::read_dir(&path)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "blob")),
        "the blobs column family should materialize large values in BlobDB files"
    );
    let session = Session::open(&path).unwrap();
    let recovery = session.recover().unwrap();
    assert_eq!(recovery.document, document);
    assert_eq!(recovery.head, Revision::new(1));
    assert_eq!(
        session
            .snapshot(BTreeSet::from([id]))
            .unwrap()
            .read_blob(id)
            .unwrap()
            .as_ref(),
        bytes
    );
}

#[test]
fn stale_writer_is_rejected_at_the_database_head() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("project.khrproj");
    let document = DocumentId::new();
    let mut first = Session::create(&path, document, Vec::new()).unwrap();
    let mut stale = Session::open(&path).unwrap();

    first
        .commit(commit(document, Revision::ZERO, b"one"), None)
        .unwrap();
    let error = stale
        .commit(commit(document, Revision::ZERO, b"two"), None)
        .unwrap_err();
    assert!(matches!(
        error,
        Error::RevisionConflict {
            expected: Revision::ZERO,
            actual
        } if actual == Revision::new(1)
    ));
}

#[test]
fn refreshed_writer_preserves_the_latest_checkpoint_boundary() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("project.khrproj");
    let document = DocumentId::new();
    let mut checkpointing = Session::create_with(
        &path,
        document,
        b"zero".to_vec(),
        Options {
            checkpoint_commits: 1,
            checkpoint_bytes: 0,
            ..Options::default()
        },
    )
    .unwrap();
    let mut refreshed = Session::open_with(
        &path,
        Options {
            checkpoint_commits: 0,
            checkpoint_bytes: 0,
            ..Options::default()
        },
    )
    .unwrap();

    checkpointing
        .commit(
            commit(document, Revision::ZERO, b"one"),
            Some(b"checkpoint-one".to_vec()),
        )
        .unwrap();
    let changes = refreshed.changes().unwrap();
    refreshed.accept(&changes).unwrap();
    refreshed
        .commit(commit(document, Revision::new(1), b"two"), None)
        .unwrap();
    drop((checkpointing, refreshed));

    let recovery = Session::open(&path).unwrap().recover().unwrap();
    assert_eq!(recovery.checkpoint_revision, Revision::new(1));
    assert_eq!(recovery.checkpoint.as_ref(), b"checkpoint-one");
    assert_eq!(
        recovery
            .entries
            .iter()
            .map(|entry| entry.revision)
            .collect::<Vec<_>>(),
        [Revision::new(2)]
    );
}

#[test]
fn preview_blobs_are_not_persisted() {
    let document = DocumentId::new();
    let session = Session::memory(document, Vec::new()).unwrap();
    let blob = Blob::new(&b"preview"[..]);
    let id = blob.id();
    let snapshot = session
        .snapshot(BTreeSet::new())
        .unwrap()
        .preview(Revision::ZERO, [blob], BTreeSet::from([id]))
        .unwrap();
    assert_eq!(snapshot.read_blob(id).unwrap().as_ref(), b"preview");
    assert!(matches!(
        session.snapshot(BTreeSet::from([id])).unwrap_err(),
        Error::BlobNotFound(missing) if missing == id
    ));
}

#[test]
fn configured_checkpoint_threshold_is_enforced() {
    let document = DocumentId::new();
    let mut session = Session::memory_with(
        document,
        b"zero".to_vec(),
        Options {
            checkpoint_commits: 1,
            checkpoint_bytes: 0,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(matches!(
        session
            .commit(commit(document, Revision::ZERO, b"one"), None)
            .unwrap_err(),
        Error::Invalid(_)
    ));
    session
        .commit(
            commit(document, Revision::ZERO, b"one"),
            Some(b"checkpoint".to_vec()),
        )
        .unwrap();
    assert_eq!(
        session.recover().unwrap().checkpoint.as_ref(),
        b"checkpoint"
    );
}
