use super::parser::Syntax;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StringStyle {
    #[default]
    Plain,
    Raw,
    Triple,
    Backtick,
    Heredoc,
    YamlBlock,
    LuaLong,
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

    if matches!(chars.get(cursor), Some('-' | '~')) {
        cursor += 1;
    }

    if matches!(chars.get(cursor), Some('\'' | '"')) {
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

    if matches!(chars.get(cursor), Some('\'' | '"')) {
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

fn string_start(chars: &[char], index: usize, style: StringStyle) -> Option<(State, usize)> {
    match style {
        StringStyle::Raw => {
            raw_start(chars, index).map(|(hashes, len)| (State::Raw { hashes }, index + len))
        }
        StringStyle::Triple => {
            if chars[index..].starts_with(&['"', '"', '"']) {
                Some((State::Triple { quote: '"' }, index + 3))
            } else if chars[index..].starts_with(&['\'', '\'', '\'']) {
                Some((State::Triple { quote: '\'' }, index + 3))
            } else {
                None
            }
        }
        StringStyle::Backtick => {
            if chars[index] == '`' {
                Some((State::Backtick, index + 1))
            } else {
                None
            }
        }
        StringStyle::Heredoc => {
            heredoc_start(chars, index).map(|(tag, len)| (State::Heredoc { tag }, index + len))
        }
        StringStyle::LuaLong => {
            lua_long_start(chars, index).map(|level| (State::LuaLong { level }, index + 2 + level))
        }
        StringStyle::Plain | StringStyle::YamlBlock => None,
    }
}

fn code_step(state: &mut State, chars: &[char], index: usize, syntax: &Syntax) -> Option<usize> {
    if let Some((next_state, next_index)) = string_start(chars, index, syntax.strings) {
        *state = next_state;
        return Some(next_index);
    }
    if syntax.block && chars[index..].starts_with(&['/', '*']) {
        *state = State::BlockComment { close: "*/" };
        return Some(index + 2);
    }
    if syntax.html && chars[index..].starts_with(&['<', '!', '-', '-']) {
        *state = State::BlockComment { close: "-->" };
        return Some(index + 4);
    }
    if chars[index] == '"' || chars[index] == '\'' {
        return line_string_end(chars, index, chars[index]);
    }
    if comment_marker(chars, index, syntax) {
        return None;
    }
    Some(index + 1)
}

fn raw_step(state: &mut State, chars: &[char], index: usize, hashes: usize) -> usize {
    if chars[index] == '"' {
        let mut cursor = index + 1;
        let mut found = true;
        for _ in 0..hashes {
            if chars.get(cursor) != Some(&'#') {
                found = false;
                break;
            }
            cursor += 1;
        }
        if found {
            *state = State::Code;
            return cursor;
        }
    }
    index + 1
}

fn triple_step(state: &mut State, chars: &[char], index: usize, quote: char) -> usize {
    let marker = [quote, quote, quote];
    if chars[index..].starts_with(&marker) {
        *state = State::Code;
        return index + 3;
    }
    index + 1
}

fn backtick_step(state: &mut State, chars: &[char], index: usize) -> usize {
    if chars[index] == '\\' {
        return index + 2;
    }
    if chars[index] == '`' {
        *state = State::Code;
        return index + 1;
    }
    index + 1
}

fn lua_step(state: &mut State, chars: &[char], index: usize, level: usize) -> usize {
    let mut marker = vec![']'];
    marker.extend(std::iter::repeat_n('=', level));
    marker.push(']');
    if chars[index..].starts_with(&marker) {
        *state = State::Code;
        return index + marker.len();
    }
    index + 1
}

fn block_step(state: &mut State, chars: &[char], index: usize, close: &str) -> usize {
    let close_chars: Vec<char> = close.chars().collect();
    if chars[index..].starts_with(&close_chars) {
        *state = State::Code;
        return index + close_chars.len();
    }
    index + 1
}

fn scan_code(state: &mut State, chars: &[char], syntax: &Syntax) {
    let mut index = 0;

    while index < chars.len() {
        let current = state.clone();
        let next = match &current {
            State::Code => code_step(state, chars, index, syntax),
            State::Raw { hashes } => Some(raw_step(state, chars, index, *hashes)),
            State::Triple { quote } => Some(triple_step(state, chars, index, *quote)),
            State::Backtick => Some(backtick_step(state, chars, index)),
            State::LuaLong { level } => Some(lua_step(state, chars, index, *level)),
            State::BlockComment { close } => Some(block_step(state, chars, index, close)),
            State::Heredoc { .. } | State::YamlBlock { .. } => return,
        };

        match next {
            Some(next_index) => index = next_index,
            None => return,
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

    if syntax.strings == StringStyle::YamlBlock && matches!(state, State::Code) {
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

    #[test]
    fn code_lines_are_not_skipped() {
        let source = "let x = 1;\n// real comment\n";
        let skip = skip_lines(source, &syntax(StringStyle::default()));
        assert_eq!(skip, vec![false, false]);
    }

    #[test]
    fn raw_string_marks_inner_lines() {
        let source = "let a = r#\"\n    // text\n\"#;\n";
        let skip = skip_lines(source, &syntax(StringStyle::Raw));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn raw_string_ends_after_the_closing_hashes() {
        let source = "let a = r#\"x\"#;\n// real comment\n";
        let skip = skip_lines(source, &syntax(StringStyle::Raw));
        assert_eq!(skip, vec![false, false]);
    }

    #[test]
    fn unterminated_raw_string_runs_to_the_end_of_file() {
        let source = "let a = r#\"\n// text\n// more\n";
        let skip = skip_lines(source, &syntax(StringStyle::Raw));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn triple_quoted_string_marks_inner_lines() {
        let source = "x = \"\"\"\n# not a comment\n\"\"\"\n";
        let skip = skip_lines(source, &syntax(StringStyle::Triple));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn backtick_template_marks_inner_lines() {
        let source = "const s = `\n// text\n`;\n";
        let skip = skip_lines(source, &syntax(StringStyle::Backtick));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn heredoc_marks_inner_lines() {
        let source = "cat <<EOF\n# text\nEOF\n";
        let skip = skip_lines(source, &syntax(StringStyle::Heredoc));
        assert_eq!(skip, vec![false, true, true]);
    }

    #[test]
    fn yaml_block_scalar_marks_inner_lines() {
        let source = "key: |\n  # text\n";
        let skip = skip_lines(source, &syntax(StringStyle::YamlBlock));
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
