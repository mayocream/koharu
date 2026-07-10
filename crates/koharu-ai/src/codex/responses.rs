use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CodexInputItem {
    #[serde(rename = "type")]
    pub item_type: &'static str,
    pub role: &'static str,
    pub content: Vec<CodexInputContent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodexInputContent {
    InputText {
        text: String,
    },
    InputImage {
        image_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl CodexInputItem {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            item_type: "message",
            role: "user",
            content: vec![CodexInputContent::input_text(text)],
        }
    }
}

impl CodexInputContent {
    pub fn input_text(text: impl Into<String>) -> Self {
        Self::InputText { text: text.into() }
    }

    pub fn input_image_url(image_url: impl Into<String>, detail: Option<String>) -> Self {
        Self::InputImage {
            image_url: image_url.into(),
            detail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_input_image_content() {
        let content = CodexInputContent::input_image_url(
            "data:image/png;base64,abc",
            Some("high".to_string()),
        );
        let value = serde_json::to_value(content).unwrap();

        assert_eq!(value["type"], "input_image");
        assert_eq!(value["image_url"], "data:image/png;base64,abc");
        assert_eq!(value["detail"], "high");
    }
}
