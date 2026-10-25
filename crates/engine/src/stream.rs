//! Shared helpers for streamed generations: timing and reasoning ("thinking") detection.

use std::time::Instant;

use infermark_sdk::TokenEvent;

/// Tracks `<think>…</think>` blocks in raw model output and the first-token timings.
pub(crate) struct StreamTracker {
    start: Instant,
    in_think: bool,
    pub first_token_ms: Option<f64>,
    pub first_visible_ms: Option<f64>,
    pub thinking_pieces: u32,
}

impl StreamTracker {
    pub fn new(start: Instant) -> Self {
        Self {
            start,
            in_think: false,
            first_token_ms: None,
            first_visible_ms: None,
            thinking_pieces: 0,
        }
    }

    /// Record one streamed piece and turn it into a [`TokenEvent`]. `thinking` is set when the
    /// backend already separates reasoning (Ollama); otherwise think tags are detected here.
    pub fn piece(&mut self, text: &str, thinking: Option<bool>) -> Option<TokenEvent> {
        if text.is_empty() {
            return None;
        }
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        self.first_token_ms.get_or_insert(now);
        let is_thinking = match thinking {
            Some(t) => t,
            None => {
                let opens = text.contains("<think>");
                let closes = text.contains("</think>");
                let was = self.in_think;
                if opens {
                    self.in_think = true;
                }
                if closes {
                    self.in_think = false;
                }
                was || opens || closes
            }
        };
        if is_thinking {
            self.thinking_pieces += 1;
        } else if !text.trim().is_empty() {
            self.first_visible_ms.get_or_insert(now);
        }
        Some(TokenEvent {
            text: text.to_string(),
            thinking: is_thinking,
        })
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_think_blocks() {
        let mut t = StreamTracker::new(Instant::now());
        let pieces = ["<think>", "hmm", "</think>", "\n", "Answer", ""];
        let events: Vec<_> = pieces.iter().filter_map(|p| t.piece(p, None)).collect();
        let thinking: Vec<bool> = events.iter().map(|e| e.thinking).collect();
        assert_eq!(thinking, [true, true, true, false, false]);
        assert_eq!(t.thinking_pieces, 3);
        assert!(t.first_token_ms.unwrap() <= t.first_visible_ms.unwrap());
    }

    #[test]
    fn backend_flag_wins() {
        let mut t = StreamTracker::new(Instant::now());
        assert!(t.piece("reasoning", Some(true)).unwrap().thinking);
        assert!(t.first_visible_ms.is_none());
        assert!(!t.piece("answer", Some(false)).unwrap().thinking);
        assert!(t.first_visible_ms.is_some());
    }
}
