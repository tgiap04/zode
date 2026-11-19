//! Finding secrets in names, text and JSON, and replacing them with [`MASK`].
//!
//! Deliberately broad, like `looks_secret` has always been: a false positive
//! costs one click to reveal, a false negative puts a password on a screen that
//! may be shared. What is recognised, in order of how obvious it is:
//!
//! - a name that says so (`DB_PASSWORD`, `SENTRY_DSN`),
//! - credentials inside a URL (`postgres://app:pw@db/app`), whatever the
//!   variable is called,
//! - the same two inside a command line (`--password=x`, `--password x`,
//!   `-e API_TOKEN=x`), and inside a mount option list (`o=...,password=x`).

use serde_json::{Map, Value};

use crate::inspect::MASK;

const MARKERS: &[&str] = &[
    "PASSWORD",
    "SECRET",
    "TOKEN",
    "KEY",
    "PASS",
    "CREDENTIAL",
    "DSN",
    "AUTH",
    "PRIVATE",
    "JWT",
    "COOKIE",
    "SALT",
    "CONNECTION_STRING",
    "CONNECTIONSTRING",
];

/// JSON keys whose value is a command line, as an array of words or one string.
const COMMAND_KEYS: &[&str] = &[
    "Cmd",
    "Entrypoint",
    "Test",
    "CreateCommand",
    "Args",
    "args",
    "command",
];

/// The annotation `kubectl apply` writes: the whole manifest as one string,
/// environment values included.
const LAST_APPLIED: &str = "kubectl.kubernetes.io/last-applied-configuration";

/// Whether a name suggests its value is a secret.
pub fn looks_secret(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    MARKERS.iter().any(|marker| upper.contains(marker))
}

/// `text` with every secret-looking value replaced by [`MASK`]: `KEY=value`
/// assignments, `--flag=value` and `--flag value` for a secret-looking flag, and
/// the password in a `scheme://user:password@host` URL. Spacing and everything
/// else is kept as it was, so the result still reads as the original.
///
/// For text the panel shows but never inspects: a command line, an image
/// layer's `created_by`, a preview of a `run` command.
pub fn mask_secrets_in_text(text: &str) -> String {
    Masker::default().text(text)
}

/// Masks command words one after another, remembering that a secret-looking
/// flag at the end of one word has its value in the next.
#[derive(Default)]
struct Masker {
    value_follows: bool,
}

impl Masker {
    fn text(&mut self, text: &str) -> String {
        let mut masked = String::with_capacity(text.len());
        for piece in pieces(text) {
            match piece {
                Piece::Space(space) => masked.push_str(space),
                Piece::Word(word) => masked.push_str(&self.word(word)),
            }
        }
        masked
    }

    fn word(&mut self, word: &str) -> String {
        if std::mem::take(&mut self.value_follows) && !word.starts_with('-') {
            return MASK.to_string();
        }
        self.value_follows = word.starts_with("--")
            && !word.contains('=')
            && looks_secret(word.trim_start_matches('-'));
        mask_word(word)
    }
}

enum Piece<'a> {
    Space(&'a str),
    Word(&'a str),
}

/// Splits into runs of whitespace and words; a quoted stretch is part of one
/// word, so `PASSWORD='a b'` is not read as two.
fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut run_start = 0;
    let mut run_is_space: Option<bool> = None;
    let mut quote: Option<char> = None;
    fn close(is_space: bool, run: &str) -> Piece<'_> {
        if is_space {
            Piece::Space(run)
        } else {
            Piece::Word(run)
        }
    }
    for (index, character) in text.char_indices() {
        let is_space = quote.is_none() && character.is_whitespace();
        match quote {
            Some(open) if character == open => quote = None,
            None if matches!(character, '\'' | '"') => quote = Some(character),
            _ => {}
        }
        if run_is_space != Some(is_space) {
            if let Some(previous) = run_is_space {
                pieces.push(close(previous, &text[run_start..index]));
            }
            run_start = index;
            run_is_space = Some(is_space);
        }
    }
    if let Some(last) = run_is_space {
        pieces.push(close(last, &text[run_start..]));
    }
    pieces
}

fn is_assignment_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        })
}

/// One word: `KEY=value`, `--flag=value`, `--env=KEY=value`, or anything
/// holding a URL.
fn mask_word(word: &str) -> String {
    let body = word.trim_start_matches(['-', '\'', '"']);
    let prefix = &word[..word.len() - body.len()];
    if let Some((key, value)) = body.split_once('=')
        && is_assignment_key(key)
    {
        if looks_secret(key) {
            if value.is_empty() {
                return word.to_string();
            }
            // `"token=abc"` as one quoted word keeps its closing quote.
            let closing = prefix
                .chars()
                .next()
                .filter(|open| matches!(open, '\'' | '"') && word.ends_with(*open))
                .map(String::from)
                .unwrap_or_default();
            return format!("{prefix}{key}={MASK}{closing}");
        }
        if prefix.starts_with('-') && value.contains('=') {
            return format!("{prefix}{key}={}", mask_word(value));
        }
    }
    mask_url_credentials(word)
}

fn is_scheme_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
}

fn ends_authority(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '/' | '?' | '#' | '"' | '\'' | '`' | '<' | '>' | '\\'
        )
}

/// `text` with the password of every `scheme://user:password@host` replaced by
/// [`MASK`]. A user name alone (`https://TOKEN@github.com`) is masked for the
/// web schemes only: there it is how a token is passed, while in
/// `ssh://git@github.com` it is just a name.
pub(crate) fn mask_url_credentials(text: &str) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(marker) = rest.find("://") {
        let scheme_start = rest[..marker]
            .char_indices()
            .rev()
            .find(|(_, character)| !is_scheme_character(*character))
            .map(|(index, character)| index + character.len_utf8())
            .unwrap_or(0);
        let authority_start = marker + 3;
        let authority_end = rest[authority_start..]
            .find(ends_authority)
            .map_or(rest.len(), |offset| authority_start + offset);
        masked.push_str(&rest[..authority_start]);
        masked.push_str(&mask_userinfo(
            &rest[scheme_start..marker],
            &rest[authority_start..authority_end],
        ));
        rest = &rest[authority_end..];
    }
    masked.push_str(rest);
    masked
}

fn mask_userinfo(scheme: &str, authority: &str) -> String {
    let Some(at) = authority.rfind('@') else {
        return authority.to_string();
    };
    let (userinfo, host) = authority.split_at(at);
    match userinfo.split_once(':') {
        Some((user, password)) if !password.is_empty() => format!("{user}:{MASK}{host}"),
        Some(_) => authority.to_string(),
        None if !userinfo.is_empty()
            && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) =>
        {
            format!("{MASK}{host}")
        }
        None => authority.to_string(),
    }
}

/// A mount or driver option list (`username=bob,password=x`), each piece read
/// as an assignment.
pub(crate) fn mask_option_list(value: &str) -> String {
    value
        .split(',')
        .map(mask_word)
        .collect::<Vec<_>>()
        .join(",")
}

/// What an option is displayed as when its value is not shown as it is, `None`
/// when it can be shown plainly.
pub(crate) fn masked_option(key: &str, value: &str) -> Option<String> {
    let shown = if looks_secret(key) {
        MASK.to_string()
    } else {
        mask_option_list(value)
    };
    (shown != value).then_some(shown)
}

/// Masks everything secret-looking, anywhere in an inspect document.
///
/// Environment (`Env`: a list of `KEY=value`, `env`: a list of `{name,
/// value}`), command lines, option maps, the last-applied-configuration
/// annotation, and any string holding a URL with credentials.
pub(crate) fn mask_secrets_in_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                mask_member(key, child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask_secrets_in_json),
        Value::String(text) => mask_string(text),
        _ => {}
    }
}

fn mask_member(key: &str, child: &mut Value) {
    match child {
        Value::Array(entries) if key == "Env" || key == "env" => {
            entries.iter_mut().for_each(mask_env_entry);
        }
        Value::Array(words) if COMMAND_KEYS.contains(&key) => mask_words(words),
        Value::String(text) if COMMAND_KEYS.contains(&key) => {
            *text = mask_secrets_in_text(text);
        }
        Value::Object(options) if key == "Options" || key == "options" => mask_options(options),
        Value::String(text) if key == LAST_APPLIED => *text = mask_manifest(text),
        other => mask_secrets_in_json(other),
    }
}

fn mask_string(text: &mut String) {
    if text.contains("://") {
        *text = mask_url_credentials(text);
    }
}

fn mask_words(words: &mut [Value]) {
    let mut masker = Masker::default();
    for word in words {
        if let Value::String(text) = word {
            *text = masker.text(text);
        }
    }
}

fn mask_options(options: &mut Map<String, Value>) {
    for (key, value) in options.iter_mut() {
        if let Value::String(text) = value
            && let Some(shown) = masked_option(key, text)
        {
            *text = shown;
        }
    }
}

fn mask_env_entry(entry: &mut Value) {
    match entry {
        Value::String(text) => {
            if let Some((key, value)) = text.split_once('=') {
                *text = if looks_secret(key) {
                    format!("{key}={MASK}")
                } else {
                    format!("{key}={}", mask_url_credentials(value))
                };
            }
        }
        Value::Object(map) => {
            let secret = map
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(looks_secret);
            if let Some(value) = map.get_mut("value") {
                if secret {
                    *value = Value::String(MASK.to_string());
                } else if let Value::String(text) = value {
                    mask_string(text);
                }
            }
        }
        _ => {}
    }
}

/// The manifest `kubectl apply` stored, masked the way the rest of the document
/// is. Left as it was when nothing in it is secret, so a reveal toggle is not
/// offered for a re-serialisation; masked whole when it cannot be read.
fn mask_manifest(text: &str) -> String {
    let Ok(mut manifest) = serde_json::from_str::<Value>(text) else {
        return MASK.to_string();
    };
    let original = manifest.clone();
    mask_secrets_in_json(&mut manifest);
    if manifest == original {
        text.to_string()
    } else {
        manifest.to_string()
    }
}
