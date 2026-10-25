use super::{Finding, Kind, Tok, ident_text, paren_args, push_finding, string_at};

const TEST_FUNCTIONS: &[&str] = &["it", "test", "describe", "specify"];

const TITLE_MODIFIERS: &[&str] = &[
    "skip",
    "only",
    "each",
    "todo",
    "concurrent",
    "fails",
    "sequential",
    "retry",
];

pub fn check(source: &str, file: &str) -> Vec<Finding> {
    let tokens = tokenize(source);
    let mut findings = Vec::new();

    for index in 0..tokens.len() {
        let Some(name) = ident_text(&tokens[index]) else {
            continue;
        };

        if TEST_FUNCTIONS.contains(&name) {
            if !is_member_call(&tokens, index)
                && let Some((line, text)) = title(&tokens, index)
            {
                push_finding(&mut findings, file, line, Kind::Title, text);
            }
        } else if (name == "assert" || name == "expect")
            && let Some((line, text)) = message(&tokens, index)
        {
            push_finding(&mut findings, file, line, Kind::Assert, text);
        }
    }

    findings
}

/// NOTE: describe is also a schema method (z.string().describe("...")), so a
/// dotted name is skipped unless the base is test (test.describe("...")).
fn is_member_call(tokens: &[Tok], index: usize) -> bool {
    if index == 0 || !matches!(tokens[index - 1], Tok::Punct('.')) {
        return false;
    }

    let base = index.checked_sub(2).and_then(|at| ident_text(&tokens[at]));
    base != Some("test")
}

/// NOTE: the title is the first direct string argument: it("x") or
/// it.each(table)("x") or it.each`table`("x").
fn title(tokens: &[Tok], name_index: usize) -> Option<(usize, String)> {
    let mut cursor = modifier_chain(tokens, name_index + 1, TITLE_MODIFIERS);

    if matches!(tokens.get(cursor), Some(Tok::Str { .. })) {
        cursor += 1;
    }

    let (args, after) = paren_args(tokens, cursor)?;
    if let Some(found) = args.iter().find_map(|index| string_at(tokens, *index)) {
        return Some(found);
    }

    if matches!(tokens.get(after), Some(Tok::Punct('('))) {
        let (inner, _) = paren_args(tokens, after)?;
        return inner.iter().find_map(|index| string_at(tokens, *index));
    }

    None
}

/// NOTE: only a pure string as the last argument is a message:
/// assert(cond, "msg"), expect(value, "msg"). expect(value) is not.
fn message(tokens: &[Tok], name_index: usize) -> Option<(usize, String)> {
    let cursor = modifier_chain(tokens, name_index + 1, &[]);
    let (args, _) = paren_args(tokens, cursor)?;

    let last = *args.last()?;
    let commas = args
        .iter()
        .filter(|index| matches!(tokens[**index], Tok::Punct(',')))
        .count();
    if commas == 0 {
        return None;
    }

    string_at(tokens, last)
}

/// NOTE: after the name the chain test.only.each(...) is dropped; with an empty
/// modifier list every dotted identifier is accepted (assert.equal, assert.deep).
fn modifier_chain(tokens: &[Tok], mut cursor: usize, allowed: &[&str]) -> usize {
    while let (Some(Tok::Punct('.')), Some(Tok::Ident { text, .. })) =
        (tokens.get(cursor), tokens.get(cursor + 1))
    {
        if !allowed.is_empty() && !allowed.contains(&text.as_str()) {
            break;
        }
        cursor += 2;
    }
    cursor
}

fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
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
                index += 2;
                while index < chars.len() {
                    if chars[index] == '\n' {
                        line += 1;
                    }
                    if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
            }
            quote @ ('"' | '\'' | '`') => {
                let (text, end, end_line) = quoted(&chars, index, line, quote);
                tokens.push(Tok::Str { text, line });
                line = end_line;
                index = end;
            }
            c if is_ident_char(c) => {
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

fn quoted(chars: &[char], start: usize, start_line: usize, quote: char) -> (String, usize, usize) {
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
            '\n' if quote != '`' => break,
            c if c == quote => {
                cursor += 1;
                break;
            }
            c => {
                text.push(c);
                cursor += 1;
            }
        }
    }

    (text, cursor, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(source: &str) -> Vec<(usize, String)> {
        check(source, "a.test.ts")
            .into_iter()
            .map(|finding| (finding.line, finding.text))
            .collect()
    }

    #[test]
    fn ascii_title_is_ok() {
        assert!(check("it(\"works\")\n", "a.test.ts").is_empty());
        assert!(
            check(
                "describe('outer', () => { it(\"inner\"); });\n",
                "a.test.ts"
            )
            .is_empty()
        );
    }

    #[test]
    fn non_ascii_title_is_reported() {
        let findings = check("it(\"тест\")\n", "a.test.ts");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, 1);
        assert_eq!(findings[0].kind, Kind::Title);
        assert_eq!(findings[0].text, "тест");
    }

    #[test]
    fn modifiers_are_followed() {
        let cases = [
            "it.skip(\"тест\");\n",
            "test.only(\"тест\");\n",
            "describe.todo(\"тест\");\n",
            "specify.concurrent(\"тест\");\n",
            "it.fails(\"тест\");\n",
        ];
        for source in cases {
            let findings = check(source, "a.test.ts");
            assert_eq!(findings.len(), 1, "{source}");
            assert_eq!(findings[0].kind, Kind::Title, "{source}");
        }
    }

    #[test]
    fn member_describe_is_not_a_title() {
        assert!(
            check(
                "const s = z.string().describe(\"тест\");\nz.object({ a: z.string() }).describe(\"тест\");\n",
                "a.test.ts"
            )
            .is_empty()
        );
    }

    #[test]
    fn member_test_call_is_not_a_title() {
        assert!(check("fixture.it(\"тест\");\nobj.test(\"тест\");\n", "a.test.ts").is_empty());
    }

    #[test]
    fn playwright_suite_title_is_reported() {
        assert_eq!(
            titles("test.describe(\"тест\", () => {});\n"),
            vec![(1, "тест".to_string())]
        );
        assert_eq!(
            titles("test.describe.only(\"тест\", () => {});\n"),
            vec![(1, "тест".to_string())]
        );
    }

    #[test]
    fn unknown_modifier_breaks_the_chain() {
        assert!(check("it.custom(\"тест\")\n", "a.test.ts").is_empty());
    }

    #[test]
    fn each_table_then_title() {
        assert_eq!(
            titles("it.each([[1, 2]])(\"титл\");\n"),
            vec![(1, "титл".to_string())]
        );
    }

    #[test]
    fn each_template_table_then_title() {
        // NOTE: the template table itself is not a title, only the string
        // after it is reported once.
        assert_eq!(
            titles("it.each`таблица`( \"титл\" );\n"),
            vec![(1, "титл".to_string())]
        );
    }

    #[test]
    fn title_on_later_line() {
        let source = "const a = 1;\nconst b = 2;\nit(\"тест\", () => {});\n";
        assert_eq!(titles(source), vec![(3, "тест".to_string())]);
    }

    #[test]
    fn comments_and_strings_are_not_matched() {
        let source = "// it(\"тест\")\n/* describe(\"тест\") */\nconst s = \"it('тест')\";\n";
        assert!(check(source, "a.test.ts").is_empty());
    }

    #[test]
    fn escaped_unicode_is_ascii_in_source() {
        assert!(check("it(\"\\u0442\\u0435\\u0441\\u0442\")\n", "a.test.ts").is_empty());
    }

    #[test]
    fn template_title_is_checked() {
        assert_eq!(
            titles("it(`привет ${name}`);\n"),
            vec![(1, "привет ${name}".to_string())]
        );
    }

    #[test]
    fn template_lines_are_counted() {
        let source = "const a = 1;\nit(`один\nдва`);\n";
        assert_eq!(titles(source), vec![(2, "один\nдва".to_string())]);
    }

    #[test]
    fn lookalike_names_are_ignored() {
        assert!(
            check(
                "itself(\"тест\")\nmytest(\"тест\")\ntest2(\"тест\")\n",
                "a.test.ts"
            )
            .is_empty()
        );
    }

    #[test]
    fn unterminated_call_reports_nothing() {
        assert!(check("it(\"тест\"\n", "a.test.ts").is_empty());
    }

    #[test]
    fn assert_message_is_reported() {
        let findings = check("assert(cond, \"сообщение\");\n", "a.test.ts");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, Kind::Assert);
        assert_eq!(findings[0].text, "сообщение");
    }

    #[test]
    fn assert_method_message_is_reported() {
        let findings = check("assert.equal(a, b, \"сообщение\");\n", "a.test.ts");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, Kind::Assert);
    }

    #[test]
    fn expect_second_argument_is_reported() {
        assert_eq!(
            check("expect(x, \"сообщение\").toBe(1);\n", "a.test.ts").len(),
            1
        );
    }

    #[test]
    fn expect_value_is_not_a_message() {
        assert!(
            check(
                "expect(\"иван\").toBe(1);\nexpect(get()).toBe(\"иван\");\n",
                "a.test.ts"
            )
            .is_empty()
        );
    }

    #[test]
    fn concatenated_message_is_ignored() {
        assert!(check("assert(cond, \"сообщение\" + suffix);\n", "a.test.ts").is_empty());
    }

    #[test]
    fn nested_call_message_is_ignored() {
        assert!(check("assert(cond, format(\"сообщение\"));\n", "a.test.ts").is_empty());
    }
}
