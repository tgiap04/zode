//! What the panel's Inspect tab shows, and the helpers every engine's mapping
//! shares.
//!
//! The engines disagree about the shape of `inspect` -- Docker prints
//! `Entrypoint` as an array and Podman as a string, Podman's network object is
//! snake_case where Docker's is PascalCase -- so the mappings read a
//! `serde_json::Value` through the small accessors below instead of one struct
//! per engine. A key an engine does not print leaves a row out; it never fails
//! the whole inspection.
//!
//! Engine output is untrusted text: everything here is shown, nothing is acted on.

use serde_json::Value;

pub use crate::secret_masking::{looks_secret, mask_secrets_in_text};
use crate::secret_masking::{mask_secrets_in_json, mask_url_credentials, masked_option};

/// What a masked value is displayed as.
pub const MASK: &str = "********";

/// One engine's answer for one resource, ready to draw.
#[derive(Clone, PartialEq, Eq)]
pub struct Inspection {
    pub sections: Vec<InspectSection>,
    /// The engine's own JSON, pretty-printed, with every secret-looking value
    /// replaced by [`MASK`] (see [`crate::mask_secrets_in_text`] for what that
    /// covers). This is the one to show by default.
    pub raw_json: String,
    /// The same JSON untouched. Shown only after the person asks to reveal.
    pub raw_json_revealed: String,
}

impl Inspection {
    /// Whether anything in this inspection is masked, so the view knows whether
    /// to offer a reveal toggle at all.
    pub fn has_secrets(&self) -> bool {
        self.raw_json != self.raw_json_revealed
            || self
                .sections
                .iter()
                .any(|section| section.rows.iter().any(InspectRow::is_secret))
    }
}

/// Redacts the unmasked JSON: a `{:?}` of an inspection ends up in logs and
/// failed-assertion output, which is the last place a password should appear.
impl std::fmt::Debug for Inspection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Inspection")
            .field("sections", &self.sections)
            .field("raw_json", &self.raw_json)
            .field("raw_json_revealed", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectSection {
    pub title: String,
    pub rows: Vec<InspectRow>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct InspectRow {
    pub label: String,
    /// What to draw. For a secret row this is [`MASK`] or the value with its
    /// secret part masked, never the real value.
    pub value: String,
    /// The real value of a secret row, kept so the view can reveal it on
    /// demand. `None` for an ordinary row.
    pub secret_value: Option<String>,
}

/// Redacts `secret_value`, for the same reason as [`Inspection`]'s.
impl std::fmt::Debug for InspectRow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InspectRow")
            .field("label", &self.label)
            .field("value", &self.value)
            .field(
                "secret_value",
                &self.secret_value.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl InspectRow {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            secret_value: None,
        }
    }

    pub fn secret(label: impl Into<String>, real_value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: MASK.to_string(),
            secret_value: Some(real_value.into()),
        }
    }

    /// A row whose `real_value` has a secret part: drawn as `shown`, which has
    /// that part masked, and revealed as the real value.
    pub fn masked(
        label: impl Into<String>,
        shown: impl Into<String>,
        real_value: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            value: shown.into(),
            secret_value: Some(real_value.into()),
        }
    }

    pub fn is_secret(&self) -> bool {
        self.secret_value.is_some()
    }

    /// The real value, whether or not it is masked on screen.
    pub fn revealed(&self) -> &str {
        self.secret_value.as_deref().unwrap_or(&self.value)
    }
}

/// A `KEY=value` entry as a row: masked whole when the key looks secret, and
/// with just the password masked when the value is a URL that carries one.
pub(crate) fn pair_row(key: &str, value: &str) -> InspectRow {
    if looks_secret(key) {
        InspectRow::secret(key, value)
    } else {
        value_row(key, value)
    }
}

/// A row whose key says nothing about its value, masked only where the value
/// is a URL that carries a password.
fn value_row(key: &str, value: &str) -> InspectRow {
    let shown = mask_url_credentials(value);
    if shown == value {
        InspectRow::new(key, value)
    } else {
        InspectRow::masked(key, shown, value)
    }
}

/// A `KEY=value` environment entry as a row.
pub(crate) fn env_row(entry: &str) -> InspectRow {
    let (key, value) = entry.split_once('=').unwrap_or((entry, ""));
    pair_row(key, value)
}

pub(crate) fn env_rows(list: Option<&Value>) -> Vec<InspectRow> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(env_row)
        .collect()
}

pub(crate) fn label_rows(labels: Option<&Value>) -> Vec<InspectRow> {
    sorted_entries(labels)
        .into_iter()
        .map(|(key, value)| value_row(key, &text(Some(value)).unwrap_or_default()))
        .collect()
}

/// A volume's or network's driver options. Unlike labels these hold what a
/// mount needs to reach its storage, so a password among them is masked.
pub(crate) fn option_rows(options: Option<&Value>) -> Vec<InspectRow> {
    sorted_entries(options)
        .into_iter()
        .map(|(key, value)| {
            let real = text(Some(value)).unwrap_or_default();
            match masked_option(key, &real) {
                Some(shown) => InspectRow::masked(key, shown, real),
                None => InspectRow::new(key, real),
            }
        })
        .collect()
}

/// Both JSON renderings of `root`, masked and untouched.
pub(crate) fn finish(root: &Value, sections: Vec<InspectSection>) -> Inspection {
    let raw_json_revealed = pretty(root);
    let mut masked = root.clone();
    mask_secrets_in_json(&mut masked);
    Inspection {
        sections,
        raw_json: pretty(&masked),
        raw_json_revealed,
    }
}

fn pretty(value: &Value) -> String {
    // A `Value` always serialises; the fallback keeps this infallible without
    // an `unwrap`.
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Accumulates sections, dropping any that ended up with no rows.
#[derive(Default)]
pub(crate) struct SectionList(pub(crate) Vec<InspectSection>);

impl SectionList {
    pub(crate) fn push(&mut self, title: impl Into<String>, rows: Vec<InspectRow>) {
        if !rows.is_empty() {
            self.0.push(InspectSection {
                title: title.into(),
                rows,
            });
        }
    }
}

/// Rows built from optional values: a `None` leaves no row behind.
#[derive(Default)]
pub(crate) struct RowList(pub(crate) Vec<InspectRow>);

impl RowList {
    pub(crate) fn add(&mut self, label: &str, value: Option<String>) -> &mut Self {
        if let Some(value) = value {
            self.0.push(InspectRow::new(label, value));
        }
        self
    }

    /// A command line: masked where it carries a secret, like an environment
    /// value, and revealable the same way.
    pub(crate) fn add_command(&mut self, label: &str, value: Option<String>) -> &mut Self {
        if let Some(value) = value {
            let shown = mask_secrets_in_text(&value);
            self.0.push(if shown == value {
                InspectRow::new(label, value)
            } else {
                InspectRow::masked(label, shown, value)
            });
        }
        self
    }
}

/// The first of `names` that `value` has and is not `null`.
pub(crate) fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names
        .iter()
        .filter_map(|name| value.get(*name))
        .find(|found| !found.is_null())
}

/// A nested lookup by exact keys.
pub(crate) fn dig<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .filter(|found| !found.is_null())
}

/// A string (non-empty) or a number as text.
pub(crate) fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub(crate) fn flag(value: Option<&Value>) -> Option<bool> {
    value?.as_bool()
}

pub(crate) fn number(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

pub(crate) fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_string()
}

/// A command: an array of words, or one string (Podman prints `Entrypoint`
/// that way).
pub(crate) fn words(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

/// An object's entries in key order, so a row list does not reshuffle between
/// two inspections of the same resource.
pub(crate) fn sorted_entries(value: Option<&Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<_> = value
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|map| map.iter())
        .collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    entries
}

pub(crate) fn list_len(value: Option<&Value>) -> Option<usize> {
    value.and_then(Value::as_array).map(Vec::len)
}

/// `2026-11-17T05:54:00.247167006Z` as `2026-11-17 05:54:00 UTC`; the zero time
/// Docker prints for "never" as nothing.
pub(crate) fn timestamp(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.starts_with("0001-01-01") {
        return None;
    }
    let Some((date, rest)) = raw.split_once('T') else {
        return Some(raw.to_string());
    };
    let (clock, zone) = match rest.find(['Z', '+', '-']) {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let clock = clock.split('.').next().unwrap_or(clock);
    let zone = if zone == "Z" { " UTC" } else { zone };
    Some(format!("{date} {clock}{zone}"))
}

pub(crate) fn timestamp_of(value: Option<&Value>) -> Option<String> {
    text(value).and_then(|raw| timestamp(&raw))
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut scaled = bytes as f64;
    let mut unit = 0;
    while scaled >= 1024.0 && unit + 1 < UNITS.len() {
        scaled /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        let name = UNITS.get(unit).copied().unwrap_or("TiB");
        format!("{scaled:.1} {name}")
    }
}

/// `1.5`, `2`, `0.25` -- no trailing zeros.
pub(crate) fn trim_decimal(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// The inspected object itself: Docker and Podman print a one-element array,
/// Kubernetes a bare object.
pub(crate) fn single_object(
    stdout: &str,
    id: &str,
) -> Result<Value, crate::backend::ContainerError> {
    let parsed: Value = serde_json::from_str(stdout.trim()).map_err(|error| {
        crate::backend::ContainerError::Parse {
            detail: format!("could not read inspect output: {error}"),
        }
    })?;
    let object = match parsed {
        Value::Array(items) => items.into_iter().next(),
        object @ Value::Object(_) => Some(object),
        _ => None,
    };
    object.ok_or_else(|| crate::backend::ContainerError::NotFound { id: id.to_string() })
}
