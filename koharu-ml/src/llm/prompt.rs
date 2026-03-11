use minijinja::{Environment, context};
use serde::Serialize;
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

// To make minijinja serialize ChatRole
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

const ENGLISH_BLOCK_TAG_INSTRUCTIONS: &str = r#"If the input contains <block id="N">...</block>, translate only the content inside each block. Preserve every block tag, id, order, and block count exactly. Do not merge blocks, split blocks, or add explanations outside the blocks."#;
const HUNYUAN_BLOCK_TAG_INSTRUCTIONS: &str = r#"The input may contain XML-like block tags. Translate only the text inside each block. Keep every tag exactly unchanged, including `<block id="N">` and `</block>`, and preserve the same ids, order, and block count. Output only the translated blocks, with no code fences, labels, or extra text outside the blocks."#;

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

    fn messages(&self, text: impl Into<String>, target_language: Option<&str>) -> Vec<ChatMessage> {
        let text = text.into();

        match self.model_id {
            // refer: https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf#translation-prompt
            ModelId::VntlLlama3_8Bv2 => vec![
                ChatMessage::new(
                    ChatRole::System,
                    format!(
                        "Translate Japanese manga dialogue into natural English for speech bubbles. Keep character voice, emotional tone, and relationship nuances clear. Keep the wording concise enough to fit manga bubbles. Preserve emphasis and sound effects naturally when they appear. Do not add notes, explanations, or romanization. {ENGLISH_BLOCK_TAG_INSTRUCTIONS}"
                    ),
                ),
                ChatMessage::new(ChatRole::Name("Japanese".to_string()), text),
                ChatMessage::new(ChatRole::Name("English".to_string()), String::new()),
            ],
            ModelId::Lfm2_350mEnjpMt => vec![
                ChatMessage::new(
                    ChatRole::System,
                    format!(
                        "Translate Japanese manga dialogue into natural English for speech bubbles. Keep character voice, emotional tone, and relationship nuances clear. Keep the wording concise enough to fit manga bubbles. Preserve emphasis and sound effects naturally when they appear. Do not add notes or explanations, and do not add or delete line breaks inside a block. {ENGLISH_BLOCK_TAG_INSTRUCTIONS}"
                    ),
                ),
                ChatMessage::new(ChatRole::User, text),
            ],
            ModelId::SakuraGalTransl7Bv3_7 | ModelId::Sakura1_5bQwen2_5v1_0 => vec![
                ChatMessage::new(
                    ChatRole::System,
                    r#"你是一个漫画翻译模型。请把日文漫画对白翻译成自然、简洁、适合放进对话气泡的简体中文。保留人物语气、情绪、关系和说话风格，正确处理人称，不要误解使役态和被动态。拟声词、强调和语气词要自然处理；不要添加注释、解释、罗马音或原文里没有的特殊符号，也不要擅自增加或减少换行。如果输入中包含 <block id="N">...</block>，只翻译每个 block 标签内部的内容。必须完整保留所有 block 标签、id、顺序和 block 数量，不要合并 block，不要拆分 block，也不要在 block 之外添加任何内容。"#,
                ),
                ChatMessage::new(ChatRole::User, text),
            ],
            ModelId::HunyuanMT7B => vec![ChatMessage::new(
                ChatRole::User,
                format!(
                    "Translate the following Japanese manga dialogue into {}. Make it read naturally inside manga speech bubbles. Keep character voice, emotional tone, and relationship nuances. Keep the wording concise, and preserve emphasis and sound effects naturally when they appear. Do not add notes or explanations. {}\n\n{}",
                    target_language.unwrap_or("English"),
                    HUNYUAN_BLOCK_TAG_INSTRUCTIONS,
                    text,
                ),
            )],
        }
    }

    pub fn format_chat_prompt(
        &self,
        prompt: String,
        target_language: Option<&str>,
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
            .map_err(anyhow::Error::msg);

        // hotfix the vntl-llama3-8b-v2 extra eos_token issue
        if self.model_id == ModelId::VntlLlama3_8Bv2 {
            prompt.map(|s| s.trim_end_matches("<|eot_id|>").to_string())
        } else {
            prompt
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vntl_prompt_format() -> anyhow::Result<()> {
        let model_id = ModelId::VntlLlama3_8Bv2;
        let renderer = PromptRenderer::new(
            model_id,
            r#"{{- bos_token }} {%- if custom_tools is defined %} {%- set tools = custom_tools %} {%- endif %} {%- if not tools_in_user_message is defined %} {%- set tools_in_user_message = true %} {%- endif %} {%- if not date_string is defined %} {%- if strftime_now is defined %} {%- set date_string = strftime_now("%d %b %Y") %} {%- else %} {%- set date_string = "26 Jul 2024" %} {%- endif %} {%- endif %} {%- if not tools is defined %} {%- set tools = none %} {%- endif %} {#- This block extracts the system message, so we can slot it into the right place. #} {%- if messages[0]['role'] == 'system' %} {%- set system_message = messages[0]['content']|trim %} {%- set messages = messages[1:] %} {%- else %} {%- set system_message = "" %} {%- endif %} {#- System message #} {{- "<|start_header_id|>Metadata<|end_header_id|>\n\n" }} {{- system_message }} {{- "<|eot_id|>" }} {#- Custom tools are passed in a user message with some extra guidance #} {%- if tools_in_user_message and not tools is none %} {#- Extract the first user message so we can plug it in here #} {%- if messages | length != 0 %} {%- set first_user_message = messages[0]['content']|trim %} {%- set messages = messages[1:] %} {%- else %} {{- raise_exception("Cannot put tools in the first user message when there's no first user message!") }} {%- endif %} {{- '<|start_header_id|>user<|end_header_id|>\n\n' -}} {{- "Given the following functions, please respond with a JSON for a function call " }} {{- "with its proper arguments that best answers the given prompt.\n\n" }} {{- 'Respond in the format {"name": function name, "parameters": dictionary of argument name and its value}.' }} {{- "Do not use variables.\n\n" }} {%- for t in tools %} {{- t | tojson(indent=4) }} {{- "\n\n" }} {%- endfor %} {{- first_user_message + "<|eot_id|>"}} {%- endif %} {%- for message in messages %} {%- if not (message.role == 'ipython' or message.role == 'tool' or 'tool_calls' in message) %} {{- '<|start_header_id|>' + message['role'] + '<|end_header_id|>\n\n'+ message['content'] | trim + '<|eot_id|>' }} {%- elif 'tool_calls' in message %} {%- if not message.tool_calls|length == 1 %} {{- raise_exception("This model only supports single tool-calls at once!") }} {%- endif %} {%- set tool_call = message.tool_calls[0].function %} {{- '<|start_header_id|>assistant<|end_header_id|>\n\n' -}} {{- '{"name": "' + tool_call.name + '", ' }} {{- '"parameters": ' }} {{- tool_call.arguments | tojson }} {{- "}" }} {{- "<|eot_id|>" }} {%- elif message.role == "tool" or message.role == "ipython" %} {{- "<|start_header_id|>ipython<|end_header_id|>\n\n" }} {%- if message.content is mapping or message.content is iterable %} {{- message.content | tojson }} {%- else %} {{- message.content }} {%- endif %} {{- "<|eot_id|>" }} {%- endif %} {%- endfor %} {%- if add_generation_prompt %} {{- '<|start_header_id|>assistant<|end_header_id|>\n\n' }} {%- endif %}"#.to_string(),
            "<|begin_of_text|>".to_string(),
            "<|end_of_text|>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("こんにちは".to_string(), None)?;
        let expected = "<|begin_of_text|><|start_header_id|>Metadata<|end_header_id|>\n\nTranslate Japanese manga dialogue into natural English for speech bubbles. Keep character voice, emotional tone, and relationship nuances clear. Keep the wording concise enough to fit manga bubbles. Preserve emphasis and sound effects naturally when they appear. Do not add notes, explanations, or romanization. If the input contains <block id=\"N\">...</block>, translate only the content inside each block. Preserve every block tag, id, order, and block count exactly. Do not merge blocks, split blocks, or add explanations outside the blocks.<|eot_id|><|start_header_id|>Japanese<|end_header_id|>\n\nこんにちは<|eot_id|><|start_header_id|>English<|end_header_id|>\n\n";
        assert_eq!(formatted, expected);

        Ok(())
    }

    #[test]
    fn lfm2_prompt_format() -> anyhow::Result<()> {
        let model_id = ModelId::Lfm2_350mEnjpMt;
        let renderer = PromptRenderer::new(
            model_id,
            r#"{{- bos_token -}}{%- set system_prompt = "" -%}{%- set ns = namespace(system_prompt="") -%}{%- if messages[0]["role"] == "system" -%} {%- set ns.system_prompt = messages[0]["content"] -%} {%- set messages = messages[1:] -%}{%- endif -%}{%- if tools -%} {%- set ns.system_prompt = ns.system_prompt + (" " if ns.system_prompt else "") + "List of tools: <|tool_list_start|>[" -%} {%- for tool in tools -%} {%- if tool is not string -%} {%- set tool = tool | tojson -%} {%- endif -%} {%- set ns.system_prompt = ns.system_prompt + tool -%} {%- if not loop.last -%} {%- set ns.system_prompt = ns.system_prompt + ", " -%} {%- endif -%} {%- endfor -%} {%- set ns.system_prompt = ns.system_prompt + "]<|tool_list_end|>" -%}{%- endif -%}{%- if ns.system_prompt -%} {{- "<|im_start|>system " + ns.system_prompt + "<|im_end|> " -}}{%- endif -%}{%- for message in messages -%} {{- "<|im_start|>" + message["role"] + " " -}} {%- set content = message["content"] -%} {%- if content is not string -%} {%- set content = content | tojson -%} {%- endif -%} {%- if message["role"] == "tool" -%} {%- set content = "<|tool_response_start|>" + content + "<|tool_response_end|>" -%} {%- endif -%} {{- content + "<|im_end|> " -}}{%- endfor -%}{%- if add_generation_prompt -%} {{- "<|im_start|>assistant " -}}{%- endif -%}"#.to_string(),
            "<|begin_of_text|>".to_string(),
            "<|end_of_text|>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("こんにちは".to_string(), None)?;
        let expected = "<|begin_of_text|><|im_start|>system Translate Japanese manga dialogue into natural English for speech bubbles. Keep character voice, emotional tone, and relationship nuances clear. Keep the wording concise enough to fit manga bubbles. Preserve emphasis and sound effects naturally when they appear. Do not add notes or explanations, and do not add or delete line breaks inside a block. If the input contains <block id=\"N\">...</block>, translate only the content inside each block. Preserve every block tag, id, order, and block count exactly. Do not merge blocks, split blocks, or add explanations outside the blocks.<|im_end|> <|im_start|>user こんにちは<|im_end|> <|im_start|>assistant ";
        assert_eq!(formatted, expected);

        Ok(())
    }

    #[test]
    fn qwen25_prompt_format() -> anyhow::Result<()> {
        let model_id = ModelId::SakuraGalTransl7Bv3_7;
        let renderer = PromptRenderer::new(
            model_id,
            r#"{% for message in messages %}{% if message['role'] == 'user' %}{{'<|im_start|>user ' + message['content'] + '<|im_end|> '}}{% elif message['role'] == 'assistant' %}{{'<|im_start|>assistant ' + message['content'] + '<|im_end|> ' }}{% else %}{{ '<|im_start|>system ' + message['content'] + '<|im_end|> ' }}{% endif %}{% endfor %}{% if add_generation_prompt %}{{ '<|im_start|>assistant ' }}{% endif %}"#.to_string(),
            "<s>".to_string(),
            "</s>".to_string(),
        );
        let formatted = renderer.format_chat_prompt("こんにちは".to_string(), None)?;
        let expected = "<|im_start|>system 你是一个漫画翻译模型。请把日文漫画对白翻译成自然、简洁、适合放进对话气泡的简体中文。保留人物语气、情绪、关系和说话风格，正确处理人称，不要误解使役态和被动态。拟声词、强调和语气词要自然处理；不要添加注释、解释、罗马音或原文里没有的特殊符号，也不要擅自增加或减少换行。如果输入中包含 <block id=\"N\">...</block>，只翻译每个 block 标签内部的内容。必须完整保留所有 block 标签、id、顺序和 block 数量，不要合并 block，不要拆分 block，也不要在 block 之外添加任何内容。<|im_end|> <|im_start|>user こんにちは<|im_end|> <|im_start|>assistant ";
        assert_eq!(formatted, expected);

        Ok(())
    }

    #[test]
    fn hunyuan_prompt_uses_strict_block_tag_output_instructions() -> anyhow::Result<()> {
        let model_id = ModelId::HunyuanMT7B;
        let renderer = PromptRenderer::new(
            model_id,
            "{{ messages[0]['content'] }}".to_string(),
            "<s>".to_string(),
            "</s>".to_string(),
        );
        let formatted = renderer.format_chat_prompt(
            "<block id=\"0\">\nこんにちは\n</block>".to_string(),
            Some("Korean"),
        )?;
        let expected = "Translate the following Japanese manga dialogue into Korean. Make it read naturally inside manga speech bubbles. Keep character voice, emotional tone, and relationship nuances. Keep the wording concise, and preserve emphasis and sound effects naturally when they appear. Do not add notes or explanations. The input may contain XML-like block tags. Translate only the text inside each block. Keep every tag exactly unchanged, including `<block id=\"N\">` and `</block>`, and preserve the same ids, order, and block count. Output only the translated blocks, with no code fences, labels, or extra text outside the blocks.\n\n<block id=\"0\">\nこんにちは\n</block>";
        assert_eq!(formatted, expected);

        Ok(())
    }
}
