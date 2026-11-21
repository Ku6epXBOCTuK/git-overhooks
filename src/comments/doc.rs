#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocPolicy {
    None,
    JsDoc { plain_allowed: bool },
    RustDoc { plain_allowed: bool },
}

impl DocPolicy {
    pub fn none() -> Self {
        Self::None
    }

    pub fn jsdoc(plain_allowed: bool) -> Self {
        Self::JsDoc { plain_allowed }
    }

    pub fn rustdoc(plain_allowed: bool) -> Self {
        Self::RustDoc { plain_allowed }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Plain,
    Bang,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Tag,
    Link,
    Code,
    Type,
}

const JSDOC_TAGS: &[&str] = &[
    "@param",
    "@returns",
    "@return",
    "@throws",
    "@type",
    "@typedef",
    "@property",
    "@example",
    "@deprecated",
    "@description",
    "@summary",
    "@remarks",
    "@template",
    "@see",
    "@extends",
    "@implements",
    "@default",
    "@enum",
    "@private",
    "@public",
];

const JSDOC_LINKS: &[&str] = &["{@link", "{@linkcode", "{@linkplain}"];

fn strip_block_decoration(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let without_star = trimmed.strip_prefix('*').unwrap_or(trimmed);
    without_star
        .strip_prefix(' ')
        .unwrap_or(without_star)
        .to_string()
}

fn has_type_brace(line: &str) -> bool {
    let mut rest = line;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return false;
        };
        let inner = &after[..end];
        if inner.chars().any(|c| c.is_ascii_alphabetic()) {
            return true;
        }
        rest = &after[end + 1..];
    }
    false
}

// NOTE: the marker alone is not proof of documentation: a plain note is often
// wrapped into a `/** */`. A real doc carries a tag, type, link or code.
pub fn signal(contents: &[String]) -> Option<Signal> {
    for line in contents {
        let line = strip_block_decoration(line);
        let line = line.trim();

        if JSDOC_TAGS.iter().any(|tag| line.starts_with(tag)) {
            return Some(Signal::Tag);
        }
        if JSDOC_LINKS.iter().any(|link| line.contains(link)) {
            return Some(Signal::Link);
        }
        if line.contains("```") {
            return Some(Signal::Code);
        }
        if line.contains("http://") || line.contains("https://") || line.contains("](") {
            return Some(Signal::Link);
        }
        if has_type_brace(line) {
            return Some(Signal::Type);
        }
    }
    None
}

fn rust_code_indent(raw: &str) -> usize {
    let mut chars = raw.chars().peekable();
    let mut indent: usize = 0;
    let mut star = false;
    while let Some(ch) = chars.peek().copied() {
        match ch {
            ' ' => indent += 1,
            '*' if !star => star = true,
            _ => break,
        }
        chars.next();
    }
    if star {
        indent.saturating_sub(1)
    } else {
        indent
    }
}

fn is_rust_param_line(line: &str) -> bool {
    let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("| "))
    else {
        return false;
    };
    rest.starts_with('`') && rest[1..].contains('`')
}

// NOTE: a bare `/// note` is a note, not documentation. rustdoc markers only
// become documentation when the body carries a section, code, a link or a
// parameter description, the same way a JSDoc block needs a signal.
pub fn rust_signal(contents: &[String]) -> Option<Signal> {
    for raw in contents {
        let cleaned = strip_block_decoration(raw);
        let line = cleaned.trim();

        if line.starts_with('#') && line.len() > 1 {
            return Some(Signal::Tag);
        }
        if line.starts_with("```") || line.starts_with("    ") || rust_code_indent(raw) >= 4 {
            return Some(Signal::Code);
        }
        if line.contains("](")
            || line.contains("][")
            || line.contains("`]")
            || line.starts_with("<http")
        {
            return Some(Signal::Link);
        }
        if is_rust_param_line(line) {
            return Some(Signal::Tag);
        }
        if line.contains('`') {
            return Some(Signal::Code);
        }
    }
    None
}

const DECLARATION_WORDS: &[&str] = &[
    "abstract",
    "async",
    "class",
    "const",
    "constructor",
    "declare",
    "enum",
    "export",
    "function",
    "import",
    "interface",
    "let",
    "module",
    "namespace",
    "type",
    "var",
];

const MEMBER_MODIFIERS: &[&str] = &[
    "abstract",
    "async",
    "declare",
    "export",
    "get",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "set",
    "static",
];

fn starts_with_word(line: &str, word: &str) -> bool {
    let Some(rest) = line.strip_prefix(word) else {
        return false;
    };
    rest.starts_with(|ch: char| ch.is_whitespace() || ch == '(')
}

fn strip_modifier(line: &str) -> Option<&str> {
    MEMBER_MODIFIERS.iter().find_map(|modifier| {
        line.strip_prefix(modifier)
            .filter(|rest| rest.starts_with(char::is_whitespace))
    })
}

// NOTE: a member is `name?: Type`, the shape every prop description in a
// `ts` type literal has; without the colon it is a statement, not a member.
fn is_member_signature(line: &str) -> bool {
    let mut rest = line.trim_start();
    while let Some(stripped) = strip_modifier(rest) {
        rest = stripped.trim_start();
    }

    let bytes = rest.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || *first == b'_' || *first == b'$') {
        return false;
    }

    let mut index = 1;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'$'))
    {
        index += 1;
    }
    while bytes.get(index) == Some(&b' ') {
        index += 1;
    }
    if matches!(bytes.get(index), Some(b'?' | b'!')) {
        index += 1;
        while bytes.get(index) == Some(&b' ') {
            index += 1;
        }
    }

    bytes.get(index) == Some(&b':')
}

fn is_declaration_line(line: &str) -> bool {
    let line = line.trim_start();
    DECLARATION_WORDS
        .iter()
        .any(|word| starts_with_word(line, word))
        || is_member_signature(line)
}

// NOTE: the block must touch the declaration, no blank line: TypeScript
// attaches JSDoc only to the immediately following item, so a detached block
// documents nothing and stays a plain comment.
fn is_attached_to_declaration(line: Option<&str>) -> bool {
    line.is_some_and(is_declaration_line)
}

// NOTE: a tag alone can disguise a note as JSDoc; a detached block documents
// nothing in TypeScript, so both the tagged and plain paths need attachment.
pub fn is_documentation(
    contents: &[String],
    next: Option<&str>,
    marker: Marker,
    policy: DocPolicy,
) -> bool {
    match policy {
        DocPolicy::None => false,
        DocPolicy::JsDoc { plain_allowed } => {
            is_attached_to_declaration(next)
                && (signal(contents).is_some() || (marker == Marker::Plain && plain_allowed))
        }
        // NOTE: the `///` marker already attaches a rustdoc block to the next
        // item, and `//!` attaches it to the enclosing one, so there is no
        // declaration check here.
        DocPolicy::RustDoc { plain_allowed } => {
            rust_signal(contents).is_some() || (marker == Marker::Plain && plain_allowed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contents(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_string()).collect()
    }

    fn documented(lines: &[&str], next: Option<&str>, marker: Marker, plain_allowed: bool) -> bool {
        is_documentation(
            &contents(lines),
            next,
            marker,
            DocPolicy::jsdoc(plain_allowed),
        )
    }

    #[test]
    fn tag_is_a_signal() {
        assert_eq!(
            signal(&contents(&["* @param {string} a - x"])),
            Some(Signal::Tag)
        );
        assert_eq!(
            signal(&contents(&["* @description Text."])),
            Some(Signal::Tag)
        );
    }

    #[test]
    fn link_is_a_signal() {
        assert_eq!(
            signal(&contents(&["* see {@link helper}"])),
            Some(Signal::Link)
        );
        assert_eq!(
            signal(&contents(&["* [docs](https://example.com)"])),
            Some(Signal::Link)
        );
    }

    #[test]
    fn code_fence_is_a_signal() {
        assert_eq!(signal(&contents(&["* ```js"])), Some(Signal::Code));
    }

    #[test]
    fn type_brace_is_a_signal() {
        assert_eq!(
            signal(&contents(&["* value {string} here"])),
            Some(Signal::Type)
        );
    }

    #[test]
    fn plain_prose_has_no_signal() {
        assert_eq!(signal(&contents(&["* Formats a date."])), None);
    }

    #[test]
    fn tagged_block_above_declaration_is_documentation() {
        assert!(documented(
            &["* @deprecated use b"],
            Some("let x = 1;"),
            Marker::Plain,
            false
        ));
    }

    #[test]
    fn detached_tagged_block_is_not_documentation() {
        assert!(!documented(
            &["* @description Text."],
            Some(""),
            Marker::Plain,
            false
        ));
    }

    #[test]
    fn tagged_block_above_statement_is_not_documentation() {
        assert!(!documented(
            &["* @returns Text."],
            Some("return value;"),
            Marker::Plain,
            false
        ));
    }

    #[test]
    fn plain_prose_needs_opt_in_and_attachment() {
        assert!(documented(
            &["* Text on the button."],
            Some("label?: string;"),
            Marker::Plain,
            true
        ));
        assert!(!documented(
            &["* Text on the button."],
            Some("label?: string;"),
            Marker::Plain,
            false
        ));
        assert!(!documented(
            &["* Text on the button."],
            Some(""),
            Marker::Plain,
            true
        ));
    }

    #[test]
    fn bang_marker_is_never_plain_documentation() {
        assert!(!documented(
            &["* terse"],
            Some("export const size = 16;"),
            Marker::Bang,
            true
        ));
    }

    #[test]
    fn declaration_words_attach() {
        for next in [
            "function Button() {}",
            "export const size = 16",
            "interface Props {",
        ] {
            assert!(
                documented(&["* @returns void"], Some(next), Marker::Plain, false),
                "{next}"
            );
        }
    }

    #[test]
    fn member_signature_attaches() {
        assert!(documented(
            &["* @param a"],
            Some("  label?: string;"),
            Marker::Plain,
            false
        ));
    }

    #[test]
    fn none_policy_never_documents() {
        assert!(!is_documentation(
            &contents(&["* @param {string} a"]),
            Some("let x = 1;"),
            Marker::Plain,
            DocPolicy::none()
        ));
    }

    #[test]
    fn rust_signals_are_detected() {
        let cases: &[&[&str]] = &[
            &[" ```rust"],
            &["     let x = 1;"],
            &[" # Examples"],
            &[" #![allow(dead_code)]"],
            &[" [`Item`] and more"],
            &[" [text](https://example.com)"],
            &[" - `arg`: the value"],
            &[" * `arg` - the value"],
            &[" | `arg` | the value |"],
            &[" use `foo` here"],
        ];
        for lines in cases {
            assert!(rust_signal(&contents(lines)).is_some(), "{lines:?}");
        }
    }

    #[test]
    fn rust_indented_code_signal_survives_block_star() {
        assert!(rust_signal(&contents(&[" *     let x = 1;"])).is_some());
    }

    #[test]
    fn rust_plain_prose_has_no_signal() {
        assert_eq!(rust_signal(&contents(&[" note"])), None);
    }

    #[test]
    fn rust_doc_with_signal_is_documentation() {
        assert!(is_documentation(
            &contents(&[" # Examples"]),
            None,
            Marker::Plain,
            DocPolicy::rustdoc(false)
        ));
        assert!(is_documentation(
            &contents(&[" ```rust"]),
            None,
            Marker::Bang,
            DocPolicy::rustdoc(false)
        ));
    }

    #[test]
    fn rust_doc_without_signal_is_not_documentation() {
        assert!(!is_documentation(
            &contents(&[" note"]),
            Some("fn internal() {}"),
            Marker::Plain,
            DocPolicy::rustdoc(false)
        ));
    }

    #[test]
    fn rust_plain_doc_needs_opt_in() {
        assert!(is_documentation(
            &contents(&[" note"]),
            None,
            Marker::Plain,
            DocPolicy::rustdoc(true)
        ));
        assert!(!is_documentation(
            &contents(&[" note"]),
            None,
            Marker::Bang,
            DocPolicy::rustdoc(true)
        ));
    }
}
