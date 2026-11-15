//! The wording of the native AnixOps Net windows, written once.
//!
//! `catalogue.json` is the only source: one stable id per sentence, with the sentence in every language. The
//! GTK window reads it through this crate and the Windows window embeds the same file, so there is no generated
//! copy that could drift. Placeholders are `{0}`, `{1}`, … and mean the same value in every language.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The catalogue as it is in the repository.
pub const CATALOGUE: &str = include_str!("../catalogue.json");

/// A language there is wording for. The ids are the ones the preferences store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Chinese,
    TraditionalChinese,
    English,
}

impl Language {
    /// Every language, in the order they are offered.
    pub const ALL: [Language; 3] = [
        Language::Chinese,
        Language::TraditionalChinese,
        Language::English,
    ];

    /// The id in the catalogue and in the preferences.
    pub fn id(self) -> &'static str {
        match self {
            Language::Chinese => "zh",
            Language::TraditionalChinese => "zh-Hant",
            Language::English => "en",
        }
    }

    /// The language's own name, for the language picker.
    pub fn name(self) -> &'static str {
        match self {
            Language::Chinese => "简体中文",
            Language::TraditionalChinese => "繁體中文",
            Language::English => "English",
        }
    }

    /// The language with this id, if there is wording for it.
    pub fn from_id(id: &str) -> Option<Language> {
        Language::ALL
            .into_iter()
            .find(|language| language.id() == id)
    }

    /// The language to use for a system locale such as `zh_CN.UTF-8`, `zh-Hant-TW` or `en-US`. The script
    /// decides when the locale names one; otherwise Taiwan, Hong Kong and Macau read Traditional Chinese and
    /// other Chinese locales read Simplified Chinese. Everything else reads English.
    pub fn preferred(locale: &str) -> Language {
        let locale = locale
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut parts = locale.split(['-', '_']);
        if parts.next() != Some("zh") {
            return Language::English;
        }
        // A script that is written out decides; the region only speaks when there is none ("zh-Hans-HK" is
        // Simplified Chinese as written in Hong Kong).
        let mut region = Language::Chinese;
        for part in parts {
            match part {
                "hans" => return Language::Chinese,
                "hant" => return Language::TraditionalChinese,
                "tw" | "hk" | "mo" => region = Language::TraditionalChinese,
                _ => {}
            }
        }
        region
    }
}

/// The catalogue once read: for every id, the sentence in each of [`Language::ALL`], in that order.
pub type Table = BTreeMap<String, [String; 3]>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        parse(CATALOGUE).expect("the wording catalogue is checked by this crate's tests")
    })
}

/// Reads a catalogue: every id must carry a sentence in every language and nothing else.
pub fn parse(text: &str) -> Result<Table, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| format!("the catalogue is not JSON: {error}"))?;
    let entries = value
        .as_object()
        .ok_or_else(|| "the catalogue is not an object".to_owned())?;
    let mut table = Table::new();
    for (id, wording) in entries {
        let wording = wording
            .as_object()
            .ok_or_else(|| format!("'{id}' is not an object"))?;
        if wording.len() != Language::ALL.len() {
            return Err(format!(
                "'{id}' does not have exactly one sentence per language"
            ));
        }
        let mut sentences: [String; 3] = Default::default();
        for (slot, language) in sentences.iter_mut().zip(Language::ALL) {
            *slot = wording
                .get(language.id())
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("'{id}' has no sentence in '{}'", language.id()))?
                .to_owned();
        }
        table.insert(id.clone(), sentences);
    }
    Ok(table)
}

/// Every id in the catalogue, in order.
pub fn ids() -> impl Iterator<Item = &'static str> {
    table().keys().map(String::as_str)
}

/// The sentence for an id, if the catalogue has it.
pub fn find(language: Language, id: &str) -> Option<&'static str> {
    let index = Language::ALL
        .iter()
        .position(|candidate| *candidate == language)
        .unwrap_or_default();
    table().get(id).map(|sentences| sentences[index].as_str())
}

/// The sentence for an id. An id without wording is a programming error: it stops a debug build, and a
/// release build shows nothing in its place rather than taking the window down.
pub fn text(language: Language, id: &str) -> &'static str {
    match find(language, id) {
        Some(sentence) => sentence,
        None => {
            debug_assert!(false, "no wording for '{id}'");
            ""
        }
    }
}

/// The sentence for an id with `{0}`, `{1}`, … replaced by the values, written as they are given.
pub fn format(language: Language, id: &str, values: &[&str]) -> String {
    fill(text(language, id), values)
}

/// The numbers of the placeholders in a sentence, in order of appearance.
pub fn placeholders(sentence: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut rest = sentence;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('}') {
            if let Ok(number) = rest[..end].parse::<usize>() {
                found.push(number);
                rest = &rest[end + 1..];
            }
        }
    }
    found
}

fn fill(sentence: &str, values: &[&str]) -> String {
    let mut filled = String::with_capacity(sentence.len());
    let mut rest = sentence;
    while let Some(start) = rest.find('{') {
        filled.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let number = after.find('}').and_then(|end| {
            after[..end]
                .parse::<usize>()
                .ok()
                .map(|number| (number, end))
        });
        match number {
            Some((number, end)) if number < values.len() => {
                filled.push_str(values[number]);
                rest = &after[end + 1..];
            }
            _ => {
                filled.push('{');
                rest = after;
            }
        }
    }
    filled.push_str(rest);
    filled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_reads() {
        let table = parse(CATALOGUE).expect("the catalogue reads");
        assert!(!table.is_empty());
    }

    #[test]
    fn no_sentence_is_empty() {
        for id in ids() {
            for language in Language::ALL {
                assert!(
                    !text(language, id).trim().is_empty(),
                    "{id} is empty in {}",
                    language.id()
                );
            }
        }
    }

    #[test]
    fn ids_are_lowercase_dotted_words_in_order() {
        // The file is kept in order so that two branches adding sentences conflict where they overlap and
        // nowhere else. serde_json's map is sorted, so the order is read from the text.
        let mut previous = String::new();
        for line in CATALOGUE.lines() {
            let Some(id) = line
                .trim_start()
                .strip_prefix('"')
                .and_then(|rest| rest.split('"').next())
            else {
                continue;
            };
            assert!(
                id.split('.').all(|word| !word.is_empty()
                    && word.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_')),
                "'{id}' is not lowercase words joined by dots"
            );
            assert!(previous.as_str() < id, "'{id}' is out of order or repeated");
            previous = id.to_owned();
        }
        assert_eq!(
            CATALOGUE.lines().count() - 2,
            ids().count(),
            "one sentence id per line"
        );
    }

    #[test]
    fn every_language_takes_the_same_values() {
        for id in ids() {
            let mut expected = placeholders(text(Language::English, id));
            expected.sort_unstable();
            // {0}, {1}, … without gaps: a sentence cannot ask for the second value and not the first.
            let mut distinct = expected.clone();
            distinct.dedup();
            assert_eq!(
                distinct,
                (0..distinct.len()).collect::<Vec<_>>(),
                "{id} skips a value"
            );
            for language in Language::ALL {
                let mut found = placeholders(text(language, id));
                found.sort_unstable();
                assert_eq!(
                    found,
                    expected,
                    "{id} takes other values in {}",
                    language.id()
                );
            }
        }
    }

    #[test]
    fn no_sentence_has_stray_space_or_ascii_ellipsis() {
        for id in ids() {
            for language in Language::ALL {
                let sentence = text(language, id);
                assert_eq!(sentence, sentence.trim(), "{id} in {}", language.id());
                assert!(
                    !sentence.contains("..."),
                    "{id} in {} uses three dots",
                    language.id()
                );
                assert!(
                    !sentence.contains("  "),
                    "{id} in {} has a double space",
                    language.id()
                );
            }
        }
    }

    #[test]
    fn traditional_chinese_is_not_left_in_simplified_characters() {
        // A few characters that only exist in the Simplified script and are common in this app's wording.
        const SIMPLIFIED_ONLY: &str = "这设规则连线请为开关节点状态览页启动诊断显条实订阅测试组扩写览径时号类级删认证书择钮错误";
        for id in ids() {
            let sentence = text(Language::TraditionalChinese, id);
            let left: String = sentence
                .chars()
                .filter(|c| SIMPLIFIED_ONLY.contains(*c))
                .collect();
            assert!(
                left.is_empty(),
                "{id} keeps Simplified characters in zh-Hant: {left}"
            );
        }
    }

    #[test]
    fn values_are_filled_in_by_number() {
        assert_eq!(fill("{0} of {1}", &["3", "9"]), "3 of 9");
        assert_eq!(fill("{1} / {0} / {1}", &["a", "b"]), "b / a / b");
        // What is not a placeholder with a value stays as written.
        assert_eq!(fill("{2} {x} { {0}", &["a"]), "{2} {x} { a");
        assert_eq!(fill("no values", &[]), "no values");
        assert_eq!(
            format(Language::English, "requests.count", &["1234", "50000"]),
            "1234 of 50000 requests"
        );
        assert_eq!(
            format(Language::Chinese, "requests.count", &["1234", "50000"]),
            "1234 / 50000 条请求"
        );
    }

    #[test]
    fn placeholders_are_found_in_order() {
        assert_eq!(placeholders("{1} then {0}"), vec![1, 0]);
        assert_eq!(placeholders("none {x} {"), Vec::<usize>::new());
    }

    #[test]
    fn a_broken_catalogue_is_refused() {
        assert!(parse("[]").is_err());
        assert!(parse(r#"{"a": "text"}"#).is_err());
        assert!(parse(r#"{"a": {"zh": "一", "en": "one"}}"#).is_err());
        assert!(parse(r#"{"a": {"zh": "一", "zh-Hant": "一", "en": 1}}"#).is_err());
        assert!(parse(r#"{"a": {"zh": "一", "zh-Hant": "一", "en": "one", "fr": "un"}}"#).is_err());
        assert!(parse(r#"{"a": {"zh": "一", "zh-Hant": "一", "en": "one"}}"#).is_ok());
    }

    #[test]
    fn the_language_follows_the_system_locale() {
        for (locale, expected) in [
            ("zh_CN.UTF-8", Language::Chinese),
            ("zh-CN", Language::Chinese),
            ("zh", Language::Chinese),
            ("zh-Hans-HK", Language::Chinese),
            ("ZH-TW", Language::TraditionalChinese),
            ("zh_HK.UTF-8", Language::TraditionalChinese),
            ("zh-MO", Language::TraditionalChinese),
            ("zh-Hant", Language::TraditionalChinese),
            ("zh-Hant-CN", Language::TraditionalChinese),
            ("zh_TW.Big5@stroke", Language::TraditionalChinese),
            ("zh-Hans-TW", Language::Chinese),
            ("en_US.UTF-8", Language::English),
            ("de-DE", Language::English),
            ("zhx", Language::English),
            ("C", Language::English),
            ("", Language::English),
        ] {
            assert_eq!(Language::preferred(locale), expected, "{locale}");
        }
    }

    #[test]
    fn ids_and_languages_round_trip() {
        for language in Language::ALL {
            assert_eq!(Language::from_id(language.id()), Some(language));
            assert!(!language.name().is_empty());
        }
        assert_eq!(Language::from_id("fr"), None);
        assert_eq!(find(Language::English, "no.such.id"), None);
    }
}
