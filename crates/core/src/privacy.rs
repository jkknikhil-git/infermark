//! Keeps personal details out of result files.
//!
//! Engine error messages and logs often contain absolute paths, and those contain the user's
//! account name (`C:\Users\alice\...`). Every note and error a module produces passes through
//! [`redact`] before it is stored.

/// Replace the home folder (and the configured storage folder) in `text` with `~` / `<storage>`.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    let storage = crate::settings::load_or_default().storage_dir();
    let mut prefixes: Vec<(String, &str)> = Vec::new();
    for (dir, replacement) in [(Some(storage), "<storage>"), (dirs::home_dir(), "~")] {
        let Some(dir) = dir else { continue };
        let s = dir
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string();
        if s.len() < 3 {
            continue;
        }
        prefixes.push((s.replace('\\', "/"), replacement));
        prefixes.push((s.replace('/', "\\"), replacement));
        // JSON-escaped Windows paths (`C:\\Users\\...`) also show up in engine output.
        prefixes.push((s.replace('/', "\\").replace('\\', "\\\\"), replacement));
    }
    // Longest first, so the storage folder wins over the home folder that contains it.
    prefixes.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    for (prefix, replacement) in prefixes {
        out = replace_case_insensitive(&out, &prefix, replacement);
    }
    out
}

/// Windows paths are case-insensitive, so match them that way.
fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower_h = haystack.to_lowercase();
    let lower_n = needle.to_lowercase();
    // Lowercasing can change byte lengths for some scripts; fall back to exact matching then.
    if lower_h.len() != haystack.len() || lower_n.len() != needle.len() {
        return haystack.replace(needle, replacement);
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while let Some(pos) = lower_h[i..].find(&lower_n) {
        out.push_str(&haystack[i..i + pos]);
        out.push_str(replacement);
        i += pos + needle.len();
    }
    out.push_str(&haystack[i..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_folder_is_removed() {
        let Some(home) = dirs::home_dir() else { return };
        let h = home.to_string_lossy().to_string();
        let text = format!(
            "loading model from '{h}{}x.gguf' failed",
            std::path::MAIN_SEPARATOR
        );
        let red = redact(&text);
        assert!(!red.contains(&h), "{red}");
        assert!(red.contains("x.gguf"));
        let upper = redact(&text.to_uppercase());
        assert!(!upper.contains(&h.to_uppercase()), "{upper}");
    }

    #[test]
    fn unrelated_text_is_unchanged() {
        assert_eq!(redact("Qwen3 0.6B: 42 tok/s"), "Qwen3 0.6B: 42 tok/s");
    }

    #[test]
    fn case_insensitive_replace() {
        assert_eq!(
            replace_case_insensitive("A C:\\Users\\Bob\\x", "c:\\users\\bob", "~"),
            "A ~\\x"
        );
    }
}
