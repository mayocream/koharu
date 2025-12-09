use strum::{Display, EnumString};

use crate::llm::{ModelId, model::Model};

#[derive(Debug, Clone, Copy)]
pub struct Markers {
    pub prefix: Option<&'static str>,
    pub role_start: Option<&'static str>,
    pub role_end: Option<&'static str>,
    pub message_end: &'static str,
}

impl Model {
    pub fn markers(&self) -> Markers {
        match self {
            Model::Llama(_) => Markers {
                prefix: Some("<|begin_of_text|>"),
                role_start: Some("<|start_header_id|>"),
                role_end: Some("<|end_header_id|>"),
                message_end: "<|eot_id|>",
            },
            Model::Qwen2(_) => Markers {
                prefix: None,
                role_start: Some("<|im_start|>"),
                role_end: None,
                message_end: "<|im_end|>",
            },
            Model::Lfm2(_) => Markers {
                prefix: Some("<|startoftext|>"),
                role_start: Some("<|im_start|>"),
                role_end: Some("<|im_end|>"),
                message_end: "<|im_end|>",
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ChatRole {
    Name(&'static str),
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
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
    pub fn prompt(&self, text: impl Into<String>) -> Vec<ChatMessage> {
        match self {
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
            ModelId::SakuraGalTransl7Bv3_7 => vec![
                ChatMessage::new(
                    ChatRole::System,
                    "你是一个视觉小说翻译模型，可以通顺地使用给定的术语表以指定的风格将日文翻译成简体中文，并联系上下文正确使用人称代词，注意不要混淆使役态和被动态的主语和宾语，不要擅自添加原文中没有的特殊符号，也不要擅自增加或减少换行。",
                ),
                ChatMessage::new(ChatRole::User, text),
                ChatMessage::assistant(),
            ],
            ModelId::Sakura1_5bQwen2_5v1_0 => vec![
                ChatMessage::new(
                    ChatRole::System,
                    "你是一个轻小说翻译模型，可以通顺地使用给定的术语表以指定的风格将日文翻译成简体中文，并联系上下文正确使用人称代词，注意不要混淆使役态和被动态的主语和宾语，不要擅自添加原文中没有的特殊符号，也不要擅自增加或减少换行。",
                ),
                ChatMessage::new(ChatRole::User, text),
                ChatMessage::assistant(),
            ],
        }
    }
}
