use super::{Finding, Kind, Tok, ident_text, paren_args, push_finding, string_at};

const ASSERT_MACROS: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
];

pub fn check(source: &str, file: &str) -> Vec<Finding> {
    let tokens = tokenize(source);
    let mut findings = Vec::new();

    for index in 0..tokens.len() {
        if is_test_attribute(&tokens, index)
            && let Some((line, name)) = test_fn_name(&tokens, index + 4)
        {
            push_finding(&mut findings, file, line, Kind::Title, name);
        }

        let Some(name) = ident_text(&tokens[index]) else {
            continue;
        };

        if (ASSERT_MACROS.contains(&name) || name == "panic")
            && let Some(open) = bang_paren(&tokens, index)
        {
            let found = if name == "panic" {
                first_string(&tokens, open)
            } else {
                last_string(&tokens, open)
            };
            if let Some((line, text)) = found {
                push_finding(&mut findings, file, line, Kind::Assert, text);
            }
        } else if is_expect_method(&tokens, index)
            && let Some(open) = call_open(&tokens, index)
            && let Some((line, text)) = first_string(&tokens, open)
        {
            push_finding(&mut findings, file, line, Kind::Assert, text);
        }
    }

    findings
}

/// NOTE: #[tokio::test] and friends are out of scope, only the plain #[test].
fn is_test_attribute(tokens: &[Tok], index: usize) -> bool {
    matches!(
        (tokens.get(index), tokens.get(index + 1), tokens.get(index + 2), tokens.get(index + 3)),
        (Some(Tok::Punct('#')), Some(Tok::Punct('[')), Some(token), Some(Tok::Punct(']')))
            if ident_text(token) == Some("test")
    )
}

/// NOTE: other attributes may sit between #[test] and the function
/// (#[should_panic], #[ignore]), so every next #[...] is dropped first.
fn test_fn_name(tokens: &[Tok], mut cursor: usize) -> Option<(usize, String)> {
    while is_attribute_start(tokens, cursor) {
        cursor = skip_attribute(tokens, cursor)?;
    }

    if ident_text(tokens.get(cursor)?) != Some("fn") {
        return None;
    }

    match tokens.get(cursor + 1)? {
        Tok::Ident { text, line } => Some((*line, text.clone())),
        _ => None,
    }
}

fn is_attribute_start(tokens: &[Tok], cursor: usize) -> bool {
    matches!(
        (tokens.get(cursor), tokens.get(cursor + 1)),
        (Some(Tok::Punct('#')), Some(Tok::Punct('[')))
    )
}

fn skip_attribute(tokens: &[Tok], cursor: usize) -> Option<usize> {
    for (index, token) in tokens.iter().enumerate().skip(cursor + 2) {
        if matches!(token, Tok::Punct(']')) {
            return Some(index + 1);
        }
    }
    None
}

/// NOTE: the message of an assert is the last string literal among the direct
/// arguments: assert!(cond, "msg {}", value). Strings of nested calls are
/// left alone.
fn last_string(tokens: &[Tok], open: usize) -> Option<(usize, String)> {
    let (args, _) = paren_args(tokens, open)?;
    args.iter()
        .rev()
        .find_map(|index| string_at(tokens, *index))
}

/// NOTE: panic! and .expect( explain a failure with their first literal.
fn first_string(tokens: &[Tok], open: usize) -> Option<(usize, String)> {
    let (args, _) = paren_args(tokens, open)?;
    args.iter().find_map(|index| string_at(tokens, *index))
}

fn bang_paren(tokens: &[Tok], index: usize) -> Option<usize> {
    matches!(
        (tokens.get(index + 1), tokens.get(index + 2)),
        (Some(Tok::Punct('!')), Some(Tok::Punct('(')))
    )
    .then_some(index + 2)
}

fn call_open(tokens: &[Tok], index: usize) -> Option<usize> {
    matches!(tokens.get(index + 1), Some(Tok::Punct('('))).then_some(index + 1)
}

fn is_expect_method(tokens: &[Tok], index: usize) -> bool {
    matches!(
        (
            tokens.get(index),
            index
                .checked_sub(1)
                .and_then(|previous| tokens.get(previous))
        ),
        (Some(Tok::Ident { text, .. }), Some(Tok::Punct('.'))) if text == "expect"
    )
}

fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn tokenize(source: &str) -> Vec<Tok> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut line = 1usize;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '\n' => {
                line += 1;
                index += 1;
            }
            c if c.is_whitespace() => index += 1,
            '/' if chars.get(index + 1) == Some(&'/') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '/' if chars.get(index + 1) == Some(&'*') => {
                // NOTE: Rust block comments nest, so they are counted, not
                // searched.
                let mut depth = 1usize;
                index += 2;
                while index < chars.len() && depth > 0 {
                    match (chars[index], chars.get(index + 1)) {
                        ('/', Some('*')) => {
                            depth += 1;
                            index += 2;
                        }
                        ('*', Some('/')) => {
                            depth -= 1;
                            index += 2;
                        }
                        (_, _) => {
                            if chars[index] == '\n' {
                                line += 1;
                            }
                            index += 1;
                        }
                    }
                }
            }
            '"' => {
                let (text, end, end_line) = double_quoted(&chars, index, line);
                tokens.push(Tok::Str { text, line });
                line = end_line;
                index = end;
            }
            '\'' => {
                index = apostrophe(&chars, index);
            }
            _ if raw_string_start(&chars, index).is_some() => {
                let (content, hashes) = raw_string_start(&chars, index).unwrap_or((index, 0));
                let (text, end, end_line) = raw_quoted(&chars, content, line, hashes);
                tokens.push(Tok::Str { text, line });
                line = end_line;
                index = end;
            }
            c if is_ident_char(c) => {
                // NOTE: r#ident is a raw identifier, the # belongs to it.
                if c == 'r' && chars.get(index + 1) == Some(&'#') {
                    index += 2;
                }
                let start = index;
                while index < chars.len() && is_ident_char(chars[index]) {
                    index += 1;
                }
                tokens.push(Tok::Ident {
                    text: chars[start..index].iter().collect(),
                    line,
                });
            }
            other => {
                tokens.push(Tok::Punct(other));
                index += 1;
            }
        }
    }

    tokens
}

fn raw_string_start(chars: &[char], index: usize) -> Option<(usize, usize)> {
    let mut cursor = index;
    if chars.get(cursor) == Some(&'b') {
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'r') {
        return None;
    }
    cursor += 1;

    let hashes_start = cursor;
    while chars.get(cursor) == Some(&'#') {
        cursor += 1;
    }
    if chars.get(cursor) != Some(&'"') {
        return None;
    }

    Some((cursor + 1, cursor - hashes_start))
}

fn double_quoted(chars: &[char], start: usize, start_line: usize) -> (String, usize, usize) {
    let mut text = String::new();
    let mut line = start_line;
    let mut cursor = start + 1;

    while cursor < chars.len() {
        let ch = chars[cursor];
        match ch {
            '\\' => {
                text.push(ch);
                cursor += 1;
                if cursor < chars.len() {
                    if chars[cursor] == '\n' {
                        line += 1;
                    }
                    text.push(chars[cursor]);
                    cursor += 1;
                }
            }
            '"' => {
                cursor += 1;
                break;
            }
            c => {
                if c == '\n' {
                    line += 1;
                }
                text.push(c);
                cursor += 1;
            }
        }
    }

    (text, cursor, line)
}

fn raw_quoted(
    chars: &[char],
    content: usize,
    start_line: usize,
    hashes: usize,
) -> (String, usize, usize) {
    let mut text = String::new();
    let mut line = start_line;
    let mut cursor = content;

    while cursor < chars.len() {
        let ch = chars[cursor];
        if ch == '"'
            && (cursor + 1..cursor + 1 + hashes).all(|offset| chars.get(offset) == Some(&'#'))
        {
            cursor += 1 + hashes;
            break;
        }
        if ch == '\n' {
            line += 1;
        }
        text.push(ch);
        cursor += 1;
    }

    (text, cursor, line)
}

/// NOTE: 'x' and '\n' are char literals and are consumed whole; everything else
/// starting with an apostrophe is a lifetime and only the quote is dropped.
fn apostrophe(chars: &[char], start: usize) -> usize {
    if chars.get(start + 1) == Some(&'\\') {
        let mut cursor = start + 1;
        while cursor < chars.len() {
            if chars[cursor] == '\\' {
                cursor += 2;
                continue;
            }
            cursor += 1;
            if chars.get(cursor - 1) == Some(&'\'') {
                break;
            }
        }
        return cursor;
    }
    if chars.get(start + 2) == Some(&'\'') {
        return start + 3;
    }
    start + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(source: &str) -> Vec<(usize, String)> {
        check(source, "a.rs")
            .into_iter()
            .map(|finding| (finding.line, finding.text))
            .collect()
    }

    #[test]
    fn ascii_fn_name_is_ok() {
        assert!(check("#[test]\nfn works() {}\n", "a.rs").is_empty());
    }

    #[test]
    fn non_ascii_fn_name_is_reported() {
        let findings = check("#[test]\nfn тест() {}\n", "a.rs");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 2);
        assert_eq!(findings[0].kind, Kind::Title);
        assert_eq!(findings[0].text, "тест");
    }

    #[test]
    fn attributes_between_test_and_fn_are_skipped() {
        let source = "#[test]\n#[should_panic]\n#[ignore]\nfn проверка() {}\n";
        assert_eq!(titles(source), vec![(4, "проверка".to_string())]);
    }

    #[test]
    fn cfg_test_attribute_is_not_matched() {
        assert!(check("#[cfg(test)]\nmod tests {}\n", "a.rs").is_empty());
    }

    #[test]
    fn plain_fn_is_not_matched() {
        assert!(check("fn тест() {}\n", "a.rs").is_empty());
    }

    #[test]
    fn test_attribute_without_fn_is_ignored() {
        assert!(check("#[test]\nconst X: u32 = 1;\n", "a.rs").is_empty());
    }

    #[test]
    fn assert_message_is_reported() {
        let findings = check("assert!(a.is_empty(), \"пусто: {a:?}\");\n", "a.rs");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, Kind::Assert);
        assert_eq!(findings[0].text, "пусто: {a:?}");
    }

    #[test]
    fn assert_without_message_is_ignored() {
        assert!(check("assert!(a.is_empty());\nassert_eq!(a, b);\n", "a.rs").is_empty());
    }

    #[test]
    fn debug_assert_variants_are_checked() {
        let source = "debug_assert_eq!(a, b, \"разные\");\ndebug_assert_ne!(a, b, \"равны\");\n";
        assert_eq!(titles(source).len(), 2);
    }

    #[test]
    fn nested_call_strings_are_not_messages() {
        let source = "assert!(a.iter().any(|v| v == \"число\"), \"сообщение\");\n";
        assert_eq!(titles(source), vec![(1, "сообщение".to_string())]);
    }

    #[test]
    fn panic_message_is_reported() {
        let findings = check("panic!(\"бум: {x}\");\n", "a.rs");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, Kind::Assert);
        assert_eq!(findings[0].text, "бум: {x}");
    }

    #[test]
    fn expect_message_is_reported() {
        let source = "let v = items.pop().expect(\"список пуст\");\n";
        assert_eq!(titles(source), vec![(1, "список пуст".to_string())]);
    }

    #[test]
    fn plain_expect_call_is_not_a_method() {
        assert!(check("expect(\"текст\");\n", "a.rs").is_empty());
    }

    #[test]
    fn raw_string_message_is_reported() {
        let source = "assert!(a, r#\"сообщение\"#);\n";
        assert_eq!(titles(source), vec![(1, "сообщение".to_string())]);
    }

    #[test]
    fn escaped_unicode_is_ascii_in_source() {
        assert!(check("panic!(\"\\u{442}\\u{435}\");\n", "a.rs").is_empty());
    }

    #[test]
    fn comments_are_not_matched() {
        let source = "// panic!(\"нет\")\n/* assert!(a, \"нет\") */\n";
        assert!(check(source, "a.rs").is_empty());
    }

    #[test]
    fn raw_identifier_is_still_a_name() {
        let findings = check("#[test]\nfn r#тест() {}\n", "a.rs");
        assert_eq!(
            titles("#[test]\nfn r#тест() {}\n"),
            vec![(2, "тест".to_string())]
        );
        assert_eq!(findings.len(), 1);
    }
}
