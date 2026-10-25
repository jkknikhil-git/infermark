//! Built-in scorers that need no model: `exact_match`, `multiple_choice`, `regex`.
//! (`json_schema`, `code_exec`, `rouge`, `llm_judge` and `needle` arrive with the quality milestone.)

use infermark_sdk::{PackItem, PackManifest, ScoreResult, Scorer};
use std::sync::LazyLock;

use regex::Regex;

use crate::Registry;

pub fn register_builtin(registry: &mut Registry) {
    registry
        .register_scorer(ExactMatch)
        .register_scorer(MultipleChoice)
        .register_scorer(RegexMatch);
}

fn require_expected(item: &PackItem) -> Result<(), String> {
    match &item.expected {
        Some(e) if !e.answers().is_empty() && e.answers().iter().all(|a| !a.trim().is_empty()) => {
            Ok(())
        }
        _ => Err("needs a non-empty `expected` answer".into()),
    }
}

/// Lowercase, collapse whitespace, strip surrounding punctuation and quotes.
pub fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c.is_ascii_punctuation() || c == '“' || c == '”')
        .to_lowercase()
}

/// Answer equals an expected answer after normalisation. With `scorer_options.contains = true`,
/// the answer only has to contain it (useful for chatty models).
pub struct ExactMatch;

impl Scorer for ExactMatch {
    fn id(&self) -> &'static str {
        "exact_match"
    }

    fn validate_item(&self, item: &PackItem) -> Result<(), String> {
        require_expected(item)
    }

    fn score(&self, pack: &PackManifest, item: &PackItem, answer: &str) -> ScoreResult {
        let contains = pack
            .scorer_options
            .get("contains")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let got = normalize(answer);
        let expected = item
            .expected
            .as_ref()
            .map(|e| e.answers())
            .unwrap_or_default();
        let ok = expected.iter().any(|e| {
            let e = normalize(e);
            if contains {
                got.contains(&e)
            } else {
                got == e
            }
        });
        if ok {
            ScoreResult::pass()
        } else {
            ScoreResult::fail(format!("expected {expected:?}"))
        }
    }
}

/// Finds the letter the model picked (`B`, `(B)`, `B) …`, `Answer: B`) and compares it with
/// `expected`.
pub struct MultipleChoice;

impl MultipleChoice {
    pub fn extract_letter(answer: &str, options: usize) -> Option<char> {
        let last = char::from(b'A' + options.clamp(1, 26) as u8 - 1);
        let in_range = |c: char| c.is_ascii_uppercase() && c <= last;
        static ANSWER_IS: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?i)\banswer\s*(?:is)?\s*[:\-]?\s*\(?([A-Z])\b").unwrap()
        });
        // A standalone option letter, e.g. "B", "(B)", "B)" or "B.".
        static STANDALONE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?:^|[\s(\[])([A-Z])(?:$|[\s).\]:,])").unwrap());

        if let Some(c) = ANSWER_IS
            .captures(answer)
            .and_then(|cap| cap[1].chars().next())
            .map(|c| c.to_ascii_uppercase())
            .filter(|c| in_range(*c))
        {
            return Some(c);
        }
        let letter = STANDALONE
            .captures_iter(answer.trim())
            .filter_map(|cap| cap[1].chars().next())
            .find(|c| in_range(*c));
        letter
    }
}

impl Scorer for MultipleChoice {
    fn id(&self) -> &'static str {
        "multiple_choice"
    }

    fn validate_item(&self, item: &PackItem) -> Result<(), String> {
        let choices = item.choices.as_ref().map_or(0, Vec::len);
        if !(2..=26).contains(&choices) {
            return Err("needs between 2 and 26 `choices`".into());
        }
        require_expected(item)?;
        let expected = item.expected.as_ref().unwrap().answers();
        let last = char::from(b'A' + choices as u8 - 1);
        for e in expected {
            let mut chars = e.trim().chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_uppercase() && c <= last => {}
                _ => return Err(format!("`expected` must be a letter A-{last}, got {e:?}")),
            }
        }
        Ok(())
    }

    fn score(&self, _pack: &PackManifest, item: &PackItem, answer: &str) -> ScoreResult {
        let options = item.choices.as_ref().map_or(0, Vec::len);
        let expected = item
            .expected
            .as_ref()
            .map(|e| e.answers())
            .unwrap_or_default();
        match Self::extract_letter(answer, options) {
            Some(c) if expected.iter().any(|e| e.trim().starts_with(c)) => ScoreResult::pass(),
            Some(c) => ScoreResult::fail(format!("picked {c}, expected {expected:?}")),
            None => ScoreResult::fail("no option letter found in the answer"),
        }
    }
}

/// `expected` holds one or more regular expressions; any match passes.
pub struct RegexMatch;

impl Scorer for RegexMatch {
    fn id(&self) -> &'static str {
        "regex"
    }

    fn validate_item(&self, item: &PackItem) -> Result<(), String> {
        require_expected(item)?;
        for pattern in item.expected.as_ref().unwrap().answers() {
            Regex::new(pattern).map_err(|e| format!("invalid regex {pattern:?}: {e}"))?;
        }
        Ok(())
    }

    fn score(&self, _pack: &PackManifest, item: &PackItem, answer: &str) -> ScoreResult {
        let patterns = item
            .expected
            .as_ref()
            .map(|e| e.answers())
            .unwrap_or_default();
        let hit = patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .any(|re| re.is_match(answer));
        if hit {
            ScoreResult::pass()
        } else {
            ScoreResult::fail(format!("no match for {patterns:?}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> PackItem {
        serde_json::from_str(json).unwrap()
    }

    fn pack(options: &str) -> PackManifest {
        toml::from_str(&format!(
            "schema = 1\nid = \"t\"\nname = \"T\"\nversion = \"1\"\ncategory = \"c\"\nscorer = \"x\"\nlicense = \"MIT\"\n[scorer_options]\n{options}"
        ))
        .unwrap()
    }

    #[test]
    fn exact_match_normalizes() {
        let i = item(r#"{"id":"1","input":"Capital of France?","expected":["Paris"]}"#);
        assert!(ExactMatch.score(&pack(""), &i, "  paris. ").passed);
        assert!(!ExactMatch.score(&pack(""), &i, "It is Paris").passed);
        assert!(
            ExactMatch
                .score(&pack("contains = true"), &i, "It is Paris.")
                .passed
        );
        assert!(ExactMatch
            .validate_item(&item(r#"{"id":"2","input":"x"}"#))
            .is_err());
    }

    #[test]
    fn multiple_choice_extraction() {
        let cases = [
            ("B", Some('B')),
            ("(C) because…", Some('C')),
            ("The answer is: D", Some('D')),
            ("answer: a", Some('A')),
            ("I think A) is right", Some('A')),
            ("Neither", None),
            ("E", None), // out of range for 4 options
        ];
        for (text, want) in cases {
            assert_eq!(MultipleChoice::extract_letter(text, 4), want, "{text}");
        }
        let i = item(r#"{"id":"1","input":"q","choices":["w","x","y","z"],"expected":"C"}"#);
        assert!(MultipleChoice.validate_item(&i).is_ok());
        assert!(MultipleChoice.score(&pack(""), &i, "C").passed);
        assert!(!MultipleChoice.score(&pack(""), &i, "B").passed);
        let bad = item(r#"{"id":"1","input":"q","choices":["w","x"],"expected":"C"}"#);
        assert!(MultipleChoice.validate_item(&bad).is_err());
    }

    #[test]
    fn regex_scorer() {
        let i = item(r#"{"id":"1","input":"q","expected":["\\b42\\b"]}"#);
        assert!(RegexMatch.validate_item(&i).is_ok());
        assert!(RegexMatch.score(&pack(""), &i, "The result is 42.").passed);
        assert!(!RegexMatch.score(&pack(""), &i, "420").passed);
        let bad = item(r#"{"id":"1","input":"q","expected":"("}"#);
        assert!(RegexMatch.validate_item(&bad).is_err());
    }
}
