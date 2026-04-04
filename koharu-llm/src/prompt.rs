use minijinja::{Environment, context};
use serde::Serialize;
use strum::{Display, EnumString};

use crate::{Language, ModelId};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
}

pub struct PromptRenderer {
    env: Environment<'static>,
    model_id: ModelId,
    template: String,
    bos_token: String,
    eos_token: String,
}

const BLOCK_TAG_INSTRUCTIONS: &str = "The input uses numbered tags like <|1|>, <|2|>, etc. to mark each text block. Translate only the text after each tag. Keep every tag exactly unchanged, including numbers and order. Output the same tags followed by the translated text. Do not merge, split, or reorder blocks.";

pub fn system_prompt(target_language: Language) -> String {
    format!(
        "You are a professional manga translator. Translate Japanese manga dialogue into natural {} that fits inside speech bubbles. Preserve character voice, emotional tone, relationship nuance, emphasis, and sound effects naturally. Keep the wording concise. Do not add notes, explanations, or romanization. {BLOCK_TAG_INSTRUCTIONS}",
        target_language
    )
}

impl PromptRenderer {
    pub fn new(model_id: ModelId, template: String, bos_token: String, eos_token: String) -> Self {
        let mut env = Environment::new();
        env.add_filter("trim", |s: String| s.trim().to_string());

        Self {
            env,
            model_id,
            template,
            bos_token,
            eos_token,
        }
    }

    fn messages(&self, text: impl Into<String>, target_language: Language) -> Vec<ChatMessage> {
        let text = text.into();

        match self.model_id {
            ModelId::VntlLlama3_8Bv2 => vec![
                ChatMessage::new(ChatRole::System, system_prompt(target_language)),
                ChatMessage::new(ChatRole::Name(Language::Japanese.to_string()), text),
                ChatMessage::new(ChatRole::Name(target_language.to_string()), String::new()),
            ],
            ModelId::Lfm2_350mEnjpMt => vec![
                ChatMessage::new(
                    ChatRole::System,
                    format!(
                        "{} Do not add or delete line breaks inside a block.",
                        system_prompt(target_language)
                    ),
                ),
                ChatMessage::new(ChatRole::User, text),
            ],
            ModelId::HunyuanMT7B => vec![ChatMessage::new(
                ChatRole::User,
                format!("{}\n\n{}", system_prompt(target_language), text),
            )],
            ModelId::SakuraGalTransl7Bv3_7 | ModelId::Sakura1_5bQwen2_5v1_0 => vec![
                ChatMessage::new(ChatRole::System, system_prompt(target_language)),
                ChatMessage::new(ChatRole::User, text),
            ],
        }
    }

    pub fn format_chat_prompt(
        &self,
        prompt: String,
        target_language: Language,
    ) -> anyhow::Result<String> {
        let messages = self.messages(prompt, target_language);
        let tmpl = self.env.template_from_str(&self.template)?;

        let prompt = tmpl
            .render(context! {
                messages => messages,
                bos_token => self.bos_token,
                eos_token => self.eos_token,
                add_generation_prompt => !matches!(self.model_id, ModelId::VntlLlama3_8Bv2),
            })
            .map_err(anyhow::Error::msg)?;

        if self.model_id == ModelId::VntlLlama3_8Bv2 {
            Ok(prompt.trim_end_matches("<|eot_id|>").to_string())
        } else {
            Ok(prompt)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_mentions_target_language_and_block_rules() {
        let prompt = system_prompt(Language::Korean);
        assert!(prompt.contains("natural Korean"));
        assert!(prompt.contains("<|1|>, <|2|>"));
        assert!(prompt.contains("Do not merge"));
    }

    #[test]
    fn vntl_prompt_format_keeps_named_role_patch() -> anyhow::Result<()> {
        let renderer = PromptRenderer::new(
            ModelId::VntlLlama3_8Bv2,
            r#"{{- bos_token }} {%- if custom_tools is defined %} {%- set tools = custom_tools %} {%- endif %} {%- if not tools_in_user_message is defined %} {%- set tools_in_user_message = true %} {%- endif %} {%- if not date_string is defined %} {%- if strftime_now is defined %} {%- set date_string = strftime_now("%d %b %Y") %} {%- else %} {%- set date_string = "26 Jul 2024" %} {%- endif %} {%- endif %} {%- if not tools is defined %} {%- set tools = none %} {%- endif %} {%- if messages[0]['role'] == 'system' %} {%- set system_message = messages[0]['content']|trim %} {%- set messages = messages[1:] %} {%- else %} {%- set system_message = "" %} {%- endif %} {{- "<|start_header_id|>Metadata<|end_header_id|>\n\n" }} {{- system_message }} {{- "<|eot_id|>" }} {%- for message in messages %} {{- '<|start_header_id|>' + message['role'] + '<|end_header_id|>\n\n'+ message['content'] | trim + '<|eot_id|>' }} {%- endfor %} {%- if add_generation_prompt %} {{- '<|start_header_id|>assistant<|end_header_id|>\n\n' }} {%- endif %}"#.to_string(),
            "<|begin_of_text|>".to_string(),
            "<|end_of_text|>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("hello".to_string(), Language::English)?;
        let expected = format!(
            "<|begin_of_text|><|start_header_id|>Metadata<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>Japanese<|end_header_id|>\n\nhello<|eot_id|><|start_header_id|>English<|end_header_id|>\n\n",
            system_prompt(Language::English)
        );
        assert_eq!(formatted, expected);

        Ok(())
    }

    #[test]
    fn lfm2_prompt_format_keeps_line_break_patch() -> anyhow::Result<()> {
        let renderer = PromptRenderer::new(
            ModelId::Lfm2_350mEnjpMt,
            r#"{{- bos_token -}}{%- set system_prompt = "" -%}{%- set ns = namespace(system_prompt="") -%}{%- if messages[0]["role"] == "system" -%} {%- set ns.system_prompt = messages[0]["content"] -%} {%- set messages = messages[1:] -%}{%- endif -%}{%- if ns.system_prompt -%} {{- "<|im_start|>system " + ns.system_prompt + "<|im_end|> " -}}{%- endif -%}{%- for message in messages -%} {{- "<|im_start|>" + message["role"] + " " -}}{{- message["content"] + "<|im_end|> " -}}{%- endfor -%}{%- if add_generation_prompt -%} {{- "<|im_start|>assistant " -}}{%- endif -%}"#.to_string(),
            "<|begin_of_text|>".to_string(),
            "<|end_of_text|>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("hello".to_string(), Language::English)?;
        let system = system_prompt(Language::English);
        let expected = format!(
            "<|begin_of_text|><|im_start|>system {system} Do not add or delete line breaks inside a block.<|im_end|> <|im_start|>user hello<|im_end|> <|im_start|>assistant "
        );
        assert_eq!(formatted, expected);

        Ok(())
    }

    #[test]
    fn qwen_prompt_format_uses_shared_prompt() -> anyhow::Result<()> {
        let renderer = PromptRenderer::new(
            ModelId::SakuraGalTransl7Bv3_7,
            r#"{% for message in messages %}{% if message['role'] == 'user' %}{{'<|im_start|>user ' + message['content'] + '<|im_end|> '}}{% elif message['role'] == 'assistant' %}{{'<|im_start|>assistant ' + message['content'] + '<|im_end|> ' }}{% else %}{{ '<|im_start|>system ' + message['content'] + '<|im_end|> ' }}{% endif %}{% endfor %}{% if add_generation_prompt %}{{ '<|im_start|>assistant ' }}{% endif %}"#.to_string(),
            "<s>".to_string(),
            "</s>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("hello".to_string(), Language::Korean)?;
        let expected = format!(
            "<|im_start|>system {}<|im_end|> <|im_start|>user hello<|im_end|> <|im_start|>assistant ",
            system_prompt(Language::Korean)
        );
        assert_eq!(formatted, expected);

        Ok(())
    }

    #[test]
    fn hunyuan_prompt_keeps_single_user_message_patch() -> anyhow::Result<()> {
        let renderer = PromptRenderer::new(
            ModelId::HunyuanMT7B,
            "{{ messages[0]['content'] }}".to_string(),
            "<s>".to_string(),
            "</s>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("hello".to_string(), Language::Korean)?;
        assert_eq!(
            formatted,
            format!("{}\n\nhello", system_prompt(Language::Korean))
        );

        Ok(())
    }
}
