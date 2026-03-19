#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOrientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextJustification {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub struct TextEngineSpec {
    pub text: String,
    pub font_name: String,
    pub font_size: f64,
    pub color: [u8; 4],
    pub faux_bold: bool,
    pub faux_italic: bool,
    pub orientation: TextOrientation,
    pub justification: TextJustification,
    pub box_width: f64,
    pub box_height: f64,
}

#[derive(Debug, Clone)]
enum EngineValue {
    Int(i32),
    Float(f64),
    Bool(bool),
    String(String),
    Array(Vec<EngineValue>),
    Dict(Vec<(String, EngineValue)>),
}

pub fn encode_engine_data(spec: &TextEngineSpec) -> Vec<u8> {
    let text = normalize_text(&spec.text);
    let paragraph_lengths = paragraph_run_lengths(&text);
    let total_length = utf16_len(&text) as i32;

    let font_name = if spec.font_name.trim().is_empty() {
        "ArialMT"
    } else {
        spec.font_name.trim()
    };
    let font_index = if font_name == "AdobeInvisFont" { 0 } else { 1 };
    let writing_direction = match spec.orientation {
        TextOrientation::Horizontal => 0,
        TextOrientation::Vertical => 2,
    };
    let procession = match spec.orientation {
        TextOrientation::Horizontal => 0,
        TextOrientation::Vertical => 1,
    };

    let paragraph_properties = paragraph_properties(spec.justification);
    let base_style_sheet = base_style_sheet(font_index);
    let style_run_sheet = style_run_sheet(spec, font_index);
    let font_set = font_set(font_name);

    let root = EngineValue::Dict(vec![
        (
            "EngineDict".to_string(),
            EngineValue::Dict(vec![
                (
                    "Editor".to_string(),
                    EngineValue::Dict(vec![("Text".to_string(), EngineValue::String(text))]),
                ),
                (
                    "ParagraphRun".to_string(),
                    EngineValue::Dict(vec![
                        (
                            "DefaultRunData".to_string(),
                            EngineValue::Dict(vec![
                                (
                                    "ParagraphSheet".to_string(),
                                    EngineValue::Dict(vec![
                                        ("DefaultStyleSheet".to_string(), EngineValue::Int(0)),
                                        ("Properties".to_string(), EngineValue::Dict(Vec::new())),
                                    ]),
                                ),
                                (
                                    "Adjustments".to_string(),
                                    EngineValue::Dict(vec![
                                        (
                                            "Axis".to_string(),
                                            EngineValue::Array(vec![
                                                EngineValue::Float(1.0),
                                                EngineValue::Float(0.0),
                                                EngineValue::Float(1.0),
                                            ]),
                                        ),
                                        (
                                            "XY".to_string(),
                                            EngineValue::Array(vec![
                                                EngineValue::Float(0.0),
                                                EngineValue::Float(0.0),
                                            ]),
                                        ),
                                    ]),
                                ),
                            ]),
                        ),
                        (
                            "RunArray".to_string(),
                            EngineValue::Array(
                                paragraph_lengths
                                    .iter()
                                    .map(|_| {
                                        EngineValue::Dict(vec![
                                            (
                                                "ParagraphSheet".to_string(),
                                                EngineValue::Dict(vec![
                                                    (
                                                        "DefaultStyleSheet".to_string(),
                                                        EngineValue::Int(0),
                                                    ),
                                                    (
                                                        "Properties".to_string(),
                                                        EngineValue::Dict(
                                                            paragraph_properties.clone(),
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                            (
                                                "Adjustments".to_string(),
                                                EngineValue::Dict(vec![
                                                    (
                                                        "Axis".to_string(),
                                                        EngineValue::Array(vec![
                                                            EngineValue::Float(1.0),
                                                            EngineValue::Float(0.0),
                                                            EngineValue::Float(1.0),
                                                        ]),
                                                    ),
                                                    (
                                                        "XY".to_string(),
                                                        EngineValue::Array(vec![
                                                            EngineValue::Float(0.0),
                                                            EngineValue::Float(0.0),
                                                        ]),
                                                    ),
                                                ]),
                                            ),
                                        ])
                                    })
                                    .collect(),
                            ),
                        ),
                        (
                            "RunLengthArray".to_string(),
                            EngineValue::Array(
                                paragraph_lengths
                                    .iter()
                                    .copied()
                                    .map(EngineValue::Int)
                                    .collect(),
                            ),
                        ),
                        ("IsJoinable".to_string(), EngineValue::Int(1)),
                    ]),
                ),
                (
                    "StyleRun".to_string(),
                    EngineValue::Dict(vec![
                        (
                            "DefaultRunData".to_string(),
                            EngineValue::Dict(vec![(
                                "StyleSheet".to_string(),
                                EngineValue::Dict(vec![(
                                    "StyleSheetData".to_string(),
                                    EngineValue::Dict(Vec::new()),
                                )]),
                            )]),
                        ),
                        (
                            "RunArray".to_string(),
                            EngineValue::Array(vec![EngineValue::Dict(vec![(
                                "StyleSheet".to_string(),
                                EngineValue::Dict(vec![(
                                    "StyleSheetData".to_string(),
                                    EngineValue::Dict(style_run_sheet.clone()),
                                )]),
                            )])]),
                        ),
                        (
                            "RunLengthArray".to_string(),
                            EngineValue::Array(vec![EngineValue::Int(total_length)]),
                        ),
                        ("IsJoinable".to_string(), EngineValue::Int(2)),
                    ]),
                ),
                (
                    "GridInfo".to_string(),
                    EngineValue::Dict(vec![
                        ("GridIsOn".to_string(), EngineValue::Bool(false)),
                        ("ShowGrid".to_string(), EngineValue::Bool(false)),
                        ("GridSize".to_string(), EngineValue::Float(18.0)),
                        ("GridLeading".to_string(), EngineValue::Float(22.0)),
                        (
                            "GridColor".to_string(),
                            EngineValue::Dict(color_type_values([0, 0, 255, 255])),
                        ),
                        (
                            "GridLeadingFillColor".to_string(),
                            EngineValue::Dict(color_type_values([0, 0, 255, 255])),
                        ),
                        (
                            "AlignLineHeightToGridFlags".to_string(),
                            EngineValue::Bool(false),
                        ),
                    ]),
                ),
                ("AntiAlias".to_string(), EngineValue::Int(4)),
                (
                    "UseFractionalGlyphWidths".to_string(),
                    EngineValue::Bool(true),
                ),
                (
                    "Rendered".to_string(),
                    EngineValue::Dict(vec![
                        ("Version".to_string(), EngineValue::Int(1)),
                        (
                            "Shapes".to_string(),
                            EngineValue::Dict(vec![
                                (
                                    "WritingDirection".to_string(),
                                    EngineValue::Int(writing_direction),
                                ),
                                (
                                    "Children".to_string(),
                                    EngineValue::Array(vec![EngineValue::Dict(vec![
                                        ("ShapeType".to_string(), EngineValue::Int(1)),
                                        ("Procession".to_string(), EngineValue::Int(procession)),
                                        (
                                            "Lines".to_string(),
                                            EngineValue::Dict(vec![
                                                (
                                                    "WritingDirection".to_string(),
                                                    EngineValue::Int(writing_direction),
                                                ),
                                                (
                                                    "Children".to_string(),
                                                    EngineValue::Array(Vec::new()),
                                                ),
                                            ]),
                                        ),
                                        (
                                            "Cookie".to_string(),
                                            EngineValue::Dict(vec![(
                                                "Photoshop".to_string(),
                                                EngineValue::Dict(vec![
                                                    ("ShapeType".to_string(), EngineValue::Int(1)),
                                                    (
                                                        "BoxBounds".to_string(),
                                                        EngineValue::Array(vec![
                                                            EngineValue::Float(0.0),
                                                            EngineValue::Float(0.0),
                                                            EngineValue::Float(spec.box_width),
                                                            EngineValue::Float(spec.box_height),
                                                        ]),
                                                    ),
                                                    (
                                                        "Base".to_string(),
                                                        EngineValue::Dict(vec![
                                                            (
                                                                "ShapeType".to_string(),
                                                                EngineValue::Int(1),
                                                            ),
                                                            (
                                                                "TransformPoint0".to_string(),
                                                                EngineValue::Array(vec![
                                                                    EngineValue::Float(1.0),
                                                                    EngineValue::Float(0.0),
                                                                ]),
                                                            ),
                                                            (
                                                                "TransformPoint1".to_string(),
                                                                EngineValue::Array(vec![
                                                                    EngineValue::Float(0.0),
                                                                    EngineValue::Float(1.0),
                                                                ]),
                                                            ),
                                                            (
                                                                "TransformPoint2".to_string(),
                                                                EngineValue::Array(vec![
                                                                    EngineValue::Float(0.0),
                                                                    EngineValue::Float(0.0),
                                                                ]),
                                                            ),
                                                        ]),
                                                    ),
                                                ]),
                                            )]),
                                        ),
                                    ])]),
                                ),
                            ]),
                        ),
                    ]),
                ),
            ]),
        ),
        (
            "ResourceDict".to_string(),
            EngineValue::Dict(resource_dict(
                font_set.clone(),
                paragraph_properties.clone(),
                base_style_sheet.clone(),
            )),
        ),
        (
            "DocumentResources".to_string(),
            EngineValue::Dict(resource_dict(
                font_set,
                paragraph_properties,
                base_style_sheet,
            )),
        ),
    ]);

    let mut out = Vec::new();
    out.extend_from_slice(b"\n\n");
    write_value(&mut out, &root, 0, false, None);
    out
}

fn normalize_text(text: &str) -> String {
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r");
    format!("{normalized}\r")
}

fn paragraph_run_lengths(text: &str) -> Vec<i32> {
    text.split_inclusive('\r')
        .map(|run| utf16_len(run) as i32)
        .collect()
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn paragraph_properties(justification: TextJustification) -> Vec<(String, EngineValue)> {
    vec![
        (
            "Justification".to_string(),
            EngineValue::Int(match justification {
                TextJustification::Left => 0,
                TextJustification::Right => 1,
                TextJustification::Center => 2,
            }),
        ),
        ("FirstLineIndent".to_string(), EngineValue::Float(0.0)),
        ("StartIndent".to_string(), EngineValue::Float(0.0)),
        ("EndIndent".to_string(), EngineValue::Float(0.0)),
        ("SpaceBefore".to_string(), EngineValue::Float(0.0)),
        ("SpaceAfter".to_string(), EngineValue::Float(0.0)),
        ("AutoHyphenate".to_string(), EngineValue::Bool(true)),
        ("HyphenatedWordSize".to_string(), EngineValue::Int(6)),
        ("PreHyphen".to_string(), EngineValue::Int(2)),
        ("PostHyphen".to_string(), EngineValue::Int(2)),
        ("ConsecutiveHyphens".to_string(), EngineValue::Int(8)),
        ("Zone".to_string(), EngineValue::Float(36.0)),
        (
            "WordSpacing".to_string(),
            EngineValue::Array(vec![
                EngineValue::Float(0.8),
                EngineValue::Float(1.0),
                EngineValue::Float(1.33),
            ]),
        ),
        (
            "LetterSpacing".to_string(),
            EngineValue::Array(vec![
                EngineValue::Float(0.0),
                EngineValue::Float(0.0),
                EngineValue::Float(0.0),
            ]),
        ),
        (
            "GlyphSpacing".to_string(),
            EngineValue::Array(vec![
                EngineValue::Float(1.0),
                EngineValue::Float(1.0),
                EngineValue::Float(1.0),
            ]),
        ),
        ("AutoLeading".to_string(), EngineValue::Float(1.2)),
        ("LeadingType".to_string(), EngineValue::Int(0)),
        ("Hanging".to_string(), EngineValue::Bool(false)),
        ("Burasagari".to_string(), EngineValue::Bool(false)),
        ("KinsokuOrder".to_string(), EngineValue::Int(0)),
        ("EveryLineComposer".to_string(), EngineValue::Bool(false)),
    ]
}

fn base_style_sheet(font_index: i32) -> Vec<(String, EngineValue)> {
    vec![
        ("Font".to_string(), EngineValue::Int(font_index)),
        ("FontSize".to_string(), EngineValue::Float(12.0)),
        ("FauxBold".to_string(), EngineValue::Bool(false)),
        ("FauxItalic".to_string(), EngineValue::Bool(false)),
        ("AutoLeading".to_string(), EngineValue::Bool(true)),
        ("Leading".to_string(), EngineValue::Float(0.0)),
        ("HorizontalScale".to_string(), EngineValue::Float(1.0)),
        ("VerticalScale".to_string(), EngineValue::Float(1.0)),
        ("Tracking".to_string(), EngineValue::Int(0)),
        ("AutoKerning".to_string(), EngineValue::Bool(true)),
        ("Kerning".to_string(), EngineValue::Int(0)),
        ("BaselineShift".to_string(), EngineValue::Float(0.0)),
        ("FontCaps".to_string(), EngineValue::Int(0)),
        ("FontBaseline".to_string(), EngineValue::Int(0)),
        ("Underline".to_string(), EngineValue::Bool(false)),
        ("Strikethrough".to_string(), EngineValue::Bool(false)),
        ("Ligatures".to_string(), EngineValue::Bool(true)),
        ("DLigatures".to_string(), EngineValue::Bool(false)),
        ("BaselineDirection".to_string(), EngineValue::Int(2)),
        ("Tsume".to_string(), EngineValue::Float(0.0)),
        ("StyleRunAlignment".to_string(), EngineValue::Int(2)),
        ("Language".to_string(), EngineValue::Int(0)),
        ("NoBreak".to_string(), EngineValue::Bool(false)),
        (
            "FillColor".to_string(),
            EngineValue::Dict(color_type_values([0, 0, 0, 255])),
        ),
        (
            "StrokeColor".to_string(),
            EngineValue::Dict(color_type_values([0, 0, 0, 255])),
        ),
        ("FillFlag".to_string(), EngineValue::Bool(true)),
        ("StrokeFlag".to_string(), EngineValue::Bool(false)),
        ("FillFirst".to_string(), EngineValue::Bool(true)),
        ("YUnderline".to_string(), EngineValue::Int(1)),
        ("OutlineWidth".to_string(), EngineValue::Float(1.0)),
        ("CharacterDirection".to_string(), EngineValue::Int(0)),
        ("HindiNumbers".to_string(), EngineValue::Bool(false)),
        ("Kashida".to_string(), EngineValue::Int(1)),
        ("DiacriticPos".to_string(), EngineValue::Int(2)),
    ]
}

fn style_run_sheet(spec: &TextEngineSpec, font_index: i32) -> Vec<(String, EngineValue)> {
    vec![
        ("Font".to_string(), EngineValue::Int(font_index)),
        ("FontSize".to_string(), EngineValue::Float(spec.font_size)),
        ("FauxBold".to_string(), EngineValue::Bool(spec.faux_bold)),
        (
            "FauxItalic".to_string(),
            EngineValue::Bool(spec.faux_italic),
        ),
        ("AutoKerning".to_string(), EngineValue::Bool(true)),
        ("Kerning".to_string(), EngineValue::Int(0)),
        (
            "FillColor".to_string(),
            EngineValue::Dict(color_type_values(spec.color)),
        ),
    ]
}

fn resource_dict(
    font_set: Vec<EngineValue>,
    paragraph_properties: Vec<(String, EngineValue)>,
    style_sheet: Vec<(String, EngineValue)>,
) -> Vec<(String, EngineValue)> {
    vec![
        (
            "KinsokuSet".to_string(),
            EngineValue::Array(vec![
                EngineValue::Dict(vec![
                    (
                        "Name".to_string(),
                        EngineValue::String("PhotoshopKinsokuHard".to_string()),
                    ),
                    (
                        "NoStart".to_string(),
                        EngineValue::String(
                            "\u{3001}\u{3002}\u{ff0c}\u{ff0e}\u{30fb}\u{ff1a}\u{ff1b}\u{ff1f}\u{ff01}\u{30fc}\u{2015}\u{2019}\u{201d}\u{ff09}\u{3015}\u{ff3d}\u{ff5d}\u{3009}\u{300b}\u{300d}\u{300f}\u{3011}\u{30fd}\u{30fe}\u{309d}\u{309e}\u{3005}\u{3041}\u{3043}\u{3045}\u{3047}\u{3049}\u{3063}\u{3083}\u{3085}\u{3087}\u{308e}\u{30a1}\u{30a3}\u{30a5}\u{30a7}\u{30a9}\u{30c3}\u{30e3}\u{30e5}\u{30e7}\u{30ee}\u{30f5}\u{30f6}\u{309b}\u{309c}?!)]},.:;\u{2103}\u{2109}\u{00a2}\u{ff05}\u{2030}".to_string(),
                        ),
                    ),
                    (
                        "NoEnd".to_string(),
                        EngineValue::String(
                            "\u{2018}\u{201c}\u{ff08}\u{3014}\u{ff3b}\u{ff5b}\u{3008}\u{300a}\u{300c}\u{300e}\u{3010}([{\u{ffe5}\u{ff04}\u{00a3}\u{ff20}\u{00a7}\u{3012}\u{ff03}".to_string(),
                        ),
                    ),
                    (
                        "Keep".to_string(),
                        EngineValue::String("\u{2015}\u{2025}".to_string()),
                    ),
                    (
                        "Hanging".to_string(),
                        EngineValue::String("\u{3001}\u{3002}.,".to_string()),
                    ),
                ]),
                EngineValue::Dict(vec![
                    (
                        "Name".to_string(),
                        EngineValue::String("PhotoshopKinsokuSoft".to_string()),
                    ),
                    (
                        "NoStart".to_string(),
                        EngineValue::String(
                            "\u{3001}\u{3002}\u{ff0c}\u{ff0e}\u{30fb}\u{ff1a}\u{ff1b}\u{ff1f}\u{ff01}\u{2019}\u{201d}\u{ff09}\u{3015}\u{ff3d}\u{ff5d}\u{3009}\u{300b}\u{300d}\u{300f}\u{3011}\u{30fd}\u{30fe}\u{309d}\u{309e}\u{3005}".to_string(),
                        ),
                    ),
                    (
                        "NoEnd".to_string(),
                        EngineValue::String(
                            "\u{2018}\u{201c}\u{ff08}\u{3014}\u{ff3b}\u{ff5b}\u{3008}\u{300a}\u{300c}\u{300e}\u{3010}".to_string(),
                        ),
                    ),
                    (
                        "Keep".to_string(),
                        EngineValue::String("\u{2015}\u{2025}".to_string()),
                    ),
                    (
                        "Hanging".to_string(),
                        EngineValue::String("\u{3001}\u{3002}.,".to_string()),
                    ),
                ]),
            ]),
        ),
        (
            "MojiKumiSet".to_string(),
            EngineValue::Array(vec![
                EngineValue::Dict(vec![(
                    "InternalName".to_string(),
                    EngineValue::String("Photoshop6MojiKumiSet1".to_string()),
                )]),
                EngineValue::Dict(vec![(
                    "InternalName".to_string(),
                    EngineValue::String("Photoshop6MojiKumiSet2".to_string()),
                )]),
                EngineValue::Dict(vec![(
                    "InternalName".to_string(),
                    EngineValue::String("Photoshop6MojiKumiSet3".to_string()),
                )]),
                EngineValue::Dict(vec![(
                    "InternalName".to_string(),
                    EngineValue::String("Photoshop6MojiKumiSet4".to_string()),
                )]),
            ]),
        ),
        ("TheNormalStyleSheet".to_string(), EngineValue::Int(0)),
        ("TheNormalParagraphSheet".to_string(), EngineValue::Int(0)),
        (
            "ParagraphSheetSet".to_string(),
            EngineValue::Array(vec![EngineValue::Dict(vec![
                (
                    "Name".to_string(),
                    EngineValue::String("Normal RGB".to_string()),
                ),
                ("DefaultStyleSheet".to_string(), EngineValue::Int(0)),
                (
                    "Properties".to_string(),
                    EngineValue::Dict(paragraph_properties),
                ),
            ])]),
        ),
        (
            "StyleSheetSet".to_string(),
            EngineValue::Array(vec![EngineValue::Dict(vec![
                (
                    "Name".to_string(),
                    EngineValue::String("Normal RGB".to_string()),
                ),
                ("StyleSheetData".to_string(), EngineValue::Dict(style_sheet)),
            ])]),
        ),
        ("FontSet".to_string(), EngineValue::Array(font_set)),
        ("SuperscriptSize".to_string(), EngineValue::Float(0.583)),
        ("SuperscriptPosition".to_string(), EngineValue::Float(0.333)),
        ("SubscriptSize".to_string(), EngineValue::Float(0.583)),
        ("SubscriptPosition".to_string(), EngineValue::Float(0.333)),
        ("SmallCapSize".to_string(), EngineValue::Float(0.7)),
    ]
}

fn font_set(font_name: &str) -> Vec<EngineValue> {
    let mut fonts = vec![font_descriptor("AdobeInvisFont")];
    if font_name != "AdobeInvisFont" {
        fonts.push(font_descriptor(font_name));
    }
    fonts
}

fn font_descriptor(name: &str) -> EngineValue {
    EngineValue::Dict(vec![
        ("Name".to_string(), EngineValue::String(name.to_string())),
        ("Script".to_string(), EngineValue::Int(0)),
        ("FontType".to_string(), EngineValue::Int(0)),
        ("Synthetic".to_string(), EngineValue::Int(0)),
    ])
}

fn color_type_values(color: [u8; 4]) -> Vec<(String, EngineValue)> {
    vec![
        ("Type".to_string(), EngineValue::Int(1)),
        (
            "Values".to_string(),
            EngineValue::Array(vec![
                EngineValue::Float(color[3] as f64 / 255.0),
                EngineValue::Float(color[0] as f64 / 255.0),
                EngineValue::Float(color[1] as f64 / 255.0),
                EngineValue::Float(color[2] as f64 / 255.0),
            ]),
        ),
    ]
}

fn write_value(
    out: &mut Vec<u8>,
    value: &EngineValue,
    indent: usize,
    in_property: bool,
    key: Option<&str>,
) {
    match value {
        EngineValue::Int(number) => {
            write_prefix(out, indent, in_property);
            out.extend_from_slice(number.to_string().as_bytes());
        }
        EngineValue::Float(number) => {
            write_prefix(out, indent, in_property);
            out.extend_from_slice(serialize_float(*number, key).as_bytes());
        }
        EngineValue::Bool(flag) => {
            write_prefix(out, indent, in_property);
            out.extend_from_slice(if *flag { b"true" } else { b"false" });
        }
        EngineValue::String(text) => {
            write_prefix(out, indent, in_property);
            out.push(b'(');
            out.push(0xFE);
            out.push(0xFF);
            for unit in text.encode_utf16() {
                write_escaped_byte(out, (unit >> 8) as u8);
                write_escaped_byte(out, unit as u8);
            }
            out.push(b')');
        }
        EngineValue::Array(items) => {
            write_prefix(out, indent, in_property);
            if items.iter().all(is_scalar) {
                out.extend_from_slice(b"[");
                for item in items {
                    out.push(b' ');
                    write_inline_value(out, item, key);
                }
                out.extend_from_slice(b" ]");
            } else {
                out.extend_from_slice(b"[\n");
                for item in items {
                    write_value(out, item, indent + 1, false, key);
                    out.push(b'\n');
                }
                write_indent(out, indent);
                out.extend_from_slice(b"]");
            }
        }
        EngineValue::Dict(entries) => {
            if in_property {
                out.push(b'\n');
            } else {
                write_indent(out, indent);
            }
            out.extend_from_slice(b"<<\n");
            for (entry_key, entry_value) in entries {
                write_indent(out, indent + 1);
                out.push(b'/');
                out.extend_from_slice(entry_key.as_bytes());
                write_value(out, entry_value, indent + 1, true, Some(entry_key));
                out.push(b'\n');
            }
            write_indent(out, indent);
            out.extend_from_slice(b">>");
        }
    }
}

fn write_inline_value(out: &mut Vec<u8>, value: &EngineValue, key: Option<&str>) {
    match value {
        EngineValue::Int(number) => out.extend_from_slice(number.to_string().as_bytes()),
        EngineValue::Float(number) => {
            out.extend_from_slice(serialize_float(*number, key).as_bytes())
        }
        EngineValue::Bool(flag) => out.extend_from_slice(if *flag { b"true" } else { b"false" }),
        _ => write_value(out, value, 0, false, key),
    }
}

fn write_prefix(out: &mut Vec<u8>, indent: usize, in_property: bool) {
    if in_property {
        out.push(b' ');
    } else {
        write_indent(out, indent);
    }
}

fn write_indent(out: &mut Vec<u8>, indent: usize) {
    for _ in 0..indent {
        out.push(b'\t');
    }
}

fn write_escaped_byte(out: &mut Vec<u8>, byte: u8) {
    if matches!(byte, b'(' | b')' | b'\\') {
        out.push(b'\\');
    }
    out.push(byte);
}

fn is_scalar(value: &EngineValue) -> bool {
    matches!(
        value,
        EngineValue::Int(_) | EngineValue::Float(_) | EngineValue::Bool(_) | EngineValue::String(_)
    )
}

fn serialize_float(value: f64, key: Option<&str>) -> String {
    let is_float = matches!(
        key,
        Some(
            "Axis"
                | "XY"
                | "Zone"
                | "WordSpacing"
                | "FirstLineIndent"
                | "GlyphSpacing"
                | "StartIndent"
                | "EndIndent"
                | "SpaceBefore"
                | "SpaceAfter"
                | "LetterSpacing"
                | "Values"
                | "GridSize"
                | "GridLeading"
                | "PointBase"
                | "BoxBounds"
                | "TransformPoint0"
                | "TransformPoint1"
                | "TransformPoint2"
                | "FontSize"
                | "Leading"
                | "HorizontalScale"
                | "VerticalScale"
                | "BaselineShift"
                | "Tsume"
                | "OutlineWidth"
                | "AutoLeading"
        )
    ) || value.fract() != 0.0;

    if !is_float {
        return (value as i32).to_string();
    }

    let mut formatted = format!("{value:.5}");
    if let Some(dot) = formatted.find('.') {
        while formatted.ends_with('0') && formatted.len() > dot + 2 {
            formatted.pop();
        }
    }
    if formatted.starts_with("0.")
        && formatted
            .as_bytes()
            .get(2)
            .is_some_and(|digit| *digit != b'0')
    {
        formatted.remove(0);
    } else if formatted.starts_with("-0.0")
        && formatted
            .as_bytes()
            .get(4)
            .is_some_and(|digit| digit.is_ascii_digit() && *digit != b'0')
    {
        formatted.remove(1);
    }

    formatted
}

#[cfg(test)]
mod tests {
    use super::{TextEngineSpec, TextJustification, TextOrientation, encode_engine_data};

    #[test]
    fn engine_data_contains_expected_sections_and_utf16_text() {
        let bytes = encode_engine_data(&TextEngineSpec {
            text: "Hello".to_string(),
            font_name: "ArialMT".to_string(),
            font_size: 14.0,
            color: [1, 2, 3, 255],
            faux_bold: true,
            faux_italic: false,
            orientation: TextOrientation::Horizontal,
            justification: TextJustification::Center,
            box_width: 100.0,
            box_height: 32.0,
        });

        assert!(
            bytes
                .windows("/EngineDict".len())
                .any(|window| window == b"/EngineDict")
        );
        assert!(
            bytes
                .windows("/FontSet".len())
                .any(|window| window == b"/FontSet")
        );
        assert!(
            bytes
                .windows("/RunLengthArray".len())
                .any(|window| window == b"/RunLengthArray")
        );
        assert!(bytes.windows(2).any(|window| window == [0xFE, 0xFF]));
    }

    #[test]
    fn engine_data_keeps_float_tokens_for_font_size_and_transforms() {
        let bytes = encode_engine_data(&TextEngineSpec {
            text: "Hello".to_string(),
            font_name: "ArialMT".to_string(),
            font_size: 14.0,
            color: [1, 2, 3, 255],
            faux_bold: true,
            faux_italic: false,
            orientation: TextOrientation::Horizontal,
            justification: TextJustification::Center,
            box_width: 100.0,
            box_height: 32.0,
        });

        assert!(
            bytes
                .windows("/FontSize 14.0".len())
                .any(|w| w == b"/FontSize 14.0")
        );
        assert!(
            bytes
                .windows("/Axis [ 1.0 0.0 1.0 ]".len())
                .any(|w| w == b"/Axis [ 1.0 0.0 1.0 ]")
        );
    }
}
