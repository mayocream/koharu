use anyhow::{Context, anyhow, bail};
use serde::de::DeserializeOwned;
use serde_json::{Map, Number, Value};

const MAX_DEPTH: usize = 128;
const MAX_ROOT_CANDIDATES: usize = 64;

pub(crate) fn from_str<T: DeserializeOwned>(input: &str) -> anyhow::Result<T> {
    let strict_error = match serde_json::from_str(input) {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };

    let mut last_error = None;
    for (position, _) in input
        .char_indices()
        .filter(|(_, character)| matches!(character, '{' | '['))
        .take(MAX_ROOT_CANDIDATES)
    {
        let result = Parser::new(&input[position..])
            .parse()
            .and_then(|value| serde_json::from_value(value).map_err(Into::into));
        match result {
            Ok(value) => return Ok(value),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| strict_error.into())).context("failed to parse or repair JSON")
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn parse(mut self) -> anyhow::Result<Value> {
        self.skip_ignored();
        self.parse_value(0)
    }

    fn parse_value(&mut self, depth: usize) -> anyhow::Result<Value> {
        if depth >= MAX_DEPTH {
            bail!("JSON nesting exceeds {MAX_DEPTH} levels");
        }

        self.skip_ignored();
        match self.peek() {
            Some('{') => self.parse_object(depth + 1),
            Some('[') => self.parse_array(depth + 1),
            Some(character) if quote_end(character).is_some() => {
                self.parse_string().map(Value::String)
            }
            Some(character) if character.is_ascii_digit() || matches!(character, '-' | '+') => {
                Ok(self.parse_number_or_string())
            }
            Some(_) => self.parse_unquoted_value(),
            None => bail!("expected a JSON value at byte {}", self.position),
        }
    }

    fn parse_object(&mut self, depth: usize) -> anyhow::Result<Value> {
        self.expect('{')?;
        let mut object = Map::new();

        loop {
            self.skip_ignored();
            while self.consume(',') {
                self.skip_ignored();
            }
            if self.consume('}') || self.peek().is_none() {
                return Ok(Value::Object(object));
            }

            let key = self.parse_key()?;
            self.skip_ignored();
            self.consume(':');
            self.skip_ignored();
            if self.peek().is_none() {
                object.insert(key, Value::Null);
                return Ok(Value::Object(object));
            }

            object.insert(key, self.parse_value(depth)?);
            self.skip_ignored();
            self.consume(',');
        }
    }

    fn parse_array(&mut self, depth: usize) -> anyhow::Result<Value> {
        self.expect('[')?;
        let mut array = Vec::new();

        loop {
            self.skip_ignored();
            while self.consume(',') {
                self.skip_ignored();
            }
            if self.consume(']') || self.peek().is_none() {
                return Ok(Value::Array(array));
            }

            array.push(self.parse_value(depth)?);
            self.skip_ignored();
            self.consume(',');
        }
    }

    fn parse_key(&mut self) -> anyhow::Result<String> {
        if self.peek().and_then(quote_end).is_some() {
            return self.parse_string();
        }

        let start = self.position;
        while let Some(character) = self.peek() {
            if character.is_whitespace() || matches!(character, ':' | ',' | '}' | '{' | '[' | ']') {
                break;
            }
            self.advance();
        }
        let key = self.input[start..self.position].trim();
        if key.is_empty() {
            bail!("expected an object key at byte {start}");
        }
        Ok(key.to_owned())
    }

    fn parse_string(&mut self) -> anyhow::Result<String> {
        let opening = self
            .advance()
            .ok_or_else(|| anyhow!("expected a string at byte {}", self.position))?;
        let closing = quote_end(opening)
            .ok_or_else(|| anyhow!("expected a quote at byte {}", self.position))?;
        let mut output = String::new();

        while let Some(character) = self.advance() {
            if character == closing {
                if closing == '\''
                    && output.chars().last().is_some_and(char::is_alphanumeric)
                    && self.peek().is_some_and(char::is_alphanumeric)
                {
                    output.push(character);
                    continue;
                }
                return Ok(output);
            }
            if character == '\\' {
                self.parse_escape(&mut output);
            } else {
                output.push(character);
            }
        }

        Ok(output)
    }

    fn parse_escape(&mut self, output: &mut String) {
        let Some(escaped) = self.advance() else {
            return;
        };
        match escaped {
            '"' => output.push('"'),
            '\'' => output.push('\''),
            '\\' => output.push('\\'),
            '/' => output.push('/'),
            'b' => output.push('\u{0008}'),
            'f' => output.push('\u{000c}'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            'u' => output.push(self.parse_unicode_escape()),
            other => output.push(other),
        }
    }

    fn parse_unicode_escape(&mut self) -> char {
        let Some(high) = self.take_hex_quad() else {
            return char::REPLACEMENT_CHARACTER;
        };
        if !(0xd800..=0xdbff).contains(&high) {
            return char::from_u32(u32::from(high)).unwrap_or(char::REPLACEMENT_CHARACTER);
        }

        let saved = self.position;
        if self.consume('\\')
            && self.consume('u')
            && let Some(low @ 0xdc00..=0xdfff) = self.take_hex_quad()
        {
            let codepoint =
                0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(low) - 0xdc00);
            return char::from_u32(codepoint).unwrap_or(char::REPLACEMENT_CHARACTER);
        }
        self.position = saved;
        char::REPLACEMENT_CHARACTER
    }

    fn take_hex_quad(&mut self) -> Option<u16> {
        let bytes = self.remaining().as_bytes().get(..4)?;
        if !bytes.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        self.position += 4;
        u16::from_str_radix(std::str::from_utf8(bytes).ok()?, 16).ok()
    }

    fn parse_number_or_string(&mut self) -> Value {
        let start = self.position;
        while self.peek().is_some_and(|character| {
            character.is_ascii_digit() || matches!(character, '+' | '-' | '.' | 'e' | 'E')
        }) {
            self.advance();
        }
        let token = &self.input[start..self.position];
        if let Ok(number) = token.parse::<Number>() {
            return Value::Number(number);
        }
        if let Ok(number) = token.parse::<f64>()
            && let Some(number) = Number::from_f64(number)
        {
            return Value::Number(number);
        }
        Value::String(token.to_owned())
    }

    fn parse_unquoted_value(&mut self) -> anyhow::Result<Value> {
        let start = self.position;
        while let Some(character) = self.peek() {
            if matches!(character, ',' | ']' | '}') || self.starts_comment() {
                break;
            }
            self.advance();
        }
        let token = self.input[start..self.position].trim();
        if token.is_empty() {
            bail!("expected a JSON value at byte {start}");
        }
        Ok(match token.to_ascii_lowercase().as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" | "none" | "undefined" => Value::Null,
            _ => Value::String(token.to_owned()),
        })
    }

    fn skip_ignored(&mut self) {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.advance();
            }
            if self.remaining().starts_with("//") || self.remaining().starts_with('#') {
                while self.peek().is_some_and(|character| character != '\n') {
                    self.advance();
                }
                continue;
            }
            if self.remaining().starts_with("/*") {
                self.position += 2;
                if let Some(end) = self.remaining().find("*/") {
                    self.position += end + 2;
                } else {
                    self.position = self.input.len();
                }
                continue;
            }
            break;
        }
    }

    fn starts_comment(&self) -> bool {
        self.remaining().starts_with("//")
            || self.remaining().starts_with("/*")
            || self.remaining().starts_with('#')
    }

    fn expect(&mut self, expected: char) -> anyhow::Result<()> {
        if self.consume(expected) {
            Ok(())
        } else {
            bail!("expected `{expected}` at byte {}", self.position)
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.position..]
    }
}

fn quote_end(character: char) -> Option<char> {
    match character {
        '"' | '\'' | '`' => Some(character),
        '“' => Some('”'),
        '‘' => Some('’'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Output {
        translations: Vec<String>,
    }

    fn expected() -> Output {
        Output {
            translations: vec!["hello".to_owned(), "world".to_owned()],
        }
    }

    #[test]
    fn parses_strict_and_fenced_json() {
        for input in [
            r#"{"translations":["hello","world"]}"#,
            "```json\n{\"translations\":[\"hello\",\"world\"]}\n```",
            "```JSON\n{\"translations\":[\"hello\",\"world\"]}\n```",
            "```\n{\"translations\":[\"hello\",\"world\"]}\n```",
        ] {
            assert_eq!(from_str::<Output>(input).unwrap(), expected());
        }
    }

    #[test]
    fn repairs_relaxed_syntax() {
        for input in [
            r#"{translations: ['hello', 'world'],}"#,
            r#"{“translations”: [“hello”, “world”]}"#,
            "{translations: [\"hello\" \"world\"]}",
            "{translations: [\"hello\", /* later */ \"world\",]}",
        ] {
            assert_eq!(from_str::<Output>(input).unwrap(), expected());
        }
    }

    #[test]
    fn extracts_json_from_prose() {
        let input = r#"Certainly. Here is the result: {"translations":["hello","world"]} Enjoy!"#;
        assert_eq!(from_str::<Output>(input).unwrap(), expected());
    }

    #[test]
    fn closes_truncated_strings_and_containers() {
        for input in [
            "{\"translations\":[\"hello\",\"world\"",
            "{\"translations\":[\"hello\",\"world",
        ] {
            assert_eq!(from_str::<Output>(input).unwrap(), expected());
        }
    }

    #[test]
    fn preserves_escapes_and_unicode() {
        let output =
            from_str::<Output>(r#"{translations: ["line\nfeed", "\u65e5\u672c \ud83d\ude00"]}"#)
                .unwrap();
        assert_eq!(output.translations, ["line\nfeed", "日本 😀"]);
    }

    #[test]
    fn rejects_the_wrong_shape() {
        assert!(from_str::<Output>(r#"{"result":["hello"]}"#).is_err());
    }
}
