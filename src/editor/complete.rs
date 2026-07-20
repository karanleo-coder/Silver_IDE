use std::collections::BTreeSet;

use super::highlight::keywords_for;

/// The suggestion popup: a few words that finish what's being typed.
/// Built from the language's keywords plus the words already in the
/// file — no index, no language server, nothing to install.
pub struct Completion {
    pub items: Vec<String>,
    pub selected: usize,
    /// What the user has typed so far; accepting inserts the rest.
    pub prefix: String,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The partial word just left of the cursor (`cx` in characters).
pub fn word_prefix(line: &str, cx: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let end = cx.min(chars.len());
    let mut start = end;
    while start > 0 && is_ident(chars[start - 1]) {
        start -= 1;
    }
    chars[start..end].iter().collect()
}

/// Up to 8 words that start with `prefix`: keywords first, then
/// identifiers harvested from the file itself, shortest first.
pub fn suggestions(lines: &[String], ext: &str, prefix: &str) -> Vec<String> {
    let first = prefix.chars().next();
    if !matches!(first, Some(c) if c.is_alphabetic() || c == '_') {
        return Vec::new();
    }

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut items: Vec<String> = Vec::new();
    for kw in keywords_for(ext) {
        if kw.starts_with(prefix) && *kw != prefix && seen.insert(kw) {
            items.push(kw.to_string());
        }
    }

    // Words from the buffer. Scanning a few thousand hand-written
    // lines is far cheaper than a frame, so no caching is needed.
    let mut words: BTreeSet<String> = BTreeSet::new();
    for line in lines.iter().take(4000) {
        let mut word = String::new();
        for c in line.chars().chain(std::iter::once(' ')) {
            if is_ident(c) {
                word.push(c);
            } else if !word.is_empty() {
                let w = std::mem::take(&mut word);
                if w.chars().count() >= 3
                    && w != prefix
                    && w.starts_with(prefix)
                    && !w.chars().next().unwrap_or('0').is_ascii_digit()
                    && !seen.contains(w.as_str())
                {
                    words.insert(w);
                }
            }
        }
    }
    let mut from_file: Vec<String> = words.into_iter().collect();
    from_file.sort_by(|a, b| a.chars().count().cmp(&b.chars().count()).then_with(|| a.cmp(b)));
    items.extend(from_file);
    items.truncate(8);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_the_word_left_of_the_cursor() {
        assert_eq!(word_prefix("let cou", 7), "cou");
        assert_eq!(word_prefix("let cou", 3), "let");
        assert_eq!(word_prefix("x + y", 3), "");
    }

    #[test]
    fn suggests_keywords_and_file_words() {
        let lines = vec!["let counter = 0;".to_string(), "counting(counter);".to_string()];
        let s = suggestions(&lines, "rs", "co");
        assert!(s.contains(&"const".to_string())); // keyword
        assert!(s.contains(&"counter".to_string())); // from the file
        assert!(s.contains(&"counting".to_string()));
    }

    #[test]
    fn exact_word_is_not_suggested_for_itself() {
        let lines = vec!["counter".to_string()];
        assert!(!suggestions(&lines, "rs", "counter").contains(&"counter".to_string()));
    }

    #[test]
    fn nothing_for_numeric_prefixes() {
        let lines = vec!["a123456".to_string()];
        assert!(suggestions(&lines, "rs", "12").is_empty());
    }
}
