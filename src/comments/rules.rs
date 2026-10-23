use super::{config::CommentConfig, parser::CommentBlock};

#[derive(Debug, Clone)]
pub struct Head {
    pub word: String,
    pub word_end: usize,
    pub zone_start: usize,
    pub zone_end: usize,
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// NOTE: decorations are the wrappers people put around a keyword
/// (`**NOTE**`, `[NOTE]`, `- NOTE`, `1. NOTE`); they are recognized and
/// dropped, so the keyword itself is what decides the block.
fn decoration_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();

    if text.starts_with("**") {
        return Some(2);
    }

    match bytes.first() {
        Some(b'*') => Some(1),
        Some(b'[') => Some(1),
        Some(b'-') => {
            if bytes.get(1).is_some_and(|byte| is_word_byte(*byte)) {
                None
            } else {
                Some(1)
            }
        }
        _ => {
            let digits = bytes
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digits > 0 && bytes.get(digits) == Some(&b'.') {
                Some(digits + 1)
            } else {
                None
            }
        }
    }
}

fn parse_head(line: &str, is_block: bool) -> Option<Head> {
    let bytes = line.as_bytes();
    let mut index = 0;

    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    // NOTE: the leading star of a block comment belongs to the block style,
    // not to the keyword, so it is kept as part of the prefix.
    if is_block && bytes.get(index) == Some(&b'*') {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
    }

    let mut zone_start = None;
    let mut closing = "";

    while let Some(len) = decoration_len(&line[index..]) {
        zone_start.get_or_insert(index);
        closing = match &line[index..index + len] {
            "**" => "**",
            "*" => "*",
            "[" => "]",
            _ => "",
        };
        index += len;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
    }

    if index >= bytes.len() {
        return None;
    }

    let word_start = index;
    let mut word_end = index;
    while word_end < bytes.len() && is_word_byte(bytes[word_end]) {
        word_end += 1;
    }

    let mut zone_end = word_end;
    if !closing.is_empty() && line[word_end..].starts_with(closing) {
        zone_end += closing.len();
    }

    Some(Head {
        word: line[word_start..word_end].to_string(),
        word_end,
        zone_start: zone_start.unwrap_or(word_start),
        zone_end,
    })
}

fn mark_truncated(line: &str, is_block: bool) -> String {
    let label_end = match parse_head(line, is_block) {
        Some(head) if !head.word.is_empty() => {
            let mut end = head.word_end;
            if line[end..].starts_with(':') {
                end += 1;
            }
            end
        }
        _ => line.find(':').map(|index| index + 1).unwrap_or(0),
    };

    format!("{} [cut]{}", &line[..label_end], &line[label_end..])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    NoKeyword,
    Normalized,
    Truncated { lines: usize, max: usize },
    NonAscii,
}

#[derive(Debug, Clone)]
pub struct Keyword {
    pub raw: String,
    pub canonical: String,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub block: CommentBlock,
    pub actions: Vec<Action>,
    pub keyword: Option<Keyword>,
}

pub fn apply(block: &CommentBlock, config: &CommentConfig) -> Outcome {
    let mut contents = block.contents.clone();

    while contents.len() > 1 && contents.last().is_some_and(|c| c.trim().is_empty()) {
        contents.pop();
    }

    let mut actions = Vec::new();
    let is_block = block.close.is_some();

    let head = contents
        .iter()
        .position(|line| parse_head(line, is_block).is_some())
        .unwrap_or(0);

    let head_info = parse_head(&contents[head], is_block);

    let keyword = match &head_info {
        Some(info) => match config.canonical_keyword(&info.word) {
            Some(canonical) => {
                let line = contents[head].clone();
                let zone = line[info.zone_start..info.zone_end].to_string();

                if zone != canonical {
                    let mut rebuilt = String::with_capacity(line.len());
                    rebuilt.push_str(&line[..info.zone_start]);
                    rebuilt.push_str(&canonical);
                    rebuilt.push_str(&line[info.zone_end..]);
                    contents[head] = rebuilt;
                    actions.push(Action::Normalized);
                }

                Some(Keyword {
                    raw: zone,
                    canonical,
                })
            }
            None => {
                actions.push(Action::NoKeyword);
                None
            }
        },
        None => {
            actions.push(Action::NoKeyword);
            None
        }
    };

    if contents.iter().any(|line| !line.is_ascii()) {
        actions.push(Action::NonAscii);
    }

    let max = config.max_lines.max(1);

    if contents.len() - head > max {
        let lines = contents.len() - head;
        let marked = mark_truncated(&contents[head], is_block);
        let mut truncated = contents[..head].to_vec();
        truncated.push(marked);
        truncated.extend(contents[head + 1..head + max].iter().cloned());
        contents = truncated;
        actions.push(Action::Truncated { lines, max });
    }

    Outcome {
        block: CommentBlock {
            contents,
            ..block.clone()
        },
        actions,
        keyword,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::parser::{self, Syntax};

    fn blocks(source: &str) -> Vec<CommentBlock> {
        let syntax = Syntax {
            line: &["//"],
            block: true,
            html: false,
            jsdoc: true,
        };
        parser::parse(source, &syntax)
    }

    fn render(source: &str) -> String {
        let original: Vec<&str> = source.lines().collect();
        let mut out: Vec<String> = Vec::new();
        let mut cursor = 0;

        for block in blocks(source) {
            out.extend(original[cursor..block.start].iter().map(|l| l.to_string()));
            out.extend(block.render());
            cursor = block.end + 1;
        }
        out.extend(original[cursor..].iter().map(|l| l.to_string()));

        let mut text = out.join("\n");
        if source.ends_with('\n') {
            text.push('\n');
        }
        text
    }

    fn outcome(source: &str) -> Outcome {
        apply(&blocks(source).remove(0), &CommentConfig::default())
    }

    #[test]
    fn removes_comment_without_keyword() {
        let outcome = outcome("// just a comment\nlet x = 1;\n");
        assert!(outcome.actions.contains(&Action::NoKeyword));
    }

    #[test]
    fn keeps_note() {
        let outcome = outcome("// NOTE: keep\n");
        assert_eq!(outcome.actions, Vec::new());
        assert_eq!(outcome.block.render(), vec!["// NOTE: keep"]);
    }

    #[test]
    fn normalizes_variants() {
        let cases = [
            ("// to-do: x\n", "// TODO: x\n"),
            ("// TO-DO: x\n", "// TODO: x\n"),
            ("// fix-me: x\n", "// FIXME: x\n"),
            ("// FixMe: x\n", "// FIXME: x\n"),
        ];
        for (source, expected) in cases {
            let outcome = outcome(source);
            assert!(outcome.actions.contains(&Action::Normalized), "{source}");
            assert_eq!(outcome.block.render().remove(0), expected.trim_end());
        }
    }

    #[test]
    fn truncates_to_three_lines_with_note() {
        let source = "// NOTE: one\n// two\n// three\n// four\n// five\n";
        let outcome = outcome(source);
        assert_eq!(
            outcome.actions,
            vec![Action::Truncated { lines: 5, max: 3 }]
        );
        assert_eq!(
            outcome.block.render(),
            vec![
                "// NOTE: [cut] one".to_string(),
                "// two".to_string(),
                "// three".to_string(),
            ]
        );
    }

    #[test]
    fn normalizes_keyword_inside_block_comment() {
        let source = "/** to-do: fix it */\nlet x = 1;\n";
        let outcome = outcome(source);
        assert!(outcome.actions.contains(&Action::Normalized));
        assert_eq!(outcome.block.render(), vec!["/** TODO: fix it */"]);
    }

    #[test]
    fn truncates_block_comment_without_losing_star() {
        let source = "/**\n * NOTE: one\n * two\n * three\n * four\n */\nlet x = 1;\n";
        let outcome = outcome(source);
        assert!(
            outcome
                .actions
                .contains(&Action::Truncated { lines: 4, max: 3 })
        );
        assert_eq!(
            outcome.block.render(),
            vec![
                "/**".to_string(),
                " * NOTE: [cut] one".to_string(),
                " * two".to_string(),
                " * three */".to_string(),
            ]
        );
    }

    #[test]
    fn detects_non_ascii() {
        let outcome = outcome("// NOTE: привет\n");
        assert!(outcome.actions.contains(&Action::NonAscii));
        assert_eq!(outcome.actions.len(), 1);
    }

    #[test]
    fn reports_length_and_keyword_together() {
        let source = "// plain text\n// second\n// third\n// fourth\n";
        let outcome = outcome(source);
        assert!(outcome.actions.contains(&Action::NoKeyword));
        assert!(
            outcome
                .actions
                .contains(&Action::Truncated { lines: 4, max: 3 })
        );
    }

    #[test]
    fn reports_non_ascii_and_length_together() {
        let source = "// NOTE: привет\n// second\n// third\n// fourth\n";
        let outcome = outcome(source);
        assert!(outcome.actions.contains(&Action::NonAscii));
        assert!(
            outcome
                .actions
                .contains(&Action::Truncated { lines: 4, max: 3 })
        );
    }

    #[test]
    fn recognizes_decorated_keywords() {
        let cases = [
            ("// **NOTE**: x\n", "// NOTE: x\n"),
            ("// *NOTE*: x\n", "// NOTE: x\n"),
            ("// [NOTE]: x\n", "// NOTE: x\n"),
            ("// [TODO]: x\n", "// TODO: x\n"),
            ("// - TODO: x\n", "// TODO: x\n"),
            ("// 1. FIXME: x\n", "// FIXME: x\n"),
            ("// 2. to-do: x\n", "// TODO: x\n"),
        ];

        for (source, expected) in cases {
            let outcome = outcome(source);
            assert!(!outcome.actions.contains(&Action::NoKeyword), "{source}");
            assert!(outcome.actions.contains(&Action::Normalized), "{source}");
            assert_eq!(outcome.block.render().remove(0), expected.trim_end());
        }
    }

    #[test]
    fn keeps_block_star_when_dropping_decorations() {
        let source = "/** **NOTE**: x */\nlet x = 1;\n";
        let outcome = outcome(source);
        assert!(outcome.actions.contains(&Action::Normalized));
        assert_eq!(outcome.block.render(), vec!["/** NOTE: x */"]);
    }

    #[test]
    fn keyword_on_the_second_line_is_ignored() {
        let source = "// plain text\n// NOTE: x\n";
        let outcome = outcome(source);
        assert!(outcome.actions.contains(&Action::NoKeyword));
    }

    #[test]
    fn decoration_only_line_is_skipped() {
        let source = "// **\n// NOTE: x\n";
        let outcome = outcome(source);
        assert_eq!(outcome.actions, Vec::new());
        assert_eq!(outcome.block.render(), vec!["// **", "// NOTE: x"]);
    }

    #[test]
    fn render_keeps_code_around_trailing_comment() {
        let source = "let x = 1; // NOTE: it\n";
        assert_eq!(render(source), source);
    }
}
