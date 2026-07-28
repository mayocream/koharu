use crate::{EntityId, RelationId};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Storage(#[from] koharu_storage::Error),
    #[error(transparent)]
    Codec(#[from] revision::Error),
    #[error("invalid scene: {0}")]
    Invalid(String),
    #[error("entity {0} is not part of the scene hierarchy")]
    EntityNotFound(EntityId),
    #[error("relation {0} was not found")]
    RelationNotFound(RelationId),
    #[error("entity {0} already has a parent")]
    MultipleParents(EntityId),
    #[error("hierarchy operation would create a cycle")]
    HierarchyCycle,
    #[error("entity {0} has children")]
    NonEmptyEntity(EntityId),
    #[error("entity {0} has incident relations")]
    IncidentRelations(EntityId),
    #[error("component authorship conflict: {0}")]
    Authorship(String),
    #[error("component {kind}@{slot} has unsupported schema {schema}")]
    UnsupportedComponent {
        kind: String,
        slot: String,
        schema: u32,
    },
    #[error("component reference envelope does not match its decoded value: {0}")]
    ReferenceMismatch(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
