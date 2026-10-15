use super::{config::CommentConfig, parser::CommentBlock};

fn mark_truncated(head: &str) -> String {
    let trimmed = head.trim_start();
    let label_end = match trimmed.find(':') {
        Some(index) => index + 1,
        None => first_word(trimmed).len(),
    };
    format!(" {} [cut]{}", &trimmed[..label_end], &trimmed[label_end..])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Removed,
    Normalized,
    Truncated { lines: usize, max: usize },
    NonAscii,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub block: CommentBlock,
    pub actions: Vec<Action>,
}

fn first_word(content: &str) -> &str {
    let trimmed = content.trim_start();
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

    let raw_word = strip_trailing_punctuation(first_word(&contents[0]).to_string());

    let Some(keyword) = config.canonical_keyword(&raw_word) else {
        actions.push(Action::Removed);
        return Outcome {
            block: CommentBlock {
                contents,
                ..block.clone()
            },
            actions,
        };
    };

    if keyword != raw_word {
        let span = contents[0].find(&raw_word).unwrap_or(0);
        let rest = &contents[0][span + raw_word.len()..];
        let mut rebuilt = String::with_capacity(contents[0].len());
        rebuilt.push_str(&contents[0][..span]);
        rebuilt.push_str(&keyword);
        rebuilt.push_str(rest);
        contents[0] = rebuilt;
        actions.push(Action::Normalized);
    }

    if contents.iter().any(|line| !line.is_ascii()) {
        actions.push(Action::NonAscii);
    }

    let max = config.max_lines.max(1);
    if contents.len() > max {
        let lines = contents.len();
        let head = mark_truncated(&contents[0]);
        let mut truncated = vec![head];
        truncated.extend(contents[1..max].iter().cloned());
        contents = truncated;
        actions.push(Action::Truncated { lines, max });
    }

    Outcome {
        block: CommentBlock {
            contents,
            ..block.clone()
        },
        actions,
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
