use std::fs;

use bytes::Bytes;

use crate::{BlobId, DocumentId, Error, Revision, Session};

fn blob_path(root: &std::path::Path, id: BlobId) -> std::path::PathBuf {
    let name = id.to_string();
    root.join("blobs").join(&name[..2]).join(name)
}

#[tokio::test]
async fn saves_reopens_and_maps_large_blobs() {
    let root = tempfile::tempdir().unwrap();
    let document = DocumentId::new();
    let session = Session::create(root.path(), document, Bytes::from_static(b"initial"))
        .await
        .unwrap();
    let initial = session.load().await.unwrap();
    let bytes = Bytes::from(vec![7; 512 * 1024]);
    let id = BlobId::for_bytes(&bytes);
    let proposed = initial
        .update(
            Revision::new(1),
            Bytes::from_static(b"updated"),
            [id],
            [(id, bytes.clone())],
        )
        .unwrap();
    let saved = session.save(&proposed).await.unwrap();
    assert_eq!(saved.blobs().get(id).await.unwrap(), bytes);
    assert!(!root.path().join("temporary").exists());
    drop((initial, proposed, saved, session));

    let reopened = Session::open(root.path()).await.unwrap();
    let loaded = reopened.load().await.unwrap();
    assert_eq!(loaded.document_id(), document);
    assert_eq!(loaded.revision(), Revision::new(1));
    assert_eq!(loaded.payload(), &Bytes::from_static(b"updated"));
    assert_eq!(loaded.blobs().get(id).await.unwrap(), bytes);
}

#[tokio::test]
async fn corrupt_newest_state_falls_back_to_previous_slot() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::create(
        root.path(),
        DocumentId::new(),
        Bytes::from_static(b"initial"),
    )
    .await
    .unwrap();
    let initial = session.load().await.unwrap();
    let proposed = initial
        .update(Revision::new(1), Bytes::from_static(b"newest"), [], [])
        .unwrap();
    let saved = session.save(&proposed).await.unwrap();
    drop((initial, proposed, saved, session));

    fs::write(root.path().join("state-b.khr"), b"torn").unwrap();
    let reopened = Session::open(root.path()).await.unwrap();
    assert_eq!(reopened.revision(), Revision::ZERO);
    assert_eq!(
        reopened.load().await.unwrap().payload(),
        &Bytes::from_static(b"initial")
    );
}

#[tokio::test]
async fn missing_newest_blob_falls_back_to_previous_slot() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::create(
        root.path(),
        DocumentId::new(),
        Bytes::from_static(b"initial"),
    )
    .await
    .unwrap();
    let initial = session.load().await.unwrap();
    let bytes = Bytes::from_static(b"blob");
    let id = BlobId::for_bytes(&bytes);
    let proposed = initial
        .update(
            Revision::new(1),
            Bytes::from_static(b"newest"),
            [id],
            [(id, bytes)],
        )
        .unwrap();
    let saved = session.save(&proposed).await.unwrap();
    drop((initial, proposed, saved, session));
    fs::remove_file(blob_path(root.path(), id)).unwrap();

    let reopened = Session::open(root.path()).await.unwrap();
    assert_eq!(reopened.revision(), Revision::ZERO);
}

#[tokio::test]
async fn project_lock_lives_as_long_as_blob_scopes() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::create(root.path(), DocumentId::new(), Bytes::new())
        .await
        .unwrap();
    let state = session.load().await.unwrap();
    drop(session);
    assert!(matches!(
        Session::open(root.path()).await,
        Err(Error::Locked)
    ));
    drop(state);
    Session::open(root.path()).await.unwrap();
}

#[tokio::test]
async fn garbage_collection_keeps_both_slots_and_active_states() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::create(root.path(), DocumentId::new(), Bytes::new())
        .await
        .unwrap();
    let initial = session.load().await.unwrap();
    let bytes = Bytes::from_static(b"retained");
    let id = BlobId::for_bytes(&bytes);
    let first_proposed = initial
        .update(
            Revision::new(1),
            Bytes::from_static(b"one"),
            [id],
            [(id, bytes)],
        )
        .unwrap();
    let first = session.save(&first_proposed).await.unwrap();
    drop(first_proposed);
    let second_proposed = first
        .update(Revision::new(2), Bytes::from_static(b"two"), [], [])
        .unwrap();
    let second = session.save(&second_proposed).await.unwrap();
    drop(second_proposed);
    assert_eq!(session.collect_garbage().await.unwrap().blobs, 0);

    let third_proposed = second
        .update(Revision::new(3), Bytes::from_static(b"three"), [], [])
        .unwrap();
    let third = session.save(&third_proposed).await.unwrap();
    drop(third_proposed);
    drop((initial, first, second, third));
    let report = session.collect_garbage().await.unwrap();
    assert_eq!(report.blobs, 1);
    assert!(!blob_path(root.path(), id).exists());
}

#[tokio::test]
async fn rejects_stale_saves() {
    let session = Session::memory(DocumentId::new(), Bytes::new())
        .await
        .unwrap();
    let state = session.load().await.unwrap();
    assert!(matches!(
        session.save(&state).await,
        Err(Error::RevisionConflict { .. })
    ));
}
