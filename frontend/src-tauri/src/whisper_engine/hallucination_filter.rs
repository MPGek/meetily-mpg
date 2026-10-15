//! Removes known Whisper hallucinations (subtitle credits, video sign-offs,
//! channel calls from its training data) from transcription output.
//!
//! Works on the ordered words of one transcription, sentence by sentence, so a
//! hallucinated sentence is dropped while the real speech around it is kept.
//! Spec: openspec `whisper-hallucination-filter`.

use crate::audio::token_assignment::Token;
use once_cell::sync::Lazy;
use regex::Regex;

/// Sign-offs that also occur inside ordinary sentences, so they must make up
/// the whole normalised sentence.
static WHOLE_SENTENCE_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    ["продолжение следует", "спасибо за просмотр", "до новых встреч"]
        .iter()
        .map(|p| Regex::new(&format!("^(?:{})$", p)).expect("valid hallucination pattern"))
        .collect()
});

/// Credits and channel calls that do not occur in work speech; matched
/// anywhere in the normalised sentence.
static CONTAINED_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"dimatorzok",
        r"субтитры (?:создавал|сделал|делал|подготовил|подготовлены)",
        r"(?:добавил|спасибо за) субтитры",
        r"(?:редактор|автор|корректор) субтитров",
        r"^корректор [а-я]\.\s*[а-я]+",
        r"подписывайтесь на (?:наш|мой) канал",
        r"ставьте лайк",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("valid hallucination pattern"))
    .collect()
});

const QUOTES: &[char] = &['«', '»', '"', '„', '“', '”', '\'', '‘', '’'];
const CLOSERS: &[char] = &['»', '"', '”', '\'', '’', ')', ']', '}'];
const END_MARKS: &[char] = &['.', '!', '?', '…'];
const TRAILING_PUNCT: &[char] = &['.', '!', '?', '…', ',', ';', ':', '-', '—'];

/// Result of [`hallucination_mask`]: one flag per input word (`true` = keep)
/// and the text of every removed sentence.
#[derive(Debug, Default, PartialEq)]
pub struct HallucinationMask {
    pub keep: Vec<bool>,
    pub removed: Vec<String>,
}

fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(CLOSERS)
        .chars()
        .last()
        .is_some_and(|c| END_MARKS.contains(&c))
}

/// «А.» — one uppercase letter followed by a period, as in «А. Семкин».
fn is_initial(word: &str) -> bool {
    let mut chars = word.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(c), Some('.'), None) if c.is_uppercase()
    )
}

fn normalise(sentence: &str) -> String {
    sentence
        .to_lowercase()
        .replace('ё', "е")
        .trim_matches(|c: char| c.is_whitespace() || QUOTES.contains(&c))
        .trim_end_matches(|c: char| {
            c.is_whitespace() || QUOTES.contains(&c) || TRAILING_PUNCT.contains(&c)
        })
        .trim_start_matches(|c: char| c.is_whitespace() || QUOTES.contains(&c))
        .to_string()
}

fn is_hallucination(sentence: &str) -> bool {
    let s = normalise(sentence);
    WHOLE_SENTENCE_PATTERNS.iter().any(|re| re.is_match(&s))
        || CONTAINED_PATTERNS.iter().any(|re| re.is_match(&s))
}

/// Groups `words` into sentences and marks the words of every sentence that
/// matches a known hallucination as dropped.
pub fn hallucination_mask(words: &[&str]) -> HallucinationMask {
    let mut keep = vec![true; words.len()];
    let mut removed = Vec::new();

    let mut start = 0;
    for i in 0..words.len() {
        let last = i + 1 == words.len();
        // A sentence ending in an initial («Редактор субтитров А.») continues
        // into the next one, so a credit split at «А.» is matched as a whole.
        let continues = i > start && is_initial(words[i]);
        if !last && (!ends_sentence(words[i]) || continues) {
            continue;
        }
        let sentence = words[start..=i].join(" ");
        if is_hallucination(&sentence) {
            keep[start..=i].iter_mut().for_each(|k| *k = false);
            removed.push(sentence);
        }
        start = i + 1;
    }

    HallucinationMask { keep, removed }
}

/// Drops the tokens of hallucinated sentences, one token per word as built by
/// the Whisper paths. Kept tokens keep their timings. Returns the rebuilt text
/// and the removed sentences; with nothing removed the tokens are untouched.
pub fn drop_hallucinated_tokens(tokens: &mut Vec<Token>) -> (String, Vec<String>) {
    let words: Vec<&str> = tokens.iter().map(|t| t.text.trim()).collect();
    let mask = hallucination_mask(&words);
    if mask.removed.is_empty() {
        return (words.join(" "), mask.removed);
    }
    let mut keep = mask.keep.iter();
    tokens.retain(|_| *keep.next().unwrap_or(&true));
    if let Some(first) = tokens.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    let text = tokens
        .iter()
        .map(|t| t.text.trim())
        .collect::<Vec<_>>()
        .join(" ");
    (text, mask.removed)
}

/// Text-only variant for paths without tokens: filters the whitespace words of
/// `text`. Returns the kept text and the removed sentences; with nothing
/// removed the text is returned unchanged.
pub fn drop_hallucinated_text(text: &str) -> (String, Vec<String>) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mask = hallucination_mask(&words);
    if mask.removed.is_empty() {
        return (text.to_string(), mask.removed);
    }
    let kept: Vec<&str> = words
        .iter()
        .zip(&mask.keep)
        .filter(|(_, k)| **k)
        .map(|(w, _)| *w)
        .collect();
    (kept.join(" "), mask.removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filtered(text: &str) -> String {
        drop_hallucinated_text(text).0
    }

    #[test]
    fn hallucination_known_phrases_are_removed() {
        for phrase in [
            "Продолжение следует...",
            "Продолжение следует.",
            "Субтитры создавал DimaTorzok",
            "Субтитры сделал DimaTorzok",
            "Добавил субтитры DimaTorzok",
            "Редактор субтитров А.Семкин Корректор А.Егорова",
            "Автор субтитров А.Семкин Корректор А.Егорова",
            "Корректор А.Кулакова",
            "«Корректор А.Кулакова»",
            "Спасибо за субтитры Алексею Дубровскому!",
            "Спасибо за просмотр!",
            "До новых встреч!",
            "Подписывайтесь на наш канал и ставьте лайки!",
            "Подписывайтесь на мой канал.",
        ] {
            assert_eq!(filtered(phrase), "", "should be removed: {phrase}");
        }
    }

    #[test]
    fn hallucination_credit_split_at_initial_is_removed_whole() {
        // As after a `max_len` cut: two segments, joined into one word list.
        for text in [
            "Редактор субтитров А. Семкин Корректор А.Егорова",
            "Редактор субтитров А. Семкин Корректор А. Егорова",
        ] {
            let words: Vec<&str> = text.split_whitespace().collect();
            let mask = hallucination_mask(&words);
            assert!(mask.keep.iter().all(|k| !k), "should be removed: {text}");
            assert_eq!(mask.removed, vec![text.to_string()]);
        }
    }

    #[test]
    fn hallucination_only_the_bad_sentence_is_removed() {
        assert_eq!(
            filtered("Пока. Редактор субтитров А.Семкин Корректор А.Егорова"),
            "Пока."
        );
        assert_eq!(
            filtered("Это всё на сегодня. Продолжение следует..."),
            "Это всё на сегодня."
        );
    }

    #[test]
    fn hallucination_real_speech_is_kept() {
        for text in [
            "Продолжение следует в пятницу на планинге.",
            "Ну, понятно, годовая подписка.",
            "Но он просил тогда подписать коммент, но он ничего не ответил.",
            "Добавь субтитры к видео в плеере.",
            "с самоподписанным каким-нибудь токеном",
            "Спасибо.",
            "Спасибо за внимание.",
            "Угу.",
            "Самый редактор.",
        ] {
            assert_eq!(
                drop_hallucinated_text(text),
                (text.to_string(), vec![]),
                "should be kept: {text}"
            );
        }
    }

    #[test]
    fn hallucination_language_independent_sign_off_is_removed() {
        // The filter has no language input: a sign-off under "auto" is removed too.
        assert_eq!(filtered("Спасибо за просмотр!"), "");
    }

    #[test]
    fn hallucination_empty_and_unterminated_input() {
        assert_eq!(hallucination_mask(&[]), HallucinationMask::default());
        // No end mark: the whole text is one sentence.
        let words = ["Субтитры", "создавал", "DimaTorzok"];
        assert_eq!(hallucination_mask(&words).keep, vec![false; 3]);
        let words = ["просто", "без", "точки"];
        assert_eq!(hallucination_mask(&words).keep, vec![true; 3]);
    }

    fn tok(text: &str, start: f32, end: f32) -> Token {
        Token {
            text: text.to_string(),
            start,
            end,
            refined: false,
        }
    }

    #[test]
    fn hallucination_tokens_keep_timings_of_kept_words() {
        let mut tokens = vec![
            tok("Хорошо.", 0.5, 1.0),
            tok(" Продолжение", 1.0, 1.6),
            tok(" следует...", 1.6, 2.2),
        ];
        let (text, removed) = drop_hallucinated_tokens(&mut tokens);
        assert_eq!(text, "Хорошо.");
        assert_eq!(removed, vec!["Продолжение следует...".to_string()]);
        assert_eq!(tokens, vec![tok("Хорошо.", 0.5, 1.0)]);
    }

    #[test]
    fn hallucination_tokens_leading_drop_normalises_first_token() {
        let mut tokens = vec![
            tok("Продолжение", 0.0, 0.5),
            tok(" следует.", 0.5, 1.0),
            tok(" Хорошо.", 1.2, 1.6),
        ];
        let (text, _) = drop_hallucinated_tokens(&mut tokens);
        assert_eq!(text, "Хорошо.");
        assert_eq!(tokens, vec![tok("Хорошо.", 1.2, 1.6)]);
    }
}
