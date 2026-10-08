use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const MAX_RULES: usize = 100;
const MAX_TERM_CHARS: usize = 200;
const MAX_REPLACED_TEXT_BYTES: usize = 16 * 1024 * 1024;
const RUSSIAN_FORMAL_RULES: &str = include_str!("../../src/lib/text-replacement-presets.json");
static WORD_CHARACTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?u)^\w$").expect("word character regex must compile"));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TextReplacementRule {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TextReplacementSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<TextReplacementRule>,
}

impl TextReplacementSettings {
    pub fn russian_formal_rules() -> Vec<TextReplacementRule> {
        serde_json::from_str(RUSSIAN_FORMAL_RULES)
            .expect("the bundled Russian formal address preset must be valid")
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.rules.len() > MAX_RULES {
            return Err("text replacement rules must not exceed 100 entries");
        }
        if self.rules.iter().any(|rule| {
            rule.from.chars().count() > MAX_TERM_CHARS
                || rule.to.chars().count() > MAX_TERM_CHARS
                || rule.from.chars().any(char::is_control)
                || rule.to.chars().any(char::is_control)
        }) {
            return Err("text replacement terms must be at most 200 characters and contain no control characters");
        }
        Ok(())
    }
}

pub fn apply_text_replacements(mut text: String, settings: &TextReplacementSettings) -> String {
    if !settings.enabled {
        return text;
    }

    if text.len() > MAX_REPLACED_TEXT_BYTES {
        tracing::warn!("text replacement skipped because the transcript exceeds the size limit");
        return text;
    }

    for rule in settings
        .rules
        .iter()
        .filter(|rule| !rule.from.trim().is_empty())
    {
        match replace_rule(&text, rule) {
            RuleApplication::Unchanged => {}
            RuleApplication::Replaced(next) => text = next,
            RuleApplication::OutputLimitExceeded => {
                tracing::warn!("text replacement rule skipped because it exceeds the size limit");
            }
        }
    }
    text
}

enum RuleApplication {
    Unchanged,
    Replaced(String),
    OutputLimitExceeded,
}

fn replace_rule(text: &str, rule: &TextReplacementRule) -> RuleApplication {
    let pattern = regex::escape(&rule.from);
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(!rule.case_sensitive)
        .build()
        .expect("escaped text replacement pattern must compile");

    let mut projected_len = text.len();
    let mut found_match = false;
    let mut search_at = 0;
    while let Some(matched) = next_word_match(&regex, text, search_at) {
        found_match = true;
        let Some(next_len) = projected_len
            .checked_sub(matched.len())
            .and_then(|len| len.checked_add(rule.to.len()))
        else {
            return RuleApplication::OutputLimitExceeded;
        };
        if next_len > MAX_REPLACED_TEXT_BYTES {
            return RuleApplication::OutputLimitExceeded;
        }
        projected_len = next_len;
        search_at = matched.end();
    }
    if !found_match {
        return RuleApplication::Unchanged;
    }

    let mut result = String::with_capacity(projected_len);
    let mut copied_until = 0;
    search_at = 0;
    while let Some(matched) = next_word_match(&regex, text, search_at) {
        result.push_str(&text[copied_until..matched.start()]);
        result.push_str(&rule.to);
        copied_until = matched.end();
        search_at = matched.end();
    }
    result.push_str(&text[copied_until..]);
    RuleApplication::Replaced(result)
}

fn next_word_match<'a>(
    regex: &Regex,
    text: &'a str,
    mut search_at: usize,
) -> Option<regex::Match<'a>> {
    while let Some(matched) = regex.find_at(text, search_at) {
        if has_word_boundaries(text, matched.start(), matched.end()) {
            return Some(matched);
        }
        search_at = next_char_boundary(text, matched.start())?;
    }
    None
}

fn next_char_boundary(text: &str, start: usize) -> Option<usize> {
    text[start..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| start + offset)
}

fn has_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    let previous_is_word = text[..start].chars().next_back().is_some_and(is_word_char);
    let next_is_word = text[end..].chars().next().is_some_and(is_word_char);
    !previous_is_word && !next_is_word
}

fn is_word_char(ch: char) -> bool {
    let mut encoded = [0; 4];
    WORD_CHARACTER.is_match(ch.encode_utf8(&mut encoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(from: &str, to: &str, case_sensitive: bool) -> TextReplacementRule {
        TextReplacementRule {
            from: from.into(),
            to: to.into(),
            case_sensitive,
        }
    }

    fn settings(rules: Vec<TextReplacementRule>) -> TextReplacementSettings {
        TextReplacementSettings {
            enabled: true,
            rules,
        }
    }

    #[test]
    fn disabled_and_blank_rules_preserve_transcript() {
        let mut config = settings(vec![rule("вы", "Вы", false)]);
        config.enabled = false;
        assert_eq!(apply_text_replacements("вы".into(), &config), "вы");
        config.enabled = true;
        config.rules = vec![rule("", "x", false), rule(" \t", "x", false)];
        assert_eq!(apply_text_replacements(" вы \n".into(), &config), " вы \n");
        assert_eq!(apply_text_replacements(String::new(), &config), "");
    }

    #[test]
    fn formal_preset_covers_all_forms_without_changing_partial_words() {
        let config = settings(TextReplacementSettings::russian_formal_rules());
        config.validate().unwrap();
        assert_eq!(config.rules.len(), 16);
        let input = config
            .rules
            .iter()
            .map(|item| item.from.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let expected = config
            .rules
            .iter()
            .map(|item| item.to.clone())
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(apply_text_replacements(input, &config), expected);
        assert_eq!(
            apply_text_replacements("выставка, вашингтон, ВЫ!".into(), &config),
            "выставка, вашингтон, Вы!"
        );
    }

    #[test]
    fn word_boundaries_include_unicode_letters_marks_digits_and_underscore() {
        let config = settings(vec![rule("вы", "Вы", false)]);
        assert_eq!(
            apply_text_replacements(
                "вы вы2 _вы вы_ αвы выя вы\u{301} (вы) вы-вы".into(),
                &config
            ),
            "Вы вы2 _вы вы_ αвы выя вы\u{301} (Вы) Вы-Вы"
        );
        assert_eq!(
            apply_text_replacements("явывы вы".into(), &config),
            "явывы Вы"
        );
    }

    #[test]
    fn case_sensitive_rule_only_changes_exact_case() {
        let config = settings(vec![rule("API", "интерфейс", true)]);
        assert_eq!(
            apply_text_replacements("API api Api API".into(), &config),
            "интерфейс api Api интерфейс"
        );
    }

    #[test]
    fn patterns_and_replacement_values_are_literal() {
        let config = settings(vec![rule("C++", "$1\\api", true)]);
        assert_eq!(
            apply_text_replacements("C++ C+ C++!".into(), &config),
            "$1\\api C+ $1\\api!"
        );
        let config = settings(vec![rule("[test]", "$name", true)]);
        assert_eq!(
            apply_text_replacements("[test] test".into(), &config),
            "$name test"
        );
    }

    #[test]
    fn rules_apply_in_order_and_support_deletion() {
        let config = settings(vec![
            rule("api", "OpenRouter", false),
            rule("OpenRouter", "service", true),
            rule("remove", "", false),
        ]);
        assert_eq!(
            apply_text_replacements("api remove API.".into(), &config),
            "service  service."
        );
    }

    #[test]
    fn validation_limits_unicode_characters_rather_than_bytes() {
        let mut config = settings(vec![rule(&"я".repeat(MAX_TERM_CHARS), "Вы", false)]);
        config.validate().unwrap();
        config.rules[0].from.push('я');
        assert!(config.validate().is_err());
        config.rules[0].from = "вы".into();
        config.rules[0].to = "я".repeat(MAX_TERM_CHARS + 1);
        assert!(config.validate().is_err());
        config.rules = vec![rule("вы", "Вы", false); MAX_RULES];
        config.validate().unwrap();
        config.rules.push(rule("ваш", "Ваш", false));
        assert!(config.validate().is_err());
    }

    #[test]
    fn validation_rejects_control_characters_on_both_sides() {
        for control in ['\n', '\r', '\t', '\0', '\u{7f}'] {
            assert!(settings(vec![rule(&format!("a{control}"), "b", false)])
                .validate()
                .is_err());
            assert!(settings(vec![rule("a", &format!("b{control}"), false)])
                .validate()
                .is_err());
        }
    }

    #[test]
    fn transcripts_over_limit_are_preserved() {
        let transcript = "a".repeat(MAX_REPLACED_TEXT_BYTES + 1);
        let config = settings(vec![rule(&transcript, "small", true)]);
        assert_eq!(
            apply_text_replacements(transcript.clone(), &config),
            transcript
        );
    }

    #[test]
    fn expansion_over_limit_skips_entire_rule_and_continues_with_next_rule() {
        let transcript = format!("a {}", " ".repeat(MAX_REPLACED_TEXT_BYTES - 2));
        let config = settings(vec![rule("a", "aa", true), rule("a", "b", true)]);
        let result = apply_text_replacements(transcript, &config);
        assert_eq!(result.len(), MAX_REPLACED_TEXT_BYTES);
        assert!(result.starts_with("b "));
        assert!(result[1..].bytes().all(|byte| byte == b' '));
    }

    #[test]
    fn expansion_limit_rejects_partial_results_after_multiple_matches() {
        let transcript = format!("a a {}", " ".repeat(MAX_REPLACED_TEXT_BYTES - 5));
        let config = settings(vec![rule("a", "aa", true)]);
        assert_eq!(
            apply_text_replacements(transcript.clone(), &config),
            transcript
        );
    }
}
