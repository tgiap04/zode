use crate::TranslateError;

/// Lowercases the language, upper-cases a two-letter region and title-cases a
/// four-letter script, so `PT_br` and `zh-hans` read as `pt-BR` and `zh-Hans`.
/// A three-digit UN M.49 region such as `es-419` is kept as is.
pub fn normalize_language_code(code: &str) -> Result<String, TranslateError> {
    let invalid = || TranslateError::InvalidLanguage {
        code: code.to_string(),
    };
    let trimmed = code.trim().replace('_', "-");
    let (language, subtag) = match trimmed.split_once('-') {
        Some((language, subtag)) => (language, Some(subtag)),
        None => (trimmed.as_str(), None),
    };
    let all_letters = |part: &str| {
        part.chars()
            .all(|character| character.is_ascii_alphabetic())
    };
    if !(2..=3).contains(&language.len()) || !all_letters(language) {
        return Err(invalid());
    }
    let mut normalized = language.to_ascii_lowercase();
    if let Some(subtag) = subtag {
        let numeric_region =
            subtag.len() == 3 && subtag.chars().all(|character| character.is_ascii_digit());
        if !numeric_region && !all_letters(subtag) {
            return Err(invalid());
        }
        normalized.push('-');
        match subtag.len() {
            2 => normalized.push_str(&subtag.to_ascii_uppercase()),
            3 if numeric_region => normalized.push_str(subtag),
            4 => {
                let lower = subtag.to_ascii_lowercase();
                normalized.push_str(&lower[..1].to_ascii_uppercase());
                normalized.push_str(&lower[1..]);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(normalized)
}

const LANGUAGE_NAMES: &[(&str, &str)] = &[
    ("ar", "Arabic"),
    ("bg", "Bulgarian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("de", "German"),
    ("el", "Greek"),
    ("en", "English"),
    ("es", "Spanish"),
    ("et", "Estonian"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("km", "Khmer"),
    ("ko", "Korean"),
    ("lo", "Lao"),
    ("lt", "Lithuanian"),
    ("lv", "Latvian"),
    ("ms", "Malay"),
    ("nb", "Norwegian Bokmål"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("sv", "Swedish"),
    ("th", "Thai"),
    ("tl", "Filipino"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("vi", "Vietnamese"),
    ("zh", "Chinese"),
];

/// Every language with a known name, as `(code, English name)`, sorted by code.
pub fn known_languages() -> &'static [(&'static str, &'static str)] {
    LANGUAGE_NAMES
}

/// The English name of a language, ignoring any region or script.
pub fn language_name(code: &str) -> Option<&'static str> {
    let base = base_language(code);
    LANGUAGE_NAMES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(base))
        .map(|(_, name)| *name)
}

fn base_language(code: &str) -> &str {
    code.split(['-', '_']).next().unwrap_or(code)
}

/// Languages DeepL translates without beta flags, from the `languages`
/// example in https://developers.deepl.com/api-reference/openapi.yaml.
const DEEPL_LANGUAGES: &[&str] = &[
    "ar", "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "he", "hu", "id", "it", "ja",
    "ko", "lt", "lv", "nb", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "th", "tr", "uk", "vi",
    "zh",
];

/// Target variants DeepL accepts, from the same specification.
const DEEPL_TARGET_VARIANTS: &[&str] = &[
    "en-GB", "en-US", "es-419", "pt-BR", "pt-PT", "zh-Hans", "zh-Hant",
];

/// DeepL takes only the base language as a source.
pub(crate) fn deepl_source_code(code: &str) -> Option<String> {
    let base = base_language(code).to_ascii_lowercase();
    DEEPL_LANGUAGES
        .contains(&base.as_str())
        .then(|| base.to_ascii_uppercase())
}

/// DeepL deprecated plain `EN` and `PT` as targets, so those get the variant
/// most readers expect; a variant the user asked for is kept.
pub(crate) fn deepl_target_code(code: &str) -> Option<String> {
    if let Some(variant) = DEEPL_TARGET_VARIANTS
        .iter()
        .find(|variant| variant.eq_ignore_ascii_case(code))
    {
        return Some(variant.to_ascii_uppercase());
    }
    let base = base_language(code).to_ascii_lowercase();
    if !DEEPL_LANGUAGES.contains(&base.as_str()) {
        return None;
    }
    let target = match base.as_str() {
        "en" => "EN-US".to_string(),
        "pt" => "PT-BR".to_string(),
        "zh" => "ZH-HANS".to_string(),
        other => other.to_ascii_uppercase(),
    };
    Some(target)
}
