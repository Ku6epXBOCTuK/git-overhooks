use std::path::Path;

pub struct Syntax {
    pub line: &'static [&'static str],
}

const SLASH: &[&str] = &["//"];
const HASH: &[&str] = &["#"];
const DASH: &[&str] = &["--"];

pub fn syntax_for(path: &Path) -> Option<Syntax> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();

    let line: &'static [&'static str] = match ext.as_str() {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "go" | "c" | "h" | "cc" | "cpp"
        | "hpp" | "java" | "kt" | "kts" | "cs" | "swift" | "scala" | "php" | "m" | "mm"
        | "dart" | "zig" => SLASH,
        "py" | "rb" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "toml" | "yaml" | "yml" | "pl"
        | "pm" | "r" | "jl" | "ex" | "exs" | "nim" | "make" | "mk" | "dockerfile" | "tf"
        | "hcl" | "cfg" | "ini" => HASH,
        "sql" | "lua" | "hs" | "adb" => DASH,
        _ => return None,
    };

    Some(Syntax { line })
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
    pub contents: Vec<String>,
}

impl CommentBlock {
    pub fn render(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (index, content) in self.contents.iter().enumerate() {
            if index == 0 {
                if self.pre.is_empty() {
                    lines.push(format!("{}{}{}", self.indent, self.marker, content));
                } else {
                    lines.push(format!("{}{}{}", self.indent, self.pre, content));
                }
            } else {
                lines.push(format!("{}{}{}", self.indent, self.marker, content));
            }
        }
        lines
    }
}

fn find_marker<'a>(trimmed: &'a str, markers: &[&'a str]) -> Option<&'a str> {
    markers
        .iter()
        .filter(|m| trimmed.starts_with(**m))
        .max_by_key(|m| m.len())
        .copied()
}

fn is_doc_comment(trimmed: &str) -> bool {
    trimmed.starts_with("///")
        || trimmed.starts_with("//!")
        || trimmed.starts_with("/**")
        || trimmed.starts_with("/*!")
        || trimmed.starts_with("*/")
        || trimmed == "*"
        || trimmed.starts_with("* ")
}

fn is_directive(content: &str) -> bool {
    let content = content.trim_start_matches([' ', '\t', ':', '-']);
    DIRECTIVES.iter().any(|d| content.starts_with(d))
}

fn clean_content(raw: &str) -> String {
    raw.trim_end_matches(['\r']).trim_end().to_string()
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

pub fn parse(source: &str, syntax: &Syntax) -> Vec<CommentBlock> {
    let lines: Vec<&str> = source.lines().collect();
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let trimmed = lines[index].trim_start();

        if index == 0 && trimmed.starts_with("#!") {
            index += 1;
            continue;
        }

        if trimmed.starts_with("/*") {
            index = skip_block(&lines, index);
            continue;
        }

        if is_doc_comment(trimmed) {
            index += 1;
            continue;
        }

        let leading = find_marker(trimmed, syntax.line);
        let leading_offset = lines[index].len() - trimmed.len();

        let (marker, marker_start, indent, pre) = match leading {
            Some(marker) => (
                marker,
                leading_offset,
                lines[index][..leading_offset].to_string(),
                String::new(),
            ),
            None => {
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
            }
        };

        let rest = &lines[index][marker_start + marker.len()..];
        if is_directive(rest) {
            index += 1;
            continue;
        }

        let mut contents = vec![clean_content(rest)];
        let mut end = index;
        while pre.is_empty() && end + 1 < lines.len() {
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
            contents,
        });

        index = end + 1;
    }

    blocks
}

fn skip_block(lines: &[&str], start: usize) -> usize {
    let mut index = start;
    while index < lines.len() {
        if lines[index].contains("*/") {
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
        Syntax { line: SLASH }
    }

    fn hash() -> Syntax {
        Syntax { line: HASH }
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
        let source = "/// doc\n//! module\n/**\n * block\n */\n// real\n";
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
    fn known_extensions() {
        assert!(syntax_for(Path::new("a.rs")).is_some());
        assert!(syntax_for(Path::new("a.ts")).is_some());
        assert!(syntax_for(Path::new("a.py")).is_some());
        assert!(syntax_for(Path::new("a.md")).is_none());
        assert!(syntax_for(Path::new("a.bin")).is_none());
    }
}
