use std::collections::BTreeSet;

use tempfile::tempdir;

use crate::{BlobAttachment, CommitRequest, DocumentId, Error, Options, Revision, Session};

fn request(
    document: DocumentId,
    parent: Revision,
    forward: &[u8],
    inverse: &[u8],
) -> CommitRequest {
    CommitRequest::new(document, parent, forward.to_vec(), inverse.to_vec())
}

#[test]
fn recovers_checkpoint_and_opaque_commit_tail() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("project.khr");
    let document = DocumentId::new();
    let mut writer = Session::create(&path, document, b"checkpoint-0".to_vec()).unwrap();
    let attachment = BlobAttachment::new(&b"asset"[..]);
    let blob = attachment.id();
    let mut commit = request(document, Revision::ZERO, b"forward-1", b"inverse-1");
    commit.attach(attachment);
    let result = writer.commit(commit, None, BTreeSet::from([blob])).unwrap();

    assert_eq!(result.revision, Revision::new(1));
    assert_eq!(result.snapshot.read_blob(blob).unwrap().as_ref(), b"asset");
    writer.flush().unwrap();

    let reader = Session::open(&path).unwrap();
    let recovery = reader.recovery().unwrap();
    assert_eq!(recovery.document, document);
    assert_eq!(recovery.checkpoint_revision, Revision::ZERO);
    assert_eq!(recovery.head, Revision::new(1));
    assert_eq!(recovery.checkpoint.as_ref(), b"checkpoint-0");
    assert_eq!(recovery.commits.len(), 1);
    assert_eq!(recovery.commits[0].forward.as_ref(), b"forward-1");
    assert_eq!(recovery.commits[0].inverse.as_ref(), b"inverse-1");
    assert_eq!(recovery.commits[0].blobs.as_ref(), &[blob]);
    assert_eq!(
        reader
            .snapshot(BTreeSet::from([blob]))
            .read_blob(blob)
            .unwrap()
            .as_ref(),
        b"asset"
    );
}

#[test]
fn stale_writer_is_rejected_by_the_database_head() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("project.khr");
    let document = DocumentId::new();
    let mut first = Session::create(&path, document, Vec::new()).unwrap();
    let mut stale = Session::open(&path).unwrap();

    first
        .commit(
            request(document, Revision::ZERO, b"one", b"undo-one"),
            None,
            BTreeSet::new(),
        )
        .unwrap();
    let error = stale
        .commit(
            request(document, Revision::ZERO, b"two", b"undo-two"),
            None,
            BTreeSet::new(),
        )
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
fn refresh_advances_only_after_the_owner_accepts_it() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("project.khr");
    let document = DocumentId::new();
    let mut writer = Session::create(&path, document, Vec::new()).unwrap();
    let mut reader = Session::open(&path).unwrap();

    writer
        .commit(
            request(document, Revision::ZERO, b"one", b"undo-one"),
            None,
            BTreeSet::new(),
        )
        .unwrap();
    let refresh = reader.prepare_refresh().unwrap();
    assert_eq!(reader.revision(), Revision::ZERO);
    assert_eq!(refresh.from, Revision::ZERO);
    assert_eq!(refresh.to, Revision::new(1));
    assert_eq!(refresh.commits[0].forward.as_ref(), b"one");

    reader.accept_refresh(&refresh).unwrap();
    assert_eq!(reader.revision(), Revision::new(1));
}

#[test]
fn configured_threshold_requires_the_callers_checkpoint() {
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
    let error = session
        .commit(
            request(document, Revision::ZERO, b"one", b"undo-one"),
            None,
            BTreeSet::new(),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Invalid(_)));

    session
        .commit(
            request(document, Revision::ZERO, b"one", b"undo-one"),
            Some(b"checkpoint-1".to_vec()),
            BTreeSet::new(),
        )
        .unwrap();
    let recovery = session.recovery().unwrap();
    assert_eq!(recovery.checkpoint_revision, Revision::new(1));
    assert_eq!(recovery.checkpoint.as_ref(), b"checkpoint-1");
    assert!(recovery.commits.is_empty());
}
