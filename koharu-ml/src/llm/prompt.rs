use minijinja::{Environment, context};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::llm::ModelId;

#[derive(Debug, Clone, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ChatRole {
    #[strum(to_string = "{0}")]
    Name(String),
    System,
    User,
    Assistant,
}

impl Serialize for ChatRole {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ChatRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let role = match value.to_lowercase().as_str() {
            "system" => ChatRole::System,
            "user" => ChatRole::User,
            "assistant" => ChatRole::Assistant,
            _ => ChatRole::Name(value),
        };
        Ok(role)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

// Chat template renderer using MiniJinja
pub struct PromptRenderer {
    env: Environment<'static>,
    model_id: ModelId,
    template: String,
    bos_token: String,
    eos_token: String,
}

impl PromptRenderer {
    pub fn new(model_id: ModelId, template: String, bos_token: String, eos_token: String) -> Self {
        let mut env = Environment::new();

        // Add custom filters that are commonly used in chat templates
        env.add_filter("trim", |s: String| s.trim().to_string());

        Self {
            env,
            model_id,
            template,
            bos_token,
            eos_token,
        }
    }

    fn messages(&self, text: impl Into<String>) -> Vec<ChatMessage> {
        match self.model_id {
            // refer: https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf#translation-prompt
            ModelId::VntlLlama3_8Bv2 => vec![
                ChatMessage::new(ChatRole::Name("Japanese".to_string()), text),
                ChatMessage::new(ChatRole::Name("English".to_string()), String::new()),
            ],
            ModelId::Lfm2_350mEnjpMt => vec![
                ChatMessage::new(
                    ChatRole::System,
                    "Translate to English, do not add any explanations, do not add or delete line breaks.",
                ),
                ChatMessage::new(ChatRole::User, text),
            ],
            ModelId::SakuraGalTransl7Bv3_7 | ModelId::Sakura1_5bQwen2_5v1_0 => vec![
                ChatMessage::new(
                    ChatRole::System,
                    "你是一个视觉小说翻译模型，可以通顺地使用给定的术语表以指定的风格将日文翻译成简体中文，并联系上下文正确使用人称代词，注意不要混淆使役态和被动态的主语和宾语，不要擅自添加原文中没有的特殊符号，也不要擅自增加或减少换行。",
                ),
                ChatMessage::new(ChatRole::User, text),
            ],
        }
    }

    pub fn format_chat_prompt(&self, prompt: String) -> anyhow::Result<String> {
        let messages = self.messages(prompt);
        let tmpl = self.env.template_from_str(&self.template)?;

        tmpl.render(context! {
            messages => messages,
            bos_token => self.bos_token,
            eos_token => self.eos_token,
            add_generation_prompt => true,
        })
        .map_err(anyhow::Error::msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llama_prompt_format() -> anyhow::Result<()> {
        let model_id = ModelId::VntlLlama3_8Bv2;
        let renderer = PromptRenderer::new(
            model_id,
            r#"{{- bos_token }} {%- if custom_tools is defined %} {%- set tools = custom_tools %} {%- endif %} {%- if not tools_in_user_message is defined %} {%- set tools_in_user_message = true %} {%- endif %} {%- if not date_string is defined %} {%- if strftime_now is defined %} {%- set date_string = strftime_now("%d %b %Y") %} {%- else %} {%- set date_string = "26 Jul 2024" %} {%- endif %} {%- endif %} {%- if not tools is defined %} {%- set tools = none %} {%- endif %} {#- This block extracts the system message, so we can slot it into the right place. #} {%- if messages[0]['role'] == 'system' %} {%- set system_message = messages[0]['content']|trim %} {%- set messages = messages[1:] %} {%- else %} {%- set system_message = "" %} {%- endif %} {#- System message #} {{- "<|start_header_id|>Metadata<|end_header_id|>\n\n" }} {{- system_message }} {{- "<|eot_id|>" }} {#- Custom tools are passed in a user message with some extra guidance #} {%- if tools_in_user_message and not tools is none %} {#- Extract the first user message so we can plug it in here #} {%- if messages | length != 0 %} {%- set first_user_message = messages[0]['content']|trim %} {%- set messages = messages[1:] %} {%- else %} {{- raise_exception("Cannot put tools in the first user message when there's no first user message!") }} {%- endif %} {{- '<|start_header_id|>user<|end_header_id|>\n\n' -}} {{- "Given the following functions, please respond with a JSON for a function call " }} {{- "with its proper arguments that best answers the given prompt.\n\n" }} {{- 'Respond in the format {"name": function name, "parameters": dictionary of argument name and its value}.' }} {{- "Do not use variables.\n\n" }} {%- for t in tools %} {{- t | tojson(indent=4) }} {{- "\n\n" }} {%- endfor %} {{- first_user_message + "<|eot_id|>"}} {%- endif %} {%- for message in messages %} {%- if not (message.role == 'ipython' or message.role == 'tool' or 'tool_calls' in message) %} {{- '<|start_header_id|>' + message['role'] + '<|end_header_id|>\n\n'+ message['content'] | trim + '<|eot_id|>' }} {%- elif 'tool_calls' in message %} {%- if not message.tool_calls|length == 1 %} {{- raise_exception("This model only supports single tool-calls at once!") }} {%- endif %} {%- set tool_call = message.tool_calls[0].function %} {{- '<|start_header_id|>assistant<|end_header_id|>\n\n' -}} {{- '{"name": "' + tool_call.name + '", ' }} {{- '"parameters": ' }} {{- tool_call.arguments | tojson }} {{- "}" }} {{- "<|eot_id|>" }} {%- elif message.role == "tool" or message.role == "ipython" %} {{- "<|start_header_id|>ipython<|end_header_id|>\n\n" }} {%- if message.content is mapping or message.content is iterable %} {{- message.content | tojson }} {%- else %} {{- message.content }} {%- endif %} {{- "<|eot_id|>" }} {%- endif %} {%- endfor %} {%- if add_generation_prompt %} {{- '<|start_header_id|>assistant<|end_header_id|>\n\n' }} {%- endif %}"#.to_string(),
            "<|begin_of_text|>".to_string(),
            "<|end_of_text|>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("こんにちは".to_string())?;
        let expected = "<|begin_of_text|><|start_header_id|>Metadata<|end_header_id|>\n\n<|eot_id|><|start_header_id|>Japanese<|end_header_id|>\n\nこんにちは<|eot_id|><|start_header_id|>English<|end_header_id|>\n\n<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n";
        assert_eq!(formatted, expected);

        Ok(())
    }
}
