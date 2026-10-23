use super::{config::CommentConfig, parser::CommentBlock};

fn mark_truncated(head: &str) -> String {
    let prefix_len = head.len() - head_text(head).len();
    let trimmed = &head[prefix_len..];
    let label_end = match trimmed.find(':') {
        Some(index) => index + 1,
        None => first_word(trimmed).len(),
    };
    format!(
        "{}{} [cut]{}",
        &head[..prefix_len],
        &trimmed[..label_end],
        &trimmed[label_end..]
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Removed,
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

fn head_text(content: &str) -> &str {
    let trimmed = content.trim_start();
    if let Some(stripped) = trimmed.strip_prefix('*') {
        stripped.trim_start()
    } else {
        trimmed
    }
}

fn first_word(content: &str) -> &str {
    let trimmed = head_text(content);
    let end = trimmed
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

fn strip_trailing_punctuation(mut word: String) -> String {
    while word.ends_with(':') || word.ends_with('!') {
        word.pop();
    }
    word
}

pub fn apply(block: &CommentBlock, config: &CommentConfig) -> Outcome {
    let mut contents = block.contents.clone();

    while contents.len() > 1 && contents.last().is_some_and(|c| c.trim().is_empty()) {
        contents.pop();
    }

    let mut actions = Vec::new();

    let head = contents
        .iter()
        .position(|line| !head_text(line).is_empty())
        .unwrap_or(0);

    let raw_word = strip_trailing_punctuation(first_word(&contents[head]).to_string());

    let Some(canonical) = config.canonical_keyword(&raw_word) else {
        actions.push(Action::Removed);
        return Outcome {
            block: CommentBlock {
                contents,
                ..block.clone()
            },
            actions,
            keyword: None,
        };
    };

    let keyword = Keyword {
        raw: raw_word.clone(),
        canonical: canonical.clone(),
    };

    if canonical != raw_word {
        let span = contents[head].find(&raw_word).unwrap_or(0);
        let rest = &contents[head][span + raw_word.len()..];
        let mut rebuilt = String::with_capacity(contents[head].len());
        rebuilt.push_str(&contents[head][..span]);
        rebuilt.push_str(&canonical);
        rebuilt.push_str(rest);
        contents[head] = rebuilt;
        actions.push(Action::Normalized);
    }

    if contents.iter().any(|line| !line.is_ascii()) {
        actions.push(Action::NonAscii);
    }

    let max = config.max_lines.max(1);

    if contents.len() - head > max {
        let lines = contents.len() - head;
        let marked = mark_truncated(&contents[head]);
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
        keyword: Some(keyword),
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

    #[test]
    fn removes_comment_without_keyword() {
        let source = "// just a comment\nlet x = 1;\n";
        let outcome = apply(&blocks(source).remove(0), &CommentConfig::default());
        assert!(outcome.actions.contains(&Action::Removed));
    }

    #[test]
    fn keeps_note() {
        let source = "// NOTE: keep\n";
        let outcome = apply(&blocks(source).remove(0), &CommentConfig::default());
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
            let block = blocks(source).remove(0);
            let outcome = apply(&block, &CommentConfig::default());
            assert!(outcome.actions.contains(&Action::Normalized), "{source}");
            assert_eq!(outcome.block.render().remove(0), expected.trim_end());
        }
    }

    #[test]
    fn truncates_to_three_lines_with_note() {
        let source = "// NOTE: one\n// two\n// three\n// four\n// five\n";
        let block = blocks(source).remove(0);
        let outcome = apply(&block, &CommentConfig::default());
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
        let block = blocks(source).remove(0);
        let outcome = apply(&block, &CommentConfig::default());
        assert!(outcome.actions.contains(&Action::Normalized));
        assert_eq!(outcome.block.render(), vec!["/** TODO: fix it */"]);
    }

    #[test]
    fn truncates_block_comment_without_losing_star() {
        let source = "/**\n * NOTE: one\n * two\n * three\n * four\n */\nlet x = 1;\n";
        let block = blocks(source).remove(0);
        let outcome = apply(&block, &CommentConfig::default());
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
        let source = "// NOTE: привет\n";
        let block = blocks(source).remove(0);
        let outcome = apply(&block, &CommentConfig::default());
        assert!(outcome.actions.contains(&Action::NonAscii));
        assert_eq!(outcome.actions.len(), 1);
    }

    #[test]
    fn render_keeps_code_around_trailing_comment() {
        let source = "let x = 1; // NOTE: it\n";
        assert_eq!(render(source), source);
    }
}
