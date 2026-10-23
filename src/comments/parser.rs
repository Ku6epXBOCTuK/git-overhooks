use std::path::Path;

use super::strings::StringStyle;

pub struct Syntax {
    pub line: &'static [&'static str],
    pub block: bool,
    pub html: bool,
    pub jsdoc: bool,
    pub strings: StringStyle,
}

const SLASH: &[&str] = &["//"];
const HASH: &[&str] = &["#"];
const DASH: &[&str] = &["--"];

const BLOCK_OPEN: &str = "/*";
const BLOCK_CLOSE: &str = "*/";
const HTML_OPEN: &str = "<!--";
const HTML_CLOSE: &str = "-->";

fn string_style(ext: &str) -> StringStyle {
    match ext {
        "rs" => StringStyle::Raw,
        "js" | "jsx" | "ts" | "tsx" | "mts" | "cts" | "mjs" | "cjs" | "go" => StringStyle::Backtick,
        "py" | "toml" => StringStyle::Triple,
        "sh" | "bash" | "zsh" | "php" | "rb" => StringStyle::Heredoc,
        "yaml" | "yml" => StringStyle::YamlBlock,
        "lua" => StringStyle::LuaLong,
        _ => StringStyle::default(),
    }
}

pub fn syntax_for(path: &Path) -> Option<Syntax> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();

    let (line, block, html, jsdoc): (&[&str], bool, bool, bool) = match ext.as_str() {
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => (SLASH, true, false, true),
        "rs" | "css" | "scss" | "sass" | "less" | "go" | "c" | "h" | "cc" | "cpp" | "hpp"
        | "java" | "kt" | "kts" | "cs" | "swift" | "scala" | "php" | "m" | "mm" | "dart"
        | "zig" | "groovy" | "gradle" => (SLASH, true, false, false),
        "svelte" | "vue" | "astro" => (SLASH, true, true, true),
        "html" | "htm" | "xml" | "svg" => (SLASH, true, true, false),
        "py" | "rb" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "toml" | "yaml" | "yml" | "pl"
        | "pm" | "r" | "jl" | "ex" | "exs" | "nim" | "make" | "mk" | "tf" | "hcl" | "cfg"
        | "ini" => (HASH, false, false, false),
        "sql" | "lua" | "hs" | "adb" => (DASH, false, false, false),
        _ => return None,
    };

    Some(Syntax {
        line,
        block,
        html,
        jsdoc,
        strings: string_style(&ext),
    })
}

const DIRECTIVES: &[&str] = &[
    "!",
    "@ts-",
    "@flow",
    "@jsx",
    "@vue",
    "@internal",
    "@__PURE__",
    "biome-ignore",
    "checkstyle:",
    "code:",
    "deno-lint-ignore",
    "eslint-",
    "go:",
    "istanbul",
    "jshint",
    "jslint",
    "noinspection",
    "nolint",
    "prettier-ignore",
    "region",
    "endregion",
    "stylelint",
    "suppress",
    "ts-ignore",
    "tslint:",
    "type:",
    "vim:",
    "vscode:",
];

#[derive(Debug, Clone)]
pub struct CommentBlock {
    pub start: usize,
    pub end: usize,
    pub marker: String,
    pub indent: String,
    pub pre: String,
    pub close: Option<String>,
    pub contents: Vec<String>,
}

impl CommentBlock {
    pub fn render(&self) -> Vec<String> {
        if let Some(close) = &self.close {
            let last = self.contents.len().saturating_sub(1);
            return self
                .contents
                .iter()
                .enumerate()
                .map(|(index, content)| {
                    let mut line = String::from(&self.indent);
                    if index == 0 {
                        line.push_str(&self.marker);
                    }
                    line.push_str(content);
                    if index == last {
                        line.push_str(close);
                    }
                    line
                })
                .collect();
        }

        self.contents
            .iter()
            .enumerate()
            .map(|(index, content)| {
                if index == 0 {
                    if self.pre.is_empty() {
                        format!("{}{}{}", self.indent, self.marker, content)
                    } else {
                        format!("{}{}{}", self.indent, self.pre, content)
                    }
                } else {
                    format!("{}{}{}", self.indent, self.marker, content)
                }
            })
            .collect()
    }
}

fn find_marker<'a>(trimmed: &'a str, markers: &[&'a str]) -> Option<&'a str> {
    markers
        .iter()
        .filter(|m| trimmed.starts_with(**m))
        .max_by_key(|m| m.len())
        .copied()
}

/// NOTE: detects a marker placed after code on the same line; quoted text is
/// treated as code so string literals containing "//" are never rewritten.
fn find_trailing_marker<'a>(trimmed: &'a str, markers: &[&'a str]) -> Option<(&'a str, usize)> {
    let mut best: Option<(&str, usize)> = None;

    for marker in markers {
        let mut from = 0;
        while let Some(index) = trimmed[from..].find(marker) {
            let start = from + index;
            from = start + marker.len();

            if start == 0 || !trimmed[..start].ends_with([' ', '\t']) {
                continue;
            }
            if trimmed[..start].trim().is_empty() {
                continue;
            }

            let rest = &trimmed[start + marker.len()..];
            let looks_like_text = !rest.contains(['"', '\'', '`'])
                && rest
                    .trim_start()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic());
            if !looks_like_text {
                continue;
            }

            if best.is_none_or(|(_, best_start)| start < best_start) {
                best = Some((marker, start));
            }
            break;
        }
    }

    best
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

/// NOTE: AI often wraps a plain note into a `/** */` block, so the marker alone
/// is not proof of documentation. A real `JSDoc` comment always carries a tag, a
/// type, an intra-doc link or markdown.
fn is_jsdoc(contents: &[String]) -> bool {
    contents.iter().any(|line| {
        let line = strip_block_decoration(line);
        let line = line.trim();

        if JSDOC_TAGS.iter().any(|tag| line.starts_with(tag)) {
            return true;
        }
        if JSDOC_LINKS.iter().any(|link| line.contains(link)) {
            return true;
        }
        if line.contains("```") || line.contains("http://") || line.contains("https://") {
            return true;
        }
        if line.contains("](") {
            return true;
        }
        has_type_brace(line)
    })
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

fn is_doc_comment(trimmed: &str) -> bool {
    trimmed.starts_with("///")
        || trimmed.starts_with("//!")
        || trimmed.starts_with("/*!")
        || trimmed.starts_with("*/")
        || trimmed == "*"
        || trimmed.starts_with("* ")
}

fn is_directive(content: &str) -> bool {
    let content = content.trim_start_matches([' ', '\t', ':', '-', '*']);
    DIRECTIVES.iter().any(|d| content.starts_with(d))
}

fn clean_content(raw: &str) -> String {
    raw.trim_end_matches(['\r']).trim_end().to_string()
}

fn strip_block_decoration(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let without_star = trimmed.strip_prefix('*').unwrap_or(trimmed);
    without_star
        .strip_prefix(' ')
        .unwrap_or(without_star)
        .to_string()
}

/// NOTE: unterminated block comments are skipped entirely, rewriting a partial
/// comment would risk eating real code.
fn parse_block_comment(
    lines: &[&str],
    start: usize,
    marker: &str,
    close_token: &str,
) -> Option<CommentBlock> {
    let first = lines[start];
    let bom_len = first.len() - first.trim_start_matches('\u{feff}').len();
    let indent_len = bom_len + (first[bom_len..].len() - first[bom_len..].trim_start().len());
    let indent = first[..indent_len].to_string();
    let body = &first[indent_len + marker.len()..];

    let mut contents = Vec::new();

    if let Some(index) = body.find(close_token) {
        contents.push(body[..index].to_string());
        let end = start;
        let close = body[index..].trim_end_matches(['\r']).to_string();
        return Some(CommentBlock {
            start,
            end,
            marker: marker.to_string(),
            indent,
            pre: String::new(),
            close: Some(close),
            contents,
        });
    }

    contents.push(clean_content(body));
    let mut index = start;
    loop {
        index += 1;
        if index >= lines.len() {
            return None;
        }
        let line = lines[index];
        match line.find(close_token) {
            Some(position) => {
                contents.push(clean_content(&line[..position]));
                let end = index;
                let close = format!(
                    "{}{}",
                    &line[indent_len.min(position)..position],
                    close_token
                );
                return Some(CommentBlock {
                    start,
                    end,
                    marker: marker.to_string(),
                    indent,
                    pre: String::new(),
                    close: Some(close),
                    contents,
                });
            }
            None => contents.push(clean_content(line)),
        }
    }
}

pub fn parse(source: &str, syntax: &Syntax) -> Vec<CommentBlock> {
    let lines: Vec<&str> = source.lines().collect();
    let skip = super::strings::skip_lines(source, syntax);
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let trimmed = lines[index].trim_start_matches('\u{feff}').trim_start();

        if skip[index] {
            index += 1;
            continue;
        }

        if index == 0 && trimmed.starts_with("#!") {
            index += 1;
            continue;
        }

        if trimmed.starts_with(HTML_OPEN) && syntax.html {
            if let Some(block) = parse_block_comment(&lines, index, HTML_OPEN, HTML_CLOSE) {
                index = block.end + 1;
                if !is_directive(&block.contents[0]) {
                    blocks.push(block);
                }
                continue;
            }
            index += 1;
            continue;
        }

        if trimmed.starts_with(BLOCK_OPEN) && syntax.block {
            if trimmed.starts_with("/**") || trimmed.starts_with("/*!") {
                match parse_block_comment(&lines, index, BLOCK_OPEN, BLOCK_CLOSE) {
                    Some(block) if syntax.jsdoc && is_jsdoc(&block.contents) => {
                        index = skip_block(&lines, index);
                    }
                    Some(block) => {
                        index = block.end + 1;
                        if !is_directive(&block.contents[0]) {
                            blocks.push(block);
                        }
                    }
                    None => index = skip_block(&lines, index),
                }
                continue;
            }
            match parse_block_comment(&lines, index, BLOCK_OPEN, BLOCK_CLOSE) {
                Some(block) => {
                    index = block.end + 1;
                    if !is_directive(&block.contents[0]) {
                        blocks.push(block);
                    }
                }
                None => index = skip_block(&lines, index),
            }
            continue;
        }

        if is_doc_comment(trimmed) {
            index += 1;
            continue;
        }

        let leading = find_marker(trimmed, syntax.line);
        let leading_offset = lines[index].len() - trimmed.len();

        let (marker, marker_start, indent, pre) = if let Some(marker) = leading {
            (
                marker,
                leading_offset,
                lines[index][..leading_offset].to_string(),
                String::new(),
            )
        } else {
            let Some((marker, start)) = find_trailing_marker(trimmed, syntax.line) else {
                index += 1;
                continue;
            };
            let absolute = leading_offset + start;
            (
                marker,
                absolute,
                lines[index][..absolute].to_string(),
                lines[index][absolute..absolute + marker.len()].to_string(),
            )
        };

        let rest = &lines[index][marker_start + marker.len()..];
        if is_directive(rest) {
            index += 1;
            continue;
        }

        let mut contents = vec![clean_content(rest)];
        let mut end = index;
        while pre.is_empty() && end + 1 < lines.len() {
            if skip[end + 1] {
                break;
            }
            let next = lines[end + 1].trim_start();
            if find_marker(next, syntax.line).is_none() {
                break;
            }
            let next_rest = &next[marker.len()..];
            if is_directive(next_rest) {
                break;
            }
            end += 1;
            contents.push(clean_content(next_rest));
        }

        blocks.push(CommentBlock {
            start: index,
            end,
            marker: marker.to_string(),
            indent,
            pre,
            close: None,
            contents,
        });

        index = end + 1;
    }

    blocks
}

fn skip_block(lines: &[&str], start: usize) -> usize {
    let mut index = start;
    while index < lines.len() {
        if lines[index].contains(BLOCK_CLOSE) {
            return index + 1;
        }
        index += 1;
    }
    lines.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slash() -> Syntax {
        Syntax {
            line: SLASH,
            block: true,
            html: false,
            jsdoc: false,
            strings: StringStyle::default(),
        }
    }

    fn hash() -> Syntax {
        Syntax {
            line: HASH,
            block: false,
            html: false,
            jsdoc: false,
            strings: StringStyle::default(),
        }
    }

    fn markup() -> Syntax {
        Syntax {
            line: SLASH,
            block: true,
            html: true,
            jsdoc: false,
            strings: StringStyle::default(),
        }
    }

    fn script() -> Syntax {
        Syntax {
            line: SLASH,
            block: true,
            html: false,
            jsdoc: true,
            strings: StringStyle::default(),
        }
    }

    #[test]
    fn groups_consecutive_lines() {
        let source = "// a\n// b\nlet x = 1;\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec![" a", " b"]);
        assert_eq!(blocks[0].start, 0);
        assert_eq!(blocks[0].end, 1);
    }

    #[test]
    fn skips_doc_comments() {
        let source = "/// doc\n//! module\n// real\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec![" real"]);
    }

    #[test]
    fn skips_directives() {
        let source = "// eslint-disable-next-line\n// prettier-ignore\n// go:generate x\n// nolint: all\n// real\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec![" real"]);
    }

    #[test]
    fn trailing_comment_keeps_code() {
        let source = "let x = 1; // NOTE: keep\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].render(), vec!["let x = 1; // NOTE: keep"]);
    }

    #[test]
    fn hash_syntax() {
        let source = "#!/usr/bin/env python\n# NOTE: run\n# code\n";
        let blocks = parse(source, &hash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec![" NOTE: run", " code"]);
    }

    #[test]
    fn byte_order_mark_is_ignored() {
        let source = "\u{feff}// NOTE: keep\nlet x = 1;\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec![" NOTE: keep"]);
    }

    #[test]
    fn byte_order_mark_before_jsdoc() {
        let source = "\u{feff}/** to-do: fix */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec!["* to-do: fix "]);
    }

    #[test]
    fn known_extensions() {
        assert!(syntax_for(Path::new("a.rs")).is_some());
        assert!(syntax_for(Path::new("a.ts")).is_some());
        assert!(syntax_for(Path::new("a.css")).is_some());
        assert!(syntax_for(Path::new("a.svelte")).is_some());
        assert!(syntax_for(Path::new("a.md")).is_none());
        assert!(syntax_for(Path::new("a.bin")).is_none());
    }

    #[test]
    fn inline_block_comment() {
        let source = "/* junk */\n.a {}\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec![" junk "]);
        assert_eq!(blocks[0].render(), vec!["/* junk */"]);
    }

    #[test]
    fn multiline_block_comment() {
        let source = "/*\n * one\n * two\n */\n.b {}\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start, 0);
        assert_eq!(blocks[0].end, 3);
        assert_eq!(blocks[0].render().len(), 4);
        assert!(blocks[0].render().last().unwrap().ends_with("*/"));
    }

    #[test]
    fn jsdoc_with_tag_is_skipped() {
        let source = "/**\n * @param {string} a - x\n * @returns {void}\n */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert!(blocks.is_empty());
    }

    #[test]
    fn jsdoc_with_deprecated_is_skipped() {
        let source = "/** @deprecated use b */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert!(blocks.is_empty());
    }

    #[test]
    fn jsdoc_with_code_sample_is_skipped() {
        let source = "/**\n * Example:\n * ```js\n * a();\n * ```\n */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert!(blocks.is_empty());
    }

    #[test]
    fn jsdoc_with_link_is_skipped() {
        let source = "/** see {@link helper} for details */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert!(blocks.is_empty());
    }

    #[test]
    fn prose_jsdoc_is_collected() {
        let source = "/**\n * Formats a date.\n */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].render()[0], "/**");
        assert!(blocks[0].render().last().unwrap().ends_with("*/"));
    }

    #[test]
    fn jsdoc_wrapped_note_is_collected() {
        let source = "/** to-do: fix it */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].contents, vec!["* to-do: fix it "]);
        assert_eq!(blocks[0].render(), vec!["/** to-do: fix it */"]);
    }

    #[test]
    fn terse_block_is_treated_as_directive() {
        let source = "/*! terse */\nlet x = 1;\n";
        let blocks = parse(source, &script());
        assert!(blocks.is_empty());
    }

    #[test]
    fn jsdoc_is_untouched_outside_script() {
        let source = "/** @param {string} a */\nlet x = 1;\n";
        let blocks = parse(source, &slash());
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn unterminated_block_is_skipped() {
        let source = "/* never closed\nlet x = 1;\n";
        let blocks = parse(source, &slash());
        assert!(blocks.is_empty());
    }

    #[test]
    fn block_directive_is_skipped() {
        let source = "/* eslint-disable no-console */\nlet x = 1;\n";
        let blocks = parse(source, &slash());
        assert!(blocks.is_empty());
    }

    #[test]
    fn html_comments() {
        let source = "<!-- old markup\n<p>x</p>\n-->\n<p>keep</p>\n";
        let blocks = parse(source, &markup());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].render()[0], "<!-- old markup");
        assert_eq!(blocks[0].render().last().unwrap().trim_end(), "-->");
    }

    #[test]
    fn html_not_parsed_for_plain_script() {
        let source = "<!-- comment -->\nlet x = 1;\n";
        assert!(parse(source, &slash()).is_empty());
    }
}
