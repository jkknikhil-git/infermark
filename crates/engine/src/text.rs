//! Text handling for the in-process llama.cpp backend: turning token bytes into streamed text,
//! stop strings, and keeping the tail of llama.cpp's log for error messages.
//!
//! `llama-server` does all of this itself; in-process we have to do it here.

use std::collections::VecDeque;

/// Joins token bytes into valid UTF-8. A token can end in the middle of a character (emoji,
/// CJK), so incomplete bytes wait for the next token instead of becoming `�`.
#[derive(Default)]
pub(crate) struct Utf8Pieces {
    pending: Vec<u8>,
}

impl Utf8Pieces {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    return out;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pending[..valid]).expect("valid"));
                    match e.error_len() {
                        // Incomplete character at the end: keep it for the next token.
                        None => {
                            self.pending.drain(..valid);
                            return out;
                        }
                        Some(bad) => {
                            out.push(char::REPLACEMENT_CHARACTER);
                            self.pending.drain(..valid + bad);
                        }
                    }
                }
            }
        }
    }

    /// Whatever is left when generation ends.
    pub fn finish(&mut self) -> String {
        let rest = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        rest
    }
}

/// Ends generation at the first stop string, like `llama-server`: text that could be the start
/// of a stop string is held back until it is clear, and the stop string itself is never shown.
pub(crate) struct StopFilter {
    stops: Vec<String>,
    held: String,
}

impl StopFilter {
    pub fn new(stops: &[String]) -> Self {
        Self {
            stops: stops.iter().filter(|s| !s.is_empty()).cloned().collect(),
            held: String::new(),
        }
    }

    /// Add generated text; returns the text that is safe to show and whether a stop string
    /// was reached.
    pub fn push(&mut self, text: &str) -> (String, bool) {
        self.held.push_str(text);
        if let Some(at) = self.stops.iter().filter_map(|s| self.held.find(s)).min() {
            self.held.truncate(at);
            return (std::mem::take(&mut self.held), true);
        }
        // Longest end of the held text that is the start of some stop string.
        let keep = self
            .stops
            .iter()
            .filter_map(|s| {
                (1..s.len())
                    .rev()
                    .find(|&k| s.is_char_boundary(k) && self.held.ends_with(&s[..k]))
            })
            .max()
            .unwrap_or(0);
        let rest = self.held.split_off(self.held.len() - keep);
        (std::mem::replace(&mut self.held, rest), false)
    }

    /// Text held back when generation ends without a stop string.
    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.held)
    }
}

const LOG_LINES: usize = 40;

/// ggml log levels (`ggml_log_level`).
const LEVEL_WARN: i64 = 3;
const LEVEL_CONT: i64 = 5;

/// The last warning and error lines llama.cpp logged, for error messages (the counterpart of
/// `llama-server`'s stderr tail). Info and debug output is dropped.
pub(crate) struct LogTail {
    lines: VecDeque<String>,
    partial: String,
    keeping: bool,
}

impl LogTail {
    pub const fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            partial: String::new(),
            keeping: false,
        }
    }

    /// One log callback; messages may arrive in pieces (`GGML_LOG_LEVEL_CONT`).
    pub fn push(&mut self, level: i64, text: &str) {
        if level != LEVEL_CONT {
            self.keeping = level >= LEVEL_WARN;
        }
        if !self.keeping {
            return;
        }
        self.partial.push_str(text);
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            let line = line.trim_end();
            if !line.is_empty() {
                if self.lines.len() == LOG_LINES {
                    self.lines.pop_front();
                }
                self.lines.push_back(line.to_string());
            }
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.partial.clear();
    }

    /// The last few lines, oldest first.
    pub fn tail(&self, n: usize) -> String {
        let skip = self.lines.len().saturating_sub(n);
        self.lines
            .iter()
            .skip(skip)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_characters_wait_for_the_next_token() {
        let mut p = Utf8Pieces::default();
        let smile = "😀".as_bytes();
        assert_eq!(p.push(b"hi "), "hi ");
        assert_eq!(p.push(&smile[..2]), "");
        assert_eq!(p.push(&smile[2..]), "😀");
        assert_eq!(p.push(&[0xff, b'a']), "\u{fffd}a");
        assert_eq!(p.push(&smile[..1]), "");
        assert_eq!(p.finish(), "\u{fffd}");
        assert_eq!(p.finish(), "");
    }

    #[test]
    fn stop_strings_are_held_back_and_hidden() {
        let mut f = StopFilter::new(&["</s>".into(), "\n\n".into(), String::new()]);
        assert_eq!(f.push("Hello"), ("Hello".into(), false));
        assert_eq!(f.push(" world<"), (" world".into(), false));
        assert_eq!(f.push("b>"), ("<b>".into(), false));
        assert_eq!(f.push("\n"), ("".into(), false));
        assert_eq!(f.push("x"), ("\nx".into(), false));
        assert_eq!(f.push("end</"), ("end".into(), false));
        assert_eq!(f.push("s>more"), ("".into(), true));
    }

    #[test]
    fn stop_inside_one_piece() {
        let mut f = StopFilter::new(&[".".into()]);
        assert_eq!(f.push("Once. Upon"), ("Once".into(), true));
        let mut none = StopFilter::new(&[]);
        assert_eq!(none.push("a."), ("a.".into(), false));
        assert_eq!(none.finish(), "");
        let mut held = StopFilter::new(&["ab".into()]);
        assert_eq!(held.push("xa"), ("x".into(), false));
        assert_eq!(held.finish(), "a");
    }

    #[test]
    fn log_keeps_warnings_and_errors() {
        let mut log = LogTail::new();
        log.push(2, "loading model\n");
        log.push(4, "error loading model: ");
        log.push(LEVEL_CONT, "file not found\n");
        log.push(2, "info\n");
        log.push(LEVEL_CONT, "more info\n");
        log.push(3, "warn\n");
        assert_eq!(log.tail(5), "error loading model: file not found\nwarn");
        assert_eq!(log.tail(1), "warn");
        log.clear();
        assert_eq!(log.tail(5), "");
    }
}
