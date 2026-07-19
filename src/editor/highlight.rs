use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Lightweight single-line syntax highlighting. No external grammar
/// engine — keeps the binary small and startup instant. Multi-line
/// strings/comments are not tracked; good enough for a v1 editor.
pub fn highlight_line(line: &str, ext: &str) -> Line<'static> {
    let keywords = keywords_for(ext);
    let comment = comment_prefix(ext);

    let fg_text = Color::Rgb(220, 220, 220);
    let st_keyword = Style::new().fg(Color::Rgb(198, 120, 221)).add_modifier(Modifier::BOLD);
    let st_string = Style::new().fg(Color::Rgb(152, 195, 121));
    let st_number = Style::new().fg(Color::Rgb(229, 192, 123));
    let st_comment = Style::new().fg(Color::Rgb(92, 99, 112)).add_modifier(Modifier::ITALIC);
    let st_punct = Style::new().fg(Color::Rgb(140, 148, 160));
    let st_text = Style::new().fg(fg_text);

    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Line comment: everything to the end of the line.
        if let Some(prefix) = comment {
            if chars[i..].starts_with(&prefix.chars().collect::<Vec<_>>()[..]) {
                let rest: String = chars[i..].iter().collect();
                spans.push(Span::styled(rest, st_comment));
                break;
            }
        }
        let c = chars[i];
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let s: String = chars[start..i.min(chars.len())].iter().collect();
            spans.push(Span::styled(s, st_string));
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            spans.push(Span::styled(s, st_number));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if keywords.contains(&word.as_str()) {
                spans.push(Span::styled(word, st_keyword));
            } else {
                spans.push(Span::styled(word, st_text));
            }
        } else if c.is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            spans.push(Span::raw(s));
        } else {
            let start = i;
            while i < chars.len()
                && !chars[i].is_alphanumeric()
                && !chars[i].is_whitespace()
                && chars[i] != '"'
                && chars[i] != '\''
                && chars[i] != '`'
                && chars[i] != '_'
            {
                // Stop before a comment prefix so it gets its own span.
                if let Some(prefix) = comment {
                    if chars[i..].starts_with(&prefix.chars().collect::<Vec<_>>()[..]) && i > start {
                        break;
                    }
                }
                i += 1;
            }
            let s: String = chars[start..i.max(start + 1).min(chars.len())].iter().collect();
            if i == start {
                i += 1;
            }
            spans.push(Span::styled(s, st_punct));
        }
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    Line::from(spans)
}

fn comment_prefix(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" | "js" | "jsx" | "ts" | "tsx" | "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "go"
        | "swift" | "kt" | "scala" | "cs" | "json5" | "zig" | "dart" => Some("//"),
        "py" | "rb" | "sh" | "bash" | "zsh" | "toml" | "yaml" | "yml" | "conf" | "ini"
        | "dockerfile" | "makefile" | "r" | "pl" => Some("#"),
        "sql" | "lua" | "hs" => Some("--"),
        _ => None,
    }
}

fn keywords_for(ext: &str) -> &'static [&'static str] {
    match ext {
        "rs" => &[
            "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait", "for",
            "while", "loop", "if", "else", "match", "return", "self", "Self", "crate", "super",
            "const", "static", "ref", "move", "async", "await", "dyn", "where", "type", "as",
            "in", "break", "continue", "unsafe", "true", "false", "Some", "None", "Ok", "Err",
        ],
        "js" | "jsx" | "ts" | "tsx" => &[
            "function", "const", "let", "var", "return", "if", "else", "for", "while", "class",
            "extends", "import", "export", "from", "default", "new", "this", "async", "await",
            "try", "catch", "finally", "throw", "typeof", "instanceof", "switch", "case",
            "break", "continue", "true", "false", "null", "undefined", "interface", "type",
            "enum", "implements", "readonly", "static", "of", "in", "yield", "delete", "void",
        ],
        "py" => &[
            "def", "class", "return", "if", "elif", "else", "for", "while", "import", "from",
            "as", "with", "try", "except", "finally", "raise", "pass", "break", "continue",
            "lambda", "yield", "global", "nonlocal", "assert", "del", "in", "is", "not", "and",
            "or", "True", "False", "None", "async", "await", "match", "case", "self",
        ],
        "go" => &[
            "func", "package", "import", "var", "const", "type", "struct", "interface", "map",
            "chan", "go", "defer", "if", "else", "for", "range", "switch", "case", "default",
            "return", "break", "continue", "select", "fallthrough", "goto", "true", "false",
            "nil", "make", "new", "len", "cap", "append",
        ],
        "c" | "h" | "cpp" | "hpp" | "cc" => &[
            "int", "char", "float", "double", "void", "long", "short", "unsigned", "signed",
            "struct", "union", "enum", "typedef", "const", "static", "extern", "if", "else",
            "for", "while", "do", "switch", "case", "default", "return", "break", "continue",
            "sizeof", "class", "public", "private", "protected", "virtual", "template",
            "namespace", "using", "new", "delete", "nullptr", "true", "false", "auto",
        ],
        "java" | "kt" => &[
            "public", "private", "protected", "class", "interface", "extends", "implements",
            "static", "final", "void", "int", "long", "float", "double", "boolean", "char",
            "new", "return", "if", "else", "for", "while", "do", "switch", "case", "break",
            "continue", "try", "catch", "finally", "throw", "throws", "import", "package",
            "this", "super", "true", "false", "null", "fun", "val", "var", "when", "object",
        ],
        "sh" | "bash" | "zsh" => &[
            "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac",
            "function", "return", "local", "export", "echo", "exit", "in", "read", "shift",
        ],
        "toml" | "yaml" | "yml" | "json" => &["true", "false", "null"],
        "html" | "xml" | "md" => &[],
        _ => &[
            "fn", "function", "def", "class", "return", "if", "else", "for", "while", "import",
            "let", "const", "var", "true", "false", "null",
        ],
    }
}
