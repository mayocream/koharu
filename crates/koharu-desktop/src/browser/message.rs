use std::sync::Arc;

use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryAttachment {
    id: Box<str>,
    bytes: Arc<[u8]>,
}

impl BinaryAttachment {
    pub fn new(
        id: impl Into<Box<str>>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<Self, AttachmentError> {
        let id = id.into();
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(AttachmentError::InvalidId);
        }
        Ok(Self {
            id,
            bytes: bytes.into(),
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebMessage {
    /// A serialized `ClientRequest` or `ServerMessage` envelope.
    pub json: Box<str>,
    /// Delivered to JavaScript as `{ [id]: ArrayBuffer }`. JSON refers to an
    /// attachment with `{ "attachment": "<id>" }`.
    pub attachments: Vec<BinaryAttachment>,
}

impl WebMessage {
    #[must_use]
    pub fn json(json: impl Into<Box<str>>) -> Self {
        Self {
            json: json.into(),
            attachments: Vec::new(),
        }
    }

    pub fn with_attachments(
        json: impl Into<Box<str>>,
        attachments: Vec<BinaryAttachment>,
    ) -> Result<Self, AttachmentError> {
        for (index, attachment) in attachments.iter().enumerate() {
            if attachments[..index]
                .iter()
                .any(|candidate| candidate.id == attachment.id)
            {
                return Err(AttachmentError::DuplicateId(attachment.id.to_string()));
            }
        }
        Ok(Self {
            json: json.into(),
            attachments,
        })
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AttachmentError {
    #[error("binary attachment IDs must be non-empty decimal strings")]
    InvalidId,
    #[error("binary attachment ID {0} is duplicated")]
    DuplicateId(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_ids_are_javascript_safe_decimal_strings() {
        assert!(BinaryAttachment::new("18446744073709551615", [1_u8, 2].as_slice()).is_ok());
        assert_eq!(
            BinaryAttachment::new("not-a-number", [1_u8].as_slice()),
            Err(AttachmentError::InvalidId)
        );
    }

    #[test]
    fn duplicate_attachments_are_rejected() {
        let first = BinaryAttachment::new("7", [1_u8].as_slice()).unwrap();
        let second = BinaryAttachment::new("7", [2_u8].as_slice()).unwrap();
        assert_eq!(
            WebMessage::with_attachments("{}", vec![first, second]),
            Err(AttachmentError::DuplicateId("7".into()))
        );
    }
}
