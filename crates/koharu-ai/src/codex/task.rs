use serde::Serialize;

use super::responses::CodexInputItem;

#[derive(Debug, Clone, Serialize)]
pub struct CodexTaskRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<CodexInputItem>,
    pub stream: bool,
    pub store: bool,
}

impl CodexTaskRequest {
    pub fn new(
        model: impl Into<String>,
        instructions: impl Into<String>,
        input: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            instructions: instructions.into(),
            input: vec![CodexInputItem::user_text(input)],
            stream: true,
            store: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_responses_input_list() {
        let request = CodexTaskRequest::new("gpt-5.5", "You are helpful.", "Who are you?");
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["instructions"], "You are helpful.");
        assert!(value["input"].is_array());
        assert_eq!(value["input"][0]["type"], "message");
        assert_eq!(value["input"][0]["role"], "user");
        assert_eq!(value["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(value["input"][0]["content"][0]["text"], "Who are you?");
        assert_eq!(value["stream"], true);
        assert_eq!(value["store"], false);
    }
}
