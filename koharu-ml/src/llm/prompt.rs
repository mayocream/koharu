use strum::{Display, EnumString};

use crate::llm::ModelId;

#[derive(Debug, Clone, Copy)]
pub struct Markers {
    pub prefix: Option<&'static str>,
    pub role_start: Option<&'static str>,
    pub role_end: Option<&'static str>,
    pub message_end: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ChatRole {
    #[strum(to_string = "{0}")]
    Name(&'static str),
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    pub const fn assistant() -> Self {
        Self {
            role: ChatRole::Assistant,
            content: String::new(),
        }
    }
}

impl ModelId {
    pub fn markers(&self) -> Markers {
        match self {
            // Llama3
            // refer: https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf#translation-prompt
            ModelId::VntlLlama3_8Bv2 => Markers {
                prefix: Some("<|begin_of_text|>"),
                role_start: Some("<|start_header_id|>"),
                role_end: Some("<|end_header_id|>"),
                message_end: "<|eot_id|>",
            },
            // Qwen2
            ModelId::Sakura1_5bQwen2_5v1_0 | ModelId::SakuraGalTransl7Bv3_7 => Markers {
                prefix: None,
                role_start: Some("<|im_start|>"),
                role_end: None,
                message_end: "<|im_end|>",
            },
            // LFM2
            ModelId::Lfm2_350mEnjpMt => Markers {
                prefix: Some("<|startoftext|>"),
                role_start: Some("<|im_start|>"),
                role_end: Some("<|im_end|>"),
                message_end: "<|im_end|>",
            },
        }
    }

    pub fn prompt(&self, text: impl Into<String>) -> Vec<ChatMessage> {
        match self {
            // refer: https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf#translation-prompt
            ModelId::VntlLlama3_8Bv2 => vec![
                ChatMessage::new(ChatRole::Name("Japanese"), text),
                ChatMessage::new(ChatRole::Name("English"), String::new()),
            ],
            ModelId::Lfm2_350mEnjpMt => vec![
                ChatMessage::new(
                    ChatRole::System,
                    "Translate to English, do not add any explanations, do not add or delete line breaks.",
                ),
                ChatMessage::new(ChatRole::User, text),
                ChatMessage::assistant(),
            ],
            ModelId::SakuraGalTransl7Bv3_7 | ModelId::Sakura1_5bQwen2_5v1_0 => vec![
                ChatMessage::new(
                    ChatRole::System,
                    "你是一个视觉小说翻译模型，可以通顺地使用给定的术语表以指定的风格将日文翻译成简体中文，并联系上下文正确使用人称代词，注意不要混淆使役态和被动态的主语和宾语，不要擅自添加原文中没有的特殊符号，也不要擅自增加或减少换行。",
                ),
                ChatMessage::new(ChatRole::User, text),
                ChatMessage::assistant(),
            ],
        }
    }

    pub fn format_chat_prompt(&self, messages: &[ChatMessage]) -> String {
        let markers = self.markers();
        let mut out = String::new();

        // e.g. <|begin_of_text|>
        if let Some(prefix) = markers.prefix {
            out.push_str(prefix);
        }

        for msg in messages {
            // e.g. <|start_header_id|>
            if let Some(role_start) = markers.role_start {
                out.push_str(role_start);
            }
            out.push_str(msg.role.to_string().as_str());

            // e.g. <|end_header_id|>
            if let Some(role_end) = markers.role_end {
                out.push_str(role_end);
            }

            if !msg.content.is_empty() {
                out.push('\n');
                out.push_str(&msg.content);
                // e.g. <|eot_id|>
                out.push_str(markers.message_end);
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llama_prompt_format() {
        let model_id = ModelId::VntlLlama3_8Bv2;
        let messages = model_id.prompt("こんにちは");
        let formatted = model_id.format_chat_prompt(&messages);
        let expected = "<|begin_of_text|><|start_header_id|>Japanese<|end_header_id|>こんにちは<|eot_id|><|start_header_id|>English<|end_header_id|>";
        assert_eq!(formatted, expected);
    }
}
