pub mod js;
pub mod rs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Name of a test: it("...") or the function of a #[test].
    Title,
    /// Message text of an assertion.
    Assert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    /// The offending literal as it is written in the source.
    pub text: String,
}

/// NOTE: a regex literal (JS) is tokenized as plain punctuation, so a pattern
/// like `/it\("…"/` could confuse the matcher; that is accepted as a rare
/// trade-off for not building a full parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tok {
    Ident { text: String, line: usize },
    Str { text: String, line: usize },
    Punct(char),
}

pub(super) fn ident_text(token: &Tok) -> Option<&str> {
    match token {
        Tok::Ident { text, .. } => Some(text),
        _ => None,
    }
}

pub(super) fn string_at(tokens: &[Tok], index: usize) -> Option<(usize, String)> {
    match &tokens[index] {
        Tok::Str { text, line } => Some((*line, text.clone())),
        _ => None,
    }
}

/// NOTE: returns the tokens directly inside the parentheses (depth one) and the
/// index right after the closing one. Unmatched parentheses give None, so a
/// partial file never produces a wrong title.
pub(super) fn paren_args(tokens: &[Tok], open: usize) -> Option<(Vec<usize>, usize)> {
    if !matches!(tokens.get(open), Some(Tok::Punct('('))) {
        return None;
    }

    let mut depth = 0usize;
    let mut direct = Vec::new();

    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Tok::Punct('(') => depth += 1,
            Tok::Punct(')') => {
                depth -= 1;
                if depth == 0 {
                    return Some((direct, index + 1));
                }
            }
            _ if depth == 1 => direct.push(index),
            _ => {}
        }
    }

    None
}

pub(super) fn push_finding(
    findings: &mut Vec<Finding>,
    file: &str,
    line: usize,
    kind: Kind,
    text: String,
) {
    if !text.is_ascii() {
        findings.push(Finding {
            file: file.to_string(),
            line,
            kind,
            text,
        });
    }
}
