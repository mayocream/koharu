use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextStyleKind {
    #[default]
    Regular,
    Italic,
    Bold,
    BoldItalic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSegment {
    pub text: String,
    pub kind: TextStyleKind,
}

static BOLD_ITALIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*\*\*([^*]+)\*\*\*").unwrap());
static BOLD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*]+)\*\*").unwrap());
static ITALIC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*([^*]+)\*").unwrap());

pub fn parse_styled_segments(text: &str) -> Vec<StyledSegment> {
    let mut segments = vec![StyledSegment {
        text: text.to_string(),
        kind: TextStyleKind::Regular,
    }];

    // Order matters: *** then ** then *
    segments = process_regex(segments, &BOLD_ITALIC_RE, TextStyleKind::BoldItalic);
    segments = process_regex(segments, &BOLD_RE, TextStyleKind::Bold);
    segments = process_regex(segments, &ITALIC_RE, TextStyleKind::Italic);

    segments
}

fn process_regex(
    segments: Vec<StyledSegment>,
    re: &Regex,
    new_kind: TextStyleKind,
) -> Vec<StyledSegment> {
    let mut result = Vec::new();
    for seg in segments {
        if seg.kind != TextStyleKind::Regular {
            result.push(seg);
            continue;
        }

        let mut last_end = 0;
        for caps in re.captures_iter(&seg.text) {
            let m = caps.get(0).unwrap();
            let content = caps.get(1).unwrap();

            if m.start() > last_end {
                result.push(StyledSegment {
                    text: seg.text[last_end..m.start()].to_string(),
                    kind: TextStyleKind::Regular,
                });
            }

            result.push(StyledSegment {
                text: content.as_str().to_string(),
                kind: new_kind,
            });
            last_end = m.end();
        }

        if last_end < seg.text.len() {
            result.push(StyledSegment {
                text: seg.text[last_end..].to_string(),
                kind: TextStyleKind::Regular,
            });
        }
    }
    result
}
