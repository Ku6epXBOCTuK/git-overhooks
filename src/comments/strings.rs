use super::parser::Syntax;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StringStyle {
    pub triple: bool,
    pub backtick: bool,
    pub raw: bool,
    pub heredoc: bool,
    pub yaml_block: bool,
    pub lua_long: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Code,
    Raw { hashes: usize },
    Triple { quote: char },
    Backtick,
    LuaLong { level: usize },
    Heredoc { tag: String },
    YamlBlock { indent: usize },
    BlockComment { close: &'static str },
}

fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn raw_start(chars: &[char], index: usize) -> Option<(usize, usize)> {
    if chars[index] != 'r' {
        return None;
    }
    if index > 0 && is_ident(chars[index - 1]) {
        return None;
    }

    let mut hashes = 0;
    let mut cursor = index + 1;

    while cursor < chars.len() && chars[cursor] == '#' {
        hashes += 1;
        cursor += 1;
    }

    if cursor >= chars.len() || chars[cursor] != '"' {
        return None;
    }

    Some((hashes, cursor - index + 1))
}

fn lua_long_start(chars: &[char], index: usize) -> Option<usize> {
    if chars[index] != '[' {
        return None;
    }

    let mut level = 0;
    let mut cursor = index + 1;

    while cursor < chars.len() && chars[cursor] == '=' {
        level += 1;
        cursor += 1;
    }

    if cursor >= chars.len() || chars[cursor] != '[' {
        return None;
    }

    Some(level)
}

fn heredoc_start(chars: &[char], index: usize) -> Option<(String, usize)> {
    if chars.get(index) != Some(&'<') || chars.get(index + 1) != Some(&'<') {
        return None;
    }

    let mut cursor = index + 2;

    if chars.get(cursor) == Some(&'<') {
        cursor += 1;
    }

    if matches!(chars.get(cursor), Some('-') | Some('~')) {
        cursor += 1;
    }

    if matches!(chars.get(cursor), Some('\'') | Some('"')) {
        cursor += 1;
    }

    let start = cursor;
    while cursor < chars.len() && is_ident(chars[cursor]) {
        cursor += 1;
    }

    if cursor == start {
        return None;
    }

    let tag: String = chars[start..cursor].iter().collect();

    if matches!(chars.get(cursor), Some('\'') | Some('"')) {
        cursor += 1;
    }

    Some((tag, cursor - index))
}

fn yaml_block_indicator(chars: &[char]) -> Option<usize> {
    let colon = chars.iter().rposition(|c| *c == ':')?;
    let rest: Vec<char> = chars[colon + 1..]
        .iter()
        .copied()
        .filter(|c| !c.is_whitespace())
        .collect();

    let first = *rest.first()?;
    if first != '|' && first != '>' {
        return None;
    }

    if !rest[1..]
        .iter()
        .all(|c| c.is_ascii_digit() || *c == '+' || *c == '-')
    {
        return None;
    }

    let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
    Some(indent)
}

fn line_string_end(chars: &[char], index: usize, quote: char) -> Option<usize> {
    let mut cursor = index + 1;

    while cursor < chars.len() {
        if chars[cursor] == '\\' {
            cursor += 2;
            continue;
        }
        if chars[cursor] == quote {
            return Some(cursor + 1);
        }
        cursor += 1;
    }

    None
}

fn comment_marker(chars: &[char], index: usize, syntax: &Syntax) -> bool {
    for marker in syntax.line {
        let marker_chars: Vec<char> = marker.chars().collect();
        if chars.len() < index + marker_chars.len() {
            continue;
        }
        if chars[index..index + marker_chars.len()] != marker_chars[..] {
            continue;
        }
        if *marker == "//" {
            return true;
        }
        if index == 0 || chars[index - 1].is_whitespace() {
            return true;
        }
    }

    false
}

fn scan_code(state: &mut State, chars: &[char], syntax: &Syntax) {
    let mut index = 0;

    while index < chars.len() {
        match state {
            State::Code => {
                if syntax.strings.backtick && chars[index] == '`' {
                    *state = State::Backtick;
                    index += 1;
                    continue;
                }
                if syntax.strings.triple && chars[index..].starts_with(&['"', '"', '"']) {
                    *state = State::Triple { quote: '"' };
                    index += 3;
                    continue;
                }
                if syntax.strings.triple && chars[index..].starts_with(&['\'', '\'', '\'']) {
                    *state = State::Triple { quote: '\'' };
                    index += 3;
                    continue;
                }
                if syntax.strings.raw
                    && let Some((hashes, len)) = raw_start(chars, index)
                {
                    *state = State::Raw { hashes };
                    index += len;
                    continue;
                }
                if syntax.strings.lua_long
                    && let Some(level) = lua_long_start(chars, index)
                {
                    *state = State::LuaLong { level };
                    index += 2 + level;
                    continue;
                }
                if syntax.strings.heredoc
                    && let Some((tag, len)) = heredoc_start(chars, index)
                {
                    *state = State::Heredoc { tag };
                    index += len;
                    continue;
                }
                if syntax.block && chars[index..].starts_with(&['/', '*']) {
                    *state = State::BlockComment { close: "*/" };
                    index += 2;
                    continue;
                }
                if syntax.html && chars[index..].starts_with(&['<', '!', '-', '-']) {
                    *state = State::BlockComment { close: "-->" };
                    index += 4;
                    continue;
                }
                if chars[index] == '"' || chars[index] == '\'' {
                    let quote = chars[index];
                    match line_string_end(chars, index, quote) {
                        Some(end) => {
                            index = end;
                            continue;
                        }
                        None => return,
                    }
                }
                if comment_marker(chars, index, syntax) {
                    return;
                }
                index += 1;
            }
            State::Raw { hashes } => {
                if chars[index] == '"' {
                    let mut cursor = index + 1;
                    let mut found = true;
                    for _ in 0..*hashes {
                        if chars.get(cursor) != Some(&'#') {
                            found = false;
                            break;
                        }
                        cursor += 1;
                    }
                    if found {
                        *state = State::Code;
                        index = cursor;
                        continue;
                    }
                }
                index += 1;
            }
            State::Triple { quote } => {
                let marker = [*quote, *quote, *quote];
                if chars[index..].starts_with(&marker) {
                    *state = State::Code;
                    index += 3;
                    continue;
                }
                index += 1;
            }
            State::Backtick => {
                if chars[index] == '\\' {
                    index += 2;
                    continue;
                }
                if chars[index] == '`' {
                    *state = State::Code;
                    index += 1;
                    continue;
                }
                index += 1;
            }
            State::LuaLong { level } => {
                let mut marker = vec![']'];
                marker.extend(std::iter::repeat_n('=', *level));
                marker.push(']');
                if chars[index..].starts_with(&marker) {
                    *state = State::Code;
                    index += marker.len();
                    continue;
                }
                index += 1;
            }
            State::BlockComment { close } => {
                let close_chars: Vec<char> = close.chars().collect();
                if chars[index..].starts_with(&close_chars) {
                    *state = State::Code;
                    index += close_chars.len();
                    continue;
                }
                index += 1;
            }
            State::Heredoc { .. } | State::YamlBlock { .. } => return,
        }
    }
}

fn step(state: State, line: &str, syntax: &Syntax) -> State {
    match state {
        State::Heredoc { tag } => {
            if line.trim() == tag {
                return State::Code;
            }
            return State::Heredoc { tag };
        }
        State::YamlBlock { indent } => {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                return State::YamlBlock { indent };
            }
            let current = line.len() - trimmed.len();
            if current > indent {
                return State::YamlBlock { indent };
            }
            return step(State::Code, line, syntax);
        }
        _ => {}
    }

    let chars: Vec<char> = line.chars().collect();
    let mut state = state;

    if syntax.strings.yaml_block && matches!(state, State::Code) {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#')
            && let Some(indent) = yaml_block_indicator(&chars)
        {
            return State::YamlBlock { indent };
        }
    }

    scan_code(&mut state, &chars, syntax);
    state
}

pub fn skip_lines(source: &str, syntax: &Syntax) -> Vec<bool> {
    let lines: Vec<&str> = source.lines().collect();
    let mut skip = vec![false; lines.len()];
    let mut state = State::Code;

    for (index, line) in lines.iter().enumerate() {
        if state != State::Code {
            skip[index] = true;
        }
        state = step(state, line, syntax);
    }

    skip
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syntax(style: StringStyle) -> Syntax {
        Syntax {
            line: &["//"],
            block: true,
            html: false,
            jsdoc: true,
            strings: style,
        }
    }

    fn raw() -> StringStyle {
        StringStyle {
            raw: true,
            ..StringStyle::default()
        }
    }

    #[test]
    fn code_lines_are_not_skipped() {
        let source = "let x = 1;\n// real comment\n";
        let skip = skip_lines(source, &syntax(StringStyle::default()));
        assert_eq!(skip, vec![false, false]);
    }

    #[test]
    fn raw_string_marks_inner_lines() {
        let source = "let a = r#\"\n    // text\n\"#;\n";
        let skip = skip_lines(source, &syntax(raw()));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn raw_string_ends_after_the_closing_hashes() {
        let source = "let a = r#\"x\"#;\n// real comment\n";
        let skip = skip_lines(source, &syntax(raw()));
        assert_eq!(skip, vec![false, false]);
    }

    #[test]
    fn unterminated_raw_string_runs_to_the_end_of_file() {
        let source = "let a = r#\"\n// text\n// more\n";
        let skip = skip_lines(source, &syntax(raw()));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn triple_quoted_string_marks_inner_lines() {
        let source = "x = \"\"\"\n# not a comment\n\"\"\"\n";
        let style = StringStyle {
            triple: true,
            ..StringStyle::default()
        };
        let skip = skip_lines(source, &syntax(style));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn backtick_template_marks_inner_lines() {
        let source = "const s = `\n// text\n`;\n";
        let style = StringStyle {
            backtick: true,
            ..StringStyle::default()
        };
        let skip = skip_lines(source, &syntax(style));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn heredoc_marks_inner_lines() {
        let source = "cat <<EOF\n# text\nEOF\n";
        let style = StringStyle {
            heredoc: true,
            ..StringStyle::default()
        };
        let skip = skip_lines(source, &syntax(style));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn yaml_block_scalar_marks_inner_lines() {
        let source = "key: |\n  # text\n";
        let style = StringStyle {
            yaml_block: true,
            ..StringStyle::default()
        };
        let skip = skip_lines(source, &syntax(style));
        assert_eq!(skip, vec![false, true]);
    }

    #[test]
    fn single_line_string_does_not_skip_the_next_line() {
        let source = "let s = \"a // b\";\n// real comment\n";
        let skip = skip_lines(source, &syntax(StringStyle::default()));
        assert_eq!(skip, vec![false, false]);
    }

    #[test]
    fn apostrophe_in_code_does_not_swallow_the_file() {
        let source = "let s = &'a str;\n// real comment\n";
        let skip = skip_lines(source, &syntax(StringStyle::default()));
        assert_eq!(skip, vec![false, false]);
    }

    #[test]
    fn comment_text_is_not_scanned_as_a_string() {
        let source = "// it's a comment\nlet x = 1;\n";
        let skip = skip_lines(source, &syntax(StringStyle::default()));
        assert_eq!(skip, vec![false, false]);
    }
}
