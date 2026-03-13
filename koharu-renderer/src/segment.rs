use std::ops::Range;

use icu::{
    properties::{CodePointMapData, props::LineBreak},
    segmenter::{LineSegmenter, LineSegmenterBorrowed, options::LineBreakOptions},
};

/// A line break candidate with its byte offset and whether it is mandatory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineBreakOpportunity {
    pub offset: usize,
    pub is_mandatory: bool,
}

/// A trimmed line segment ready for shaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSegment {
    /// Range of visible text for this segment, excluding trailing mandatory break chars.
    pub range: Range<usize>,
    /// Byte offset where the next segment begins in the original string.
    pub next_offset: usize,
    /// Whether this segment ends with a mandatory break in the original text.
    pub is_mandatory: bool,
}

/// Line breaker using ICU4X.
pub struct LineBreaker {
    segmenter: LineSegmenterBorrowed<'static>,
}

fn trim_mandatory_break_suffix(text: &str, start: usize, end: usize) -> usize {
    let mut trimmed_end = end;
    while trimmed_end > start {
        let Some(ch) = text[..trimmed_end].chars().next_back() else {
            break;
        };
        if !matches!(ch, '\n' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}') {
            break;
        }
        trimmed_end -= ch.len_utf8();
    }
    trimmed_end
}

impl LineBreaker {
    /// Creates a new LineBreaker with default options.
    ///
    /// TODO: CJK specific customization.
    pub fn new() -> Self {
        Self {
            segmenter: LineSegmenter::new_auto(LineBreakOptions::default()),
        }
    }

    /// Returns a vector of line break opportunities in the given text.
    pub fn line_break_opportunities(&self, text: &str) -> Vec<LineBreakOpportunity> {
        self.segmenter
            .segment_str(text)
            .map(|break_pos| LineBreakOpportunity {
                offset: break_pos,
                is_mandatory: text[..break_pos].chars().next_back().is_some_and(|c| {
                    matches!(
                        CodePointMapData::<LineBreak>::new().get(c),
                        LineBreak::MandatoryBreak
                            | LineBreak::CarriageReturn
                            | LineBreak::LineFeed
                            | LineBreak::NextLine
                    )
                }),
            })
            .collect()
    }

    /// Returns shaped-text segments where mandatory break characters are excluded
    /// from the segment range but preserved in `next_offset`.
    pub fn line_segments(&self, text: &str) -> Vec<LineSegment> {
        self.line_break_opportunities(text)
            .windows(2)
            .map(|window| {
                let start = window[0].offset;
                let end = window[1].offset;
                let is_mandatory = window[1].is_mandatory;
                let segment_end = if is_mandatory {
                    trim_mandatory_break_suffix(text, start, end)
                } else {
                    end
                };
                LineSegment {
                    range: start..segment_end,
                    next_offset: end,
                    is_mandatory,
                }
            })
            .collect()
    }
}

impl Default for LineBreaker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_on_whitespace() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let linebreaker = LineBreaker::new();
        let breaks = linebreaker.line_break_opportunities(text);
        let segments: Vec<&str> = breaks
            .windows(2)
            .map(|w| &text[w[0].offset..w[1].offset])
            .collect();
        let expected = vec![
            "The ", "quick ", "brown ", "fox ", "jumps ", "over ", "the ", "lazy ", "dog.",
        ];
        assert_eq!(segments, expected);
    }

    #[test]
    fn break_on_newline() {
        let text = "Hello, \nWorld!";
        let linebreaker = LineBreaker::new();
        let breaks = linebreaker.line_break_opportunities(text);
        let expected = vec![
            LineBreakOpportunity {
                offset: 0,
                is_mandatory: false,
            },
            LineBreakOpportunity {
                offset: 8,
                is_mandatory: true,
            },
            LineBreakOpportunity {
                offset: 14,
                is_mandatory: false,
            },
        ];
        assert_eq!(breaks, expected);
    }

    #[test]
    fn line_segments_trim_newline_suffixes() {
        let text = "Hello, \nWorld!";
        let linebreaker = LineBreaker::new();
        let segments = linebreaker.line_segments(text);

        assert_eq!(segments.len(), 2);
        assert_eq!(&text[segments[0].range.clone()], "Hello, ");
        assert_eq!(segments[0].next_offset, 8);
        assert!(segments[0].is_mandatory);
        assert_eq!(&text[segments[1].range.clone()], "World!");
        assert_eq!(segments[1].next_offset, text.len());
        assert!(!segments[1].is_mandatory);
    }

    #[test]
    fn japanese_break_on_characters() {
        let text = "吾輩は猫である。名前はまだない。";
        let linebreaker = LineBreaker::new();
        let breaks = linebreaker.line_break_opportunities(text);
        let segments: Vec<&str> = breaks
            .windows(2)
            .map(|w| &text[w[0].offset..w[1].offset])
            .collect();
        let expected = vec![
            "吾", "輩", "は", "猫", "で", "あ", "る。", "名", "前", "は", "ま", "だ", "な", "い。",
        ];
        assert_eq!(segments, expected);
    }

    #[test]
    fn mixed_language_breaks_01() {
        let text = "『シャイニング』（The Shining）は、スタンリー・キューブリックが製作・監督し、小説家のダイアン・ジョンソンと共同脚本を務めた、1980年公開のサイコロジカルホラー映画。";
        let linebreaker = LineBreaker::new();
        let breaks = linebreaker.line_break_opportunities(text);
        let segments: Vec<&str> = breaks
            .windows(2)
            .map(|w| &text[w[0].offset..w[1].offset])
            .collect();
        #[rustfmt::skip]
        let expected = vec![
            "『シャ", "イ", "ニ", "ン", "グ』", "（The ", "Shining）", "は、", "ス", "タ", "ン", "リー・", "キュー", "ブ", "リッ", "ク", "が", "製", "作・", "監", "督", "し、", "小", "説", "家", "の", "ダ", "イ", "ア", "ン・", "ジョ", "ン", "ソ", "ン", "と", "共", "同", "脚", "本", "を", "務", "め", "た、", "1980", "年", "公", "開", "の", "サ", "イ", "コ", "ロ", "ジ", "カ", "ル", "ホ", "ラー", "映", "画。"
        ];
        assert_eq!(segments, expected);
    }

    #[test]
    fn mixed_language_breaks_02() {
        let text = "《我是猫》是日本作家夏目漱石创作的长篇小说，也是其代表作，它确立了夏目漱石在文学史上的地位。作品淋漓尽致地反映了二十世纪初，日本中小资产阶级的思想和生活，尖锐地揭露和批判了明治“文明开化”的资本主义社会。小说采用幽默、讽刺、滑稽的手法，借助一只猫的视觉、听觉、感觉，嘲笑了明治时代知识分子空虚的精神生活，小说构思奇巧，描写夸张，结构灵活，具有鲜明的艺术特色。";
        let linebreaker = LineBreaker::new();
        let breaks = linebreaker.line_break_opportunities(text);
        let segments: Vec<&str> = breaks
            .windows(2)
            .map(|w| &text[w[0].offset..w[1].offset])
            .collect();
        #[rustfmt::skip]
        let expected = vec![
            "《我", "是", "猫》", "是", "日", "本", "作", "家", "夏", "目", "漱", "石", "创", "作", "的", "长", "篇", "小", "说，", "也", "是", "其", "代", "表", "作，", "它", "确", "立", "了", "夏", "目", "漱", "石", "在", "文", "学", "史", "上", "的", "地", "位。", "作", "品", "淋", "漓", "尽", "致", "地", "反", "映", "了", "二", "十", "世", "纪", "初，", "日", "本", "中", "小", "资", "产", "阶", "级", "的", "思", "想", "和", "生", "活，", "尖", "锐", "地", "揭", "露", "和", "批", "判", "了", "明", "治“文", "明", "开", "化”的", "资", "本", "主", "义", "社", "会。", "小", "说", "采", "用", "幽", "默、", "讽", "刺、", "滑", "稽", "的", "手", "法，", "借", "助", "一", "只", "猫", "的", "视", "觉、", "听", "觉、", "感", "觉，", "嘲", "笑", "了", "明", "治", "时", "代", "知", "识", "分", "子", "空", "虚", "的", "精", "神", "生", "活，", "小", "说", "构", "思", "奇", "巧，", "描", "写", "夸", "张，", "结", "构", "灵", "活，", "具", "有", "鲜", "明", "的", "艺", "术", "特", "色。"
        ];
        assert_eq!(segments, expected);
    }
}
