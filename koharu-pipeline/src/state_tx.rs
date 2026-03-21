use anyhow::Result;
use koharu_types::{AppState, Document};
use once_cell::sync::Lazy;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum ChangedField {
    #[strum(serialize = "name")]
    Name,
    #[strum(serialize = "textBlocks")]
    TextBlocks,
    #[strum(serialize = "segment")]
    Segment,
    #[strum(serialize = "brushLayer")]
    BrushLayer,
    #[strum(serialize = "inpainted")]
    Inpainted,
    #[strum(serialize = "rendered")]
    Rendered,
}

#[derive(Debug, Clone)]
pub enum StateEvent {
    DocumentsChanged,
    DocumentChanged {
        document_id: String,
        revision: u64,
        changed: Vec<String>,
    },
}

static STATE_TX: Lazy<broadcast::Sender<StateEvent>> = Lazy::new(|| broadcast::channel(256).0);

pub fn subscribe() -> broadcast::Receiver<StateEvent> {
    STATE_TX.subscribe()
}

fn emit(event: StateEvent) {
    let _ = STATE_TX.send(event);
}

fn serialize_changed_fields(changed: &[ChangedField]) -> Vec<String> {
    changed.iter().map(ToString::to_string).collect()
}

pub async fn read_doc(state: &AppState, index: usize) -> Result<Document> {
    let guard = state.read().await;
    guard
        .documents
        .get(index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))
}

pub async fn list_docs(state: &AppState) -> Vec<Document> {
    state.read().await.documents.clone()
}

pub async fn find_doc_index(state: &AppState, document_id: &str) -> Result<usize> {
    let guard = state.read().await;
    guard
        .documents
        .iter()
        .position(|document| document.id == document_id)
        .ok_or_else(|| anyhow::anyhow!("Document not found: {document_id}"))
}

pub async fn replace_docs(state: &AppState, mut documents: Vec<Document>) -> Result<usize> {
    for document in &mut documents {
        document.prepare_for_store();
    }

    let count = documents.len();
    let mut guard = state.write().await;
    guard.documents = documents;
    drop(guard);
    emit(StateEvent::DocumentsChanged);
    Ok(count)
}

pub async fn append_docs(state: &AppState, mut documents: Vec<Document>) -> Result<usize> {
    for document in &mut documents {
        document.prepare_for_store();
    }

    let mut guard = state.write().await;
    guard.documents.extend(documents);
    let count = guard.documents.len();
    drop(guard);
    emit(StateEvent::DocumentsChanged);
    Ok(count)
}

pub async fn update_doc(
    state: &AppState,
    index: usize,
    mut document: Document,
    changed: &[ChangedField],
) -> Result<()> {
    document.prepare_for_store();
    document.bump_revision();
    let document_id = document.id.clone();
    let revision = document.revision;

    let mut guard = state.write().await;
    let target = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    *target = document;
    drop(guard);
    emit(StateEvent::DocumentChanged {
        document_id,
        revision,
        changed: serialize_changed_fields(changed),
    });
    Ok(())
}

pub async fn mutate_doc<T, F>(
    state: &AppState,
    index: usize,
    changed: &[ChangedField],
    mutator: F,
) -> Result<T>
where
    F: FnOnce(&mut Document) -> Result<T>,
{
    let mut guard = state.write().await;
    let target = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {index}"))?;
    let result = mutator(target)?;
    target.prepare_for_store();
    target.bump_revision();
    let document_id = target.id.clone();
    let revision = target.revision;
    drop(guard);
    emit(StateEvent::DocumentChanged {
        document_id,
        revision,
        changed: serialize_changed_fields(changed),
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        ChangedField, append_docs, find_doc_index, list_docs, mutate_doc, read_doc, replace_docs,
        update_doc,
    };
    use koharu_types::{AppState, Document, State};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn test_state() -> AppState {
        Arc::new(RwLock::new(State {
            documents: vec![Default::default()],
        }))
    }

    #[tokio::test]
    async fn read_update_mutate_doc_round_trip() {
        let state = test_state();

        let mut doc = read_doc(&state, 0).await.expect("doc should exist");
        doc.name = "before".to_string();
        update_doc(&state, 0, doc, &[ChangedField::Name])
            .await
            .expect("update should work");

        mutate_doc(&state, 0, &[ChangedField::Name], |doc| {
            doc.name = "after".to_string();
            Ok(())
        })
        .await
        .expect("mutation should work");

        let doc = read_doc(&state, 0).await.expect("doc should exist");
        assert_eq!(doc.name, "after");
    }

    #[tokio::test]
    async fn index_not_found_errors_are_stable() {
        let state = test_state();
        let err = read_doc(&state, 1)
            .await
            .expect_err("missing document should fail");
        assert_eq!(err.to_string(), "Document not found at index 1");

        let err = mutate_doc(&state, 1, &[ChangedField::Name], |_| Ok(()))
            .await
            .expect_err("missing document should fail");
        assert_eq!(err.to_string(), "Document not found at index 1");
    }

    #[tokio::test]
    async fn replace_append_and_find_doc_work() {
        let state = test_state();
        let first = Document {
            id: "first".to_string(),
            ..Default::default()
        };
        replace_docs(&state, vec![first])
            .await
            .expect("replace should work");
        assert_eq!(list_docs(&state).await.len(), 1);

        let second = Document {
            id: "second".to_string(),
            ..Default::default()
        };
        append_docs(&state, vec![second])
            .await
            .expect("append should work");
        assert_eq!(find_doc_index(&state, "second").await.expect("find"), 1);
    }
}
