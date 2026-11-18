//! Holds what was typed into a cell to the column it is for, before anything
//! is saved, so the grid can say *what* is wrong next to the cell instead of
//! showing the server's error after the fact.
//!
//! The server stays the final judge: a type this module does not know is
//! accepted, and so is anything it cannot be sure about. A false rejection
//! blocks an edit the database would have taken; a false acceptance only costs
//! the error the server then sends.
//!
//! How strictly a value is held to its column comes from
//! [`Dialect::type_discipline`], so no engine is named here.
use crate::sql_dialect::{Dialect, TypeDiscipline};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};
use database::protocol::{Cell, ColumnDef};
use std::net::IpAddr;

/// Why a cell cannot be edited at all, as opposed to why a value is wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotEditable {
    PrimaryKey,
    Binary,
    NestedDocument,
}

impl NotEditable {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::PrimaryKey => "part of the primary key",
            Self::Binary => "binary values cannot be edited here",
            Self::NestedDocument => "nested documents cannot be edited here",
        }
    }
}

/// Key columns are refused because changing one changes which row the edit
/// addresses.
pub(crate) fn editability(
    dialect: &Dialect,
    column: &ColumnDef,
    cell: &Cell,
) -> Result<(), NotEditable> {
    if column.primary_key {
        return Err(NotEditable::PrimaryKey);
    }
    match cell {
        Cell::Binary { .. } => Err(NotEditable::Binary),
        // A `json` column is text a person can write; a document store's
        // nested value is a tree the grid has no editor for.
        Cell::Json { .. } if dialect.type_discipline() == TypeDiscipline::Schemaless => {
            Err(NotEditable::NestedDocument)
        }
        _ => Ok(()),
    }
}

/// `input` is what was typed, `None` setting the cell to NULL. `current` is the
/// cell being replaced: for a store without column types it is the only
/// evidence of what the field holds.
///
/// The error is a sentence about what is expected, shown beside the cell.
pub(crate) fn validate(
    dialect: &Dialect,
    column: &ColumnDef,
    current: &Cell,
    input: Option<&str>,
) -> Result<(), String> {
    let Some(text) = input else {
        return if column.nullable {
            Ok(())
        } else {
            Err("cannot be NULL".into())
        };
    };
    check(
        &family_of(dialect, column, current),
        text,
        dialect.reads_word_values(),
    )
}

/// What to store for `input` once it has passed [`validate`]: the check reads
/// past the blanks around a number, a date or an id, and the server may not,
/// so it is sent without them. Text, JSON and the choices of an enum are what
/// was typed, blanks included.
pub(crate) fn tidy(dialect: &Dialect, column: &ColumnDef, current: &Cell, input: String) -> String {
    let trimmed = input.trim();
    if trimmed.len() != input.len() && is_read_past_blanks(&family_of(dialect, column, current)) {
        return trimmed.to_string();
    }
    input
}

fn is_read_past_blanks(family: &Family) -> bool {
    !matches!(
        family,
        Family::Text { .. }
            | Family::Json
            | Family::Enumeration(_)
            | Family::Set(_)
            | Family::Unchecked
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Family {
    Integer {
        min: i128,
        max: i128,
    },
    Float,
    Decimal {
        integer_digits: Option<usize>,
        unsigned: bool,
    },
    Boolean,
    Text {
        max_chars: usize,
    },
    Date,
    Time,
    Timestamp,
    Uuid,
    Json,
    Inet,
    Cidr,
    Enumeration(Vec<String>),
    Set(Vec<String>),
    Unchecked,
}

fn family_of(dialect: &Dialect, column: &ColumnDef, current: &Cell) -> Family {
    match dialect.type_discipline() {
        TypeDiscipline::Declared => declared_family(&column.type_name),
        TypeDiscipline::Affinity => affinity_family(&column.type_name),
        TypeDiscipline::Schemaless => match current {
            Cell::Number { .. } => Family::Float,
            Cell::Bool { .. } => Family::Boolean,
            Cell::Timestamp { .. } => Family::Timestamp,
            _ => Family::Unchecked,
        },
    }
}

/// A type name split into the parts that matter: `int(10) unsigned` is the
/// base `int`, the parameters `10` and the unsigned flag. The parameters keep
/// their case, which an enum's values need.
struct TypeName {
    base: String,
    parameters: Option<String>,
    unsigned: bool,
    array: bool,
}

impl TypeName {
    fn parse(type_name: &str) -> Self {
        let array = type_name.contains('[');
        let without_brackets = type_name.split('[').next().unwrap_or_default();
        let (outside, parameters) = match (without_brackets.find('('), without_brackets.rfind(')'))
        {
            (Some(open), Some(close)) if open < close => (
                format!(
                    "{} {}",
                    &without_brackets[..open],
                    &without_brackets[close + 1..]
                ),
                Some(without_brackets[open + 1..close].to_string()),
            ),
            _ => (without_brackets.to_string(), None),
        };
        let mut unsigned = false;
        let words: Vec<String> = outside
            .to_ascii_lowercase()
            .split_whitespace()
            .filter(|word| match *word {
                "unsigned" => {
                    unsigned = true;
                    false
                }
                "signed" | "zerofill" => false,
                _ => true,
            })
            .map(str::to_string)
            .collect();
        Self {
            base: words.join(" "),
            parameters,
            unsigned,
            array,
        }
    }

    fn numbers(&self) -> Vec<usize> {
        self.parameters
            .as_deref()
            .map(|parameters| {
                parameters
                    .split(',')
                    .map_while(|part| part.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn integer_range(bits: u32, unsigned: bool) -> Family {
    if unsigned {
        Family::Integer {
            min: 0,
            max: (1i128 << bits) - 1,
        }
    } else {
        Family::Integer {
            min: -(1i128 << (bits - 1)),
            max: (1i128 << (bits - 1)) - 1,
        }
    }
}

fn declared_family(type_name: &str) -> Family {
    let name = TypeName::parse(type_name);
    // `{1,2}` and the like: checking every element is the server's job.
    if name.array {
        return Family::Unchecked;
    }
    let numbers = name.numbers();
    match name.base.as_str() {
        "smallint" | "int2" | "smallserial" | "serial2" => integer_range(16, name.unsigned),
        // `tinyint(1)` is how that engine spells its boolean, and it is an
        // integer all the same: its driver returns it as a number and binds
        // what was typed as it is, so `true` is no value and `2` is.
        "tinyint" => integer_range(8, name.unsigned),
        "mediumint" => integer_range(24, name.unsigned),
        "int" | "integer" | "int4" | "serial" | "serial4" => integer_range(32, name.unsigned),
        "bigint" | "int8" | "bigserial" | "serial8" => integer_range(64, name.unsigned),
        "real" | "float" | "float4" | "float8" | "double" | "double precision" => Family::Float,
        "numeric" | "decimal" | "dec" | "fixed" => Family::Decimal {
            integer_digits: match numbers.as_slice() {
                [precision] => Some(*precision),
                [precision, scale] => precision.checked_sub(*scale),
                _ => None,
            },
            unsigned: name.unsigned,
        },
        "boolean" | "bool" => Family::Boolean,
        "char" | "character" | "nchar" => Family::Text {
            max_chars: numbers.first().copied().unwrap_or(1),
        },
        "varchar" | "character varying" | "nvarchar" => match numbers.first() {
            Some(max_chars) => Family::Text {
                max_chars: *max_chars,
            },
            None => Family::Unchecked,
        },
        "date" => Family::Date,
        "time" | "timetz" | "time with time zone" | "time without time zone" => Family::Time,
        "timestamp"
        | "timestamptz"
        | "datetime"
        | "timestamp with time zone"
        | "timestamp without time zone" => Family::Timestamp,
        "uuid" => Family::Uuid,
        "json" | "jsonb" => Family::Json,
        "inet" => Family::Inet,
        "cidr" => Family::Cidr,
        "enum" => name
            .parameters
            .as_deref()
            .map_or(Family::Unchecked, |values| {
                Family::Enumeration(quoted_values(values))
            }),
        "set" => name
            .parameters
            .as_deref()
            .map_or(Family::Unchecked, |values| {
                Family::Set(quoted_values(values))
            }),
        _ => Family::Unchecked,
    }
}

/// The type-affinity rules of a dynamically typed engine: a name with `INT` in
/// it is an integer, one with `REAL`, `FLOA` or `DOUB` a float. A name with
/// `CHAR`, `CLOB` or `TEXT` in it holds text, and every other name is a
/// numeric affinity that stores whatever it is given.
fn affinity_family(type_name: &str) -> Family {
    let name = TypeName::parse(type_name).base;
    if name.contains("int") {
        integer_range(64, false)
    } else if ["char", "clob", "text"]
        .iter()
        .any(|text| name.contains(text))
    {
        Family::Unchecked
    } else if ["real", "floa", "doub"]
        .iter()
        .any(|real| name.contains(real))
    {
        Family::Float
    } else {
        Family::Unchecked
    }
}

/// The values of `'a','b''c'`, a doubled quote being one quote.
fn quoted_values(list: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut characters = list.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\'' {
            continue;
        }
        let mut value = String::new();
        while let Some(inner) = characters.next() {
            if inner == '\'' {
                if characters.peek() == Some(&'\'') {
                    characters.next();
                    value.push('\'');
                } else {
                    break;
                }
            } else {
                value.push(inner);
            }
        }
        values.push(value);
    }
    values
}

/// `words` is whether the engine reads a word where a number or a date goes
/// (see [`Dialect::reads_word_values`]).
fn check(family: &Family, text: &str, words: bool) -> Result<(), String> {
    match family {
        Family::Integer { min, max } => check_integer(text, *min, *max),
        Family::Float => match text.trim().parse::<f64>() {
            Ok(number) if number.is_finite() || words => Ok(()),
            _ => Err("number expected (for example 3.14 or 1e-3)".into()),
        },
        Family::Decimal {
            integer_digits,
            unsigned,
        } => {
            if words && is_number_word(text) {
                return Ok(());
            }
            check_decimal(text, *integer_digits, *unsigned)
        }
        Family::Boolean => {
            let word = text.trim().to_ascii_lowercase();
            let known = matches!(word.as_str(), "true" | "false" | "1" | "0")
                || (words
                    && matches!(
                        word.as_str(),
                        "t" | "f" | "y" | "n" | "yes" | "no" | "on" | "off"
                    ));
            verdict(known, "must be true or false")
        }
        Family::Text { max_chars } => {
            let length = text.chars().count();
            if length > *max_chars {
                Err(format!(
                    "longer than {max_chars} characters (this is {length})"
                ))
            } else {
                Ok(())
            }
        }
        Family::Date => verdict(
            is_date(text.trim()) || (words && is_date_word(text)),
            "not a valid date, use YYYY-MM-DD",
        ),
        Family::Time => verdict(
            is_time(text.trim()) || (words && is_time_word(text)),
            "not a valid time, use HH:MM:SS",
        ),
        Family::Timestamp => verdict(
            is_timestamp(text.trim()) || (words && is_date_word(text)),
            "not a valid date and time, use YYYY-MM-DD HH:MM:SS",
        ),
        Family::Uuid => verdict(
            is_uuid(text.trim()),
            "not a valid UUID, use 8-4-4-4-12 hexadecimal digits",
        ),
        Family::Json => serde_json::from_str::<serde_json::Value>(text)
            .map(|_| ())
            .map_err(|error| format!("not valid JSON: {error}")),
        Family::Inet => check_network(text.trim(), false),
        Family::Cidr => check_network(text.trim(), true),
        Family::Enumeration(values) => {
            let matches = values.iter().any(|value| value.eq_ignore_ascii_case(text));
            verdict(matches, &format!("must be one of {}", quoted_list(values)))
        }
        Family::Set(values) => {
            let all_known = text.split(',').map(str::trim).all(|part| {
                part.is_empty() || values.iter().any(|value| value.eq_ignore_ascii_case(part))
            });
            verdict(
                all_known,
                &format!(
                    "must be some of {}, separated by commas",
                    quoted_list(values)
                ),
            )
        }
        Family::Unchecked => Ok(()),
    }
}

/// `NaN` and the infinities, which parse as numbers and are values only to an
/// engine that spells them out.
fn is_number_word(text: &str) -> bool {
    text.trim()
        .parse::<f64>()
        .is_ok_and(|number| !number.is_finite())
}

/// The words a date or a timestamp may be written as, for the engine that
/// reads them.
fn is_date_word(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "infinity" | "-infinity" | "epoch" | "now" | "today" | "tomorrow" | "yesterday"
    )
}

fn is_time_word(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "now" | "allballs"
    )
}

fn verdict(valid: bool, message: &str) -> Result<(), String> {
    if valid { Ok(()) } else { Err(message.into()) }
}

fn quoted_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("'{value}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_integer(text: &str, min: i128, max: i128) -> Result<(), String> {
    let text = text.trim();
    match text.parse::<i128>() {
        Ok(value) if (min..=max).contains(&value) => Ok(()),
        Ok(_) => Err(format!("out of range (between {min} and {max})")),
        Err(_) => {
            let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
            if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
                Err(format!("out of range (between {min} and {max})"))
            } else if let Some(whole) = whole_number_with_a_point(text)
                && whole
                    .parse::<i128>()
                    .is_ok_and(|value| (min..=max).contains(&value))
            {
                // Some engines round `5.0` into an integer column; a value that
                // changes on its way in is not the one that was typed.
                Err(format!(
                    "whole number expected (between {min} and {max}) — write {whole}"
                ))
            } else {
                Err(format!("whole number expected (between {min} and {max})"))
            }
        }
    }
}

/// `5.0` as `5`: a whole number written with a point and nothing but zeros
/// after it.
fn whole_number_with_a_point(text: &str) -> Option<String> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = rest.split_once('.')?;
    let only_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !only_digits(whole) || !fraction.bytes().all(|byte| byte == b'0') {
        return None;
    }
    Some(format!("{}{whole}", if negative { "-" } else { "" }))
}

/// A number in plain notation, optionally with an exponent: its sign, the
/// digits before the point and whether an exponent was written.
fn decimal_shape(text: &str) -> Option<(bool, usize, bool)> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], Some(&rest[at + 1..])),
        None => (rest, None),
    };
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (mantissa, ""),
    };
    let only_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !only_digits(whole) || !only_digits(fraction) {
        return None;
    }
    let significant = whole.trim_start_matches('0').len();
    Some((negative, significant, exponent.is_some()))
}

fn check_decimal(text: &str, integer_digits: Option<usize>, unsigned: bool) -> Result<(), String> {
    let text = text.trim();
    let Some((negative, whole_digits, has_exponent)) = decimal_shape(text) else {
        return Err("decimal number expected (for example 12.50)".into());
    };
    if unsigned && negative && text.bytes().any(|byte| matches!(byte, b'1'..=b'9')) {
        return Err("must not be negative".into());
    }
    // Digits past the scale are rounded away by the server rather than
    // refused, so only the ones before the point can overflow.
    match integer_digits {
        Some(allowed) if !has_exponent && whole_digits > allowed => Err(format!(
            "too many digits before the decimal point (at most {allowed})"
        )),
        _ => Ok(()),
    }
}

fn has_four_digit_year(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 4 && bytes[..4].iter().all(|byte| byte.is_ascii_digit()) && bytes[4] == b'-'
}

fn is_date(text: &str) -> bool {
    has_four_digit_year(text) && NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

fn is_time(text: &str) -> bool {
    ["%H:%M:%S%.f", "%H:%M", "%H:%M:%S%.f %#z", "%H:%M %#z"]
        .iter()
        .any(|format| NaiveTime::parse_from_str(text, format).is_ok())
}

fn is_timestamp(text: &str) -> bool {
    if !has_four_digit_year(text) {
        return false;
    }
    let local = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ];
    let zoned = [
        "%Y-%m-%d %H:%M:%S%.f %#z",
        "%Y-%m-%dT%H:%M:%S%.f %#z",
        "%Y-%m-%d %H:%M %#z",
        "%Y-%m-%dT%H:%M %#z",
    ];
    is_date(text)
        || local
            .iter()
            .any(|format| NaiveDateTime::parse_from_str(text, format).is_ok())
        || zoned
            .iter()
            .any(|format| DateTime::parse_from_str(text, format).is_ok())
}

/// The forms a UUID is written in: hyphenated, bare, or in braces.
fn is_uuid(text: &str) -> bool {
    let inner = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(text);
    let hex_only = |part: &str| part.bytes().all(|byte| byte.is_ascii_hexdigit());
    let groups: Vec<&str> = inner.split('-').collect();
    match groups.as_slice() {
        [bare] => bare.len() == 32 && hex_only(bare),
        groups => {
            groups.len() == 5
                && groups
                    .iter()
                    .zip([8, 4, 4, 4, 12])
                    .all(|(group, length)| group.len() == length && hex_only(group))
        }
    }
}

fn check_network(text: &str, host_bits_must_be_zero: bool) -> Result<(), String> {
    let (address, prefix) = match text.split_once('/') {
        Some((address, prefix)) => (address.trim(), Some(prefix.trim())),
        None => (text, None),
    };
    let Ok(ip) = address.parse::<IpAddr>() else {
        return Err("not a valid IP address (for example 192.168.0.1 or 10.0.0.0/8)".into());
    };
    let (value, width) = match ip {
        IpAddr::V4(address) => (u128::from(u32::from(address)), 32u32),
        IpAddr::V6(address) => (u128::from(address), 128u32),
    };
    let Some(prefix) = prefix else {
        return Ok(());
    };
    let Ok(prefix) = prefix.parse::<u32>() else {
        return Err(format!("network prefix must be a number from 0 to {width}"));
    };
    if prefix > width {
        return Err(format!("network prefix must be from 0 to {width}"));
    }
    let host_bits = width - prefix;
    if host_bits_must_be_zero && host_bits > 0 && value & (u128::MAX >> (128 - host_bits)) != 0 {
        return Err("has bits set to the right of the network prefix".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::protocol::Capabilities;

    fn dialect(driver: &str) -> Dialect {
        let capabilities = match driver {
            "mysql" => Capabilities {
                identifier_quote: Some("`".into()),
                ..Default::default()
            },
            "mongodb" => Capabilities {
                read_table_template: Some("{}".into()),
                ..Default::default()
            },
            _ => Capabilities::default(),
        };
        Dialect::detect(driver, &capabilities)
    }

    fn column(type_name: &str) -> ColumnDef {
        ColumnDef {
            name: "c".into(),
            type_name: type_name.into(),
            nullable: true,
            primary_key: false,
        }
    }

    fn check_text(driver: &str, type_name: &str, input: &str) -> Result<(), String> {
        validate(
            &dialect(driver),
            &column(type_name),
            &Cell::Null,
            Some(input),
        )
    }

    /// Every row is `(type, accepted inputs, rejected inputs)`.
    fn run_table(driver: &str, table: &[(&str, &[&str], &[&str])]) {
        for (type_name, accepted, rejected) in table {
            for input in *accepted {
                assert_eq!(
                    check_text(driver, type_name, input),
                    Ok(()),
                    "{driver} {type_name}: {input:?} should be accepted"
                );
            }
            for input in *rejected {
                assert!(
                    check_text(driver, type_name, input).is_err(),
                    "{driver} {type_name}: {input:?} should be rejected"
                );
            }
        }
    }

    #[test]
    fn integers_hold_to_their_width_at_the_boundaries() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "smallint",
                &["-32768", "32767", "0", " 5 ", "+7"],
                &["32768", "-32769", "1.5", "", "abc", "1e3", "0x10"],
            ),
            ("int2", &["32767"], &["32768"]),
            (
                "integer",
                &["-2147483648", "2147483647"],
                &["2147483648", "-2147483649"],
            ),
            ("int4", &["2147483647"], &["2147483648"]),
            ("serial", &["1"], &["99999999999"]),
            (
                "bigint",
                &["9223372036854775807", "-9223372036854775808"],
                &["9223372036854775808", "1.0"],
            ),
            ("int8", &["0"], &["x"]),
            ("tinyint", &["127", "-128"], &["128", "-129"]),
            ("tinyint unsigned", &["255", "0"], &["256", "-1"]),
            ("tinyint(3) unsigned", &["255"], &["256"]),
            ("mediumint", &["8388607", "-8388608"], &["8388608"]),
            ("mediumint unsigned", &["16777215"], &["16777216", "-1"]),
            (
                "int(10) unsigned zerofill",
                &["4294967295"],
                &["4294967296", "-1"],
            ),
            (
                "bigint unsigned",
                &["18446744073709551615"],
                &["18446744073709551616"],
            ),
        ];
        run_table("postgres", table);
        run_table("mysql", table);
    }

    #[test]
    fn an_integer_message_says_the_range_and_tells_overflow_from_nonsense() {
        assert_eq!(
            check_text("postgres", "integer", "abc"),
            Err("whole number expected (between -2147483648 and 2147483647)".into())
        );
        assert_eq!(
            check_text("postgres", "integer", "3000000000"),
            Err("out of range (between -2147483648 and 2147483647)".into())
        );
        assert_eq!(
            check_text(
                "postgres",
                "bigint",
                "99999999999999999999999999999999999999999999"
            ),
            Err("out of range (between -9223372036854775808 and 9223372036854775807)".into())
        );
    }

    #[test]
    fn a_dynamically_typed_engine_takes_every_integer_as_64_bit_and_ignores_the_rest() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "INTEGER",
                &["9223372036854775807", "-9223372036854775808"],
                &["9223372036854775808", "x", "1.5"],
            ),
            ("INT", &["3000000000"], &[]),
            ("BIGINT", &["1"], &["abc"]),
            ("REAL", &["1.5", "-2e3", "7"], &["abc", ""]),
            ("DOUBLE PRECISION", &["0.1"], &["x"]),
            ("FLOAT", &["1"], &["x"]),
            // A declared type that is only a hint: whatever it holds, it holds.
            ("TEXT", &["anything", ""], &[]),
            ("NUMERIC", &["abc"], &[]),
            ("DATE", &["last tuesday"], &[]),
            ("BOOLEAN", &["maybe"], &[]),
            ("VARCHAR(2)", &["far too long"], &[]),
            ("JSON", &["{not json"], &[]),
        ];
        run_table("sqlite", table);
    }

    #[test]
    fn floats_and_decimals_are_numbers_and_decimals_keep_their_digits_before_the_point() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "real",
                &["1", "-1.5", ".5", "5.", "1e-3", "2E+8", " 3 "],
                &["abc", "", "1,5", "1.2.3"],
            ),
            ("double precision", &["3.14"], &["pi"]),
            ("float(24)", &["1.5"], &["x"]),
            (
                "numeric(5,2)",
                &["999.99", "-999.99", "0.001", "12.3456", "1e9", "000123"],
                &["10000", "-10000", "abc", "", "-", "1e"],
            ),
            ("numeric(5,2)", &["999"], &["1000.00"]),
            ("decimal(10)", &["9999999999"], &["10000000000"]),
            ("decimal(10,0)", &["9999999999"], &["10000000000"]),
            (
                "numeric",
                &["123456789012345678901234567890.123456789"],
                &["abc"],
            ),
            (
                "decimal(5,2) unsigned",
                &["0", "-0", "-0.00", "999.99"],
                &["-1", "-0.01", "1000"],
            ),
        ];
        run_table("postgres", table);
        run_table("mysql", table);
        assert_eq!(
            check_text("postgres", "numeric(5,2)", "1000"),
            Err("too many digits before the decimal point (at most 3)".into())
        );
        assert_eq!(
            check_text("postgres", "numeric", "x"),
            Err("decimal number expected (for example 12.50)".into())
        );
        assert_eq!(
            check_text("mysql", "decimal(5,2) unsigned", "-1"),
            Err("must not be negative".into())
        );
    }

    /// A boolean column as the server that has one reads it: the words the
    /// text-to-boolean conversion takes, in any case, with space around.
    #[test]
    fn a_boolean_column_takes_every_spelling_its_engine_reads() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "boolean",
                &[
                    "true", "FALSE", "1", "0", " True ", "t", "F", "yes", "No", "on", "OFF", "y",
                    "n",
                ],
                &["2", "", "maybe", "-1", "yes please"],
            ),
            ("bool", &["false", "t"], &["nope"]),
        ];
        run_table("postgres", table);
        assert_eq!(
            check_text("postgres", "boolean", "maybe"),
            Err("must be true or false".into())
        );
    }

    /// The other engine has no boolean: its `BOOLEAN` is a `tinyint(1)`, which
    /// its driver sends back as a number and to which it binds the typed text
    /// as it is -- so `true` is not a value, and `2` to `127` are.
    #[test]
    fn a_tinyint_is_an_8_bit_integer_whatever_its_display_width() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "tinyint(1)",
                &["1", "0", "2", "100", "127", "-128", " 1 "],
                &["true", "false", "x", "128", "-129", "1.5", ""],
            ),
            (
                "tinyint(1) unsigned",
                &["0", "1", "255"],
                &["-1", "256", "true"],
            ),
            ("tinyint(4)", &["100"], &["true"]),
            ("tinyint", &["-128"], &["128"]),
        ];
        run_table("mysql", table);
        assert_eq!(
            check_text("mysql", "tinyint(1)", "true"),
            Err("whole number expected (between -128 and 127)".into())
        );
    }

    /// What the validator takes for a flag, and what the driver then does with
    /// it, agree: the driver's cell for the column is a number, so the check
    /// the cell gets is the integer check.
    #[test]
    fn the_check_for_a_flag_column_follows_the_cell_its_driver_returns() {
        let flag = Cell::Number { value: "1".into() };
        assert_eq!(
            validate(&dialect("mysql"), &column("tinyint(1)"), &flag, Some("0")),
            Ok(())
        );
        assert!(
            validate(
                &dialect("mysql"),
                &column("tinyint(1)"),
                &flag,
                Some("false")
            )
            .is_err()
        );
        let real_boolean = Cell::Bool { value: true };
        assert_eq!(
            validate(
                &dialect("postgres"),
                &column("boolean"),
                &real_boolean,
                Some("off")
            ),
            Ok(())
        );
    }

    /// `inf` and `nan` parse as numbers in Rust and are refused by every
    /// engine here but the one that spells them out for float columns.
    #[test]
    fn not_a_number_and_infinity_are_a_float_only_where_the_engine_reads_them() {
        let words: &[&str] = &[
            "NaN",
            "nan",
            "Infinity",
            "-infinity",
            "inf",
            "+Inf",
            " NaN ",
        ];
        let table: &[(&str, &[&str], &[&str])] = &[("double precision", &["1.5", "-2e3"], words)];
        run_table("mysql", table);
        run_table("sqlite", &[("REAL", &["1.5"], words)]);
        let table: &[(&str, &[&str], &[&str])] = &[("real", words, &["abc", "1e", "inf inity"])];
        run_table("postgres", table);
        run_table("postgres", &[("float8", &["Infinity", "1"], &["x"])]);
        // The document store's driver refuses them too.
        let number = Cell::Number { value: "5".into() };
        for word in ["NaN", "inf"] {
            assert!(
                validate(&dialect("mongodb"), &column(""), &number, Some(word)).is_err(),
                "{word}"
            );
        }
    }

    #[test]
    fn a_decimal_may_be_not_a_number_only_where_the_engine_reads_the_word() {
        run_table(
            "postgres",
            &[("numeric(5,2)", &["NaN", "nan", "Infinity"], &["abc"])],
        );
        run_table("mysql", &[("decimal(5,2)", &["1.5"], &["NaN", "Infinity"])]);
    }

    #[test]
    fn the_special_dates_are_dates_only_where_the_engine_reads_them() {
        let words: &[&str] = &[
            "infinity",
            "-infinity",
            "Infinity",
            "now",
            "today",
            "tomorrow",
            "yesterday",
            "epoch",
        ];
        for type_name in [
            "date",
            "timestamp",
            "timestamptz",
            "timestamp without time zone",
        ] {
            run_table("postgres", &[(type_name, words, &["later", "nowhere"])]);
            run_table("mysql", &[(type_name, &["2026-11-17"], words)]);
            run_table("sqlite", &[(type_name, &["whatever"], &[])]);
        }
        run_table("postgres", &[("time", &["now", "allballs"], &["today"])]);
        run_table("mysql", &[("time", &["10:30"], &["now", "allballs"])]);
    }

    /// MySQL rounds `5.0` for an integer column, and this check refuses it all
    /// the same: a value that changes on its way in is not the one that was
    /// typed. The message says what to type.
    #[test]
    fn an_integer_written_with_a_point_is_refused_and_told_how_to_write_it() {
        assert_eq!(
            check_text("mysql", "int", "5.0"),
            Err("whole number expected (between -2147483648 and 2147483647) — write 5".into())
        );
        assert_eq!(
            check_text("postgres", "smallint", "-12.00"),
            Err("whole number expected (between -32768 and 32767) — write -12".into())
        );
        assert_eq!(
            check_text("postgres", "integer", "5.5"),
            Err("whole number expected (between -2147483648 and 2147483647)".into()),
            "nothing to suggest for a value that is not whole"
        );
        assert_eq!(
            check_text("postgres", "integer", "1e3"),
            Err("whole number expected (between -2147483648 and 2147483647)".into())
        );
    }

    #[test]
    fn a_bounded_text_counts_characters_not_bytes() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "varchar(5)",
                &["", "hello", "héllo", "tên đ"],
                &["toolong", "hello!"],
            ),
            ("character varying(3)", &["abc"], &["abcd"]),
            ("char(2)", &["ab"], &["abc"]),
            ("char", &["a"], &["ab"]),
            (
                "varchar",
                &["any length at all, with nothing to hold it to"],
                &[],
            ),
            ("text", &["x"], &[]),
            ("longtext", &["x"], &[]),
        ];
        run_table("postgres", table);
        run_table("mysql", table);
        assert_eq!(
            check_text("postgres", "varchar(255)", &"a".repeat(300)),
            Err("longer than 255 characters (this is 300)".into())
        );
        assert_eq!(check_text("postgres", "varchar(3)", "日本語"), Ok(()));
        assert!(check_text("postgres", "varchar(3)", "日本語字").is_err());
    }

    #[test]
    fn dates_times_and_timestamps_take_the_iso_forms() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "date",
                &["2026-11-17", "2024-02-29", " 2026-01-05 "],
                &[
                    "2026-13-01",
                    "2025-02-29",
                    "17/11/2026",
                    "2026-11-31",
                    "26-11-17",
                    "",
                    "2026-11-17 10:00",
                ],
            ),
            (
                "time",
                &["10:30", "10:30:15", "23:59:59.999", "00:00:00"],
                &["24:00:01", "10:61", "10", "", "noon", "2026-11-17"],
            ),
            (
                "time with time zone",
                &["10:30:00+09", "10:30:00 +09:00", "10:30:00Z"],
                &["10:30:00+99:99x"],
            ),
            (
                "timestamp",
                &[
                    "2026-11-17 10:30:00",
                    "2026-11-17T10:30:00",
                    "2026-11-17 10:30",
                    "2026-11-17",
                    "2026-11-17 10:30:00.123456",
                    "2026-11-17 10:30:00+09",
                    "2026-11-17T10:30:00Z",
                    "2026-11-17 10:30:00 +09:00",
                ],
                &[
                    "2026-11-17 25:00:00",
                    "2026-02-30 10:00:00",
                    "last tuesday",
                    "",
                    "2026/11/17 10:00:00",
                    "10:30:00",
                ],
            ),
            (
                "timestamp with time zone",
                &["2026-11-17 10:30:00+07"],
                &["nope"],
            ),
            (
                "timestamptz",
                &["2026-11-17T10:30:00.5+07:00"],
                &["2026-11-17 10:30:00 PST?"],
            ),
            (
                "datetime",
                &["2026-11-17 10:30:00"],
                &["2026-11-17 10:30:99"],
            ),
        ];
        run_table("postgres", table);
        run_table("mysql", table);
        assert_eq!(
            check_text("postgres", "date", "17/11/2026"),
            Err("not a valid date, use YYYY-MM-DD".into())
        );
        assert_eq!(
            check_text("postgres", "timestamp", "x"),
            Err("not a valid date and time, use YYYY-MM-DD HH:MM:SS".into())
        );
        assert_eq!(
            check_text("postgres", "time", "x"),
            Err("not a valid time, use HH:MM:SS".into())
        );
    }

    #[test]
    fn uuids_take_the_hyphenated_bare_and_braced_forms() {
        let table: &[(&str, &[&str], &[&str])] = &[(
            "uuid",
            &[
                "123e4567-e89b-12d3-a456-426614174000",
                "123E4567-E89B-12D3-A456-426614174000",
                "123e4567e89b12d3a456426614174000",
                "{123e4567-e89b-12d3-a456-426614174000}",
            ],
            &[
                "",
                "not-a-uuid",
                "123e4567-e89b-12d3-a456-42661417400",
                "123e4567-e89b-12d3-a456-4266141740000",
                "123e4567-e89b-12d3-a456-42661417400g",
                "123e4567e89b12d3a45642661417400",
            ],
        )];
        run_table("postgres", table);
        assert_eq!(
            check_text("postgres", "uuid", "x"),
            Err("not a valid UUID, use 8-4-4-4-12 hexadecimal digits".into())
        );
    }

    #[test]
    fn json_columns_take_any_json_value_and_say_where_it_breaks() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "json",
                &[
                    "{}",
                    "[1, 2]",
                    "\"text\"",
                    "12",
                    "null",
                    "true",
                    r#"{"a": {"b": [1]}}"#,
                ],
                &["", "{", "{'a': 1}", "undefined", "[1,]", r#"{"a": 1} x"#],
            ),
            ("jsonb", &["{}"], &["{"]),
        ];
        run_table("postgres", table);
        run_table("mysql", table);
        let message = check_text("postgres", "jsonb", "{\"a\": }").expect_err("a broken document");
        assert!(message.starts_with("not valid JSON: "), "{message}");
        assert!(
            message.contains("line 1"),
            "names where it broke: {message}"
        );
    }

    #[test]
    fn network_addresses_are_checked_and_a_cidr_has_no_host_bits() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "inet",
                &[
                    "192.168.0.1",
                    "192.168.0.1/24",
                    "::1",
                    "2001:db8::1/64",
                    "10.0.0.5/32",
                    "0.0.0.0/0",
                    " 10.0.0.1 ",
                ],
                &[
                    "",
                    "256.0.0.1",
                    "192.168.0.1/33",
                    "::1/129",
                    "192.168.0.1/x",
                    "host",
                    "192.168.0",
                ],
            ),
            (
                "cidr",
                &[
                    "192.168.0.0/24",
                    "10.0.0.0/8",
                    "2001:db8::/32",
                    "0.0.0.0/0",
                    "10.0.0.1/32",
                    "::/0",
                ],
                &[
                    "192.168.0.1/24",
                    "10.1.0.0/8",
                    "2001:db8::1/32",
                    "10.0.0.0/33",
                    "x",
                ],
            ),
        ];
        run_table("postgres", table);
        assert_eq!(
            check_text("postgres", "cidr", "192.168.0.1/24"),
            Err("has bits set to the right of the network prefix".into())
        );
        assert_eq!(
            check_text("postgres", "inet", "300.1.1.1"),
            Err("not a valid IP address (for example 192.168.0.1 or 10.0.0.0/8)".into())
        );
        assert_eq!(
            check_text("postgres", "inet", "10.0.0.1/40"),
            Err("network prefix must be from 0 to 32".into())
        );
    }

    #[test]
    fn an_enum_must_be_one_of_its_values_and_a_set_some_of_them() {
        let table: &[(&str, &[&str], &[&str])] = &[
            (
                "enum('small','medium','large')",
                &["small", "LARGE"],
                &["huge", "", "small,large"],
            ),
            ("enum('it''s','a,b')", &["it's", "a,b"], &["it", "a"]),
            (
                "set('a','b','c')",
                &["a", "a,c", "a, b, c", ""],
                &["d", "a,d"],
            ),
        ];
        run_table("mysql", table);
        assert_eq!(
            check_text("mysql", "enum('small','medium')", "huge"),
            Err("must be one of 'small', 'medium'".into())
        );
        assert_eq!(
            check_text("mysql", "set('a','b')", "z"),
            Err("must be some of 'a', 'b', separated by commas".into())
        );
    }

    #[test]
    fn a_type_that_is_not_known_is_accepted_whatever_is_typed() {
        for driver in ["postgres", "mysql"] {
            for type_name in [
                "money",
                "bytea",
                "geometry",
                "mood",
                "int4[]",
                "text[]",
                "integer[]",
                "macaddr",
                "tsvector",
                "xml",
                "",
                "interval",
                "bit(3)",
                "year",
                "point",
            ] {
                assert_eq!(
                    check_text(driver, type_name, "anything at all ;'\""),
                    Ok(()),
                    "{driver} {type_name}"
                );
            }
        }
    }

    #[test]
    fn a_type_name_is_read_whatever_its_case_spacing_or_parameters() {
        assert!(check_text("postgres", "INTEGER", "x").is_err());
        assert!(check_text("postgres", "  Integer  ", "x").is_err());
        assert!(check_text("postgres", "VARCHAR( 3 )", "abcd").is_err());
        assert!(check_text("postgres", "NUMERIC(5, 2)", "1000").is_err());
        assert_eq!(check_text("postgres", "NUMERIC(5, 2)", "999"), Ok(()));
        assert!(check_text("mysql", "INT UNSIGNED", "-1").is_err());
        assert!(check_text("mysql", "int(11) UNSIGNED", "-1").is_err());
        // A type name that does not close its parentheses is not guessed at.
        assert_eq!(check_text("postgres", "varchar(3", "abcd"), Ok(()));
    }

    #[test]
    fn null_is_refused_for_a_not_null_column_and_nowhere_else() {
        for driver in ["postgres", "mysql", "sqlite", "mongodb"] {
            let dialect = dialect(driver);
            let mut not_null = column("integer");
            not_null.nullable = false;
            assert_eq!(
                validate(&dialect, &not_null, &Cell::Null, None),
                Err("cannot be NULL".into()),
                "{driver}"
            );
            assert_eq!(
                validate(&dialect, &column("integer"), &Cell::Null, None),
                Ok(()),
                "{driver}"
            );
            // A value is not NULL because the column says it cannot be.
            assert_eq!(
                validate(&dialect, &not_null, &Cell::Null, Some("5")),
                Ok(()),
                "{driver}"
            );
        }
        // The empty string is a value, not a NULL.
        assert_eq!(check_text("postgres", "text", ""), Ok(()));
    }

    #[test]
    fn a_store_without_column_types_goes_by_the_kind_of_the_value_replaced() {
        let mongo = dialect("mongodb");
        let field = column("");
        let number = Cell::Number { value: "5".into() };
        let boolean = Cell::Bool { value: true };
        let stamp = Cell::Timestamp {
            value: "2026-11-17T10:00:00Z".into(),
        };
        let text = Cell::Text { value: "x".into() };

        let cases: &[(&Cell, &[&str], &[&str])] = &[
            (&number, &["1", "-2.5", "1e3", "0"], &["abc", ""]),
            (&boolean, &["true", "false"], &["yes", "2"]),
            (
                &stamp,
                &["2026-11-17T10:00:00Z", "2026-11-17"],
                &["tomorrow"],
            ),
            (&text, &["anything", ""], &[]),
            (&Cell::Null, &["anything"], &[]),
        ];
        for (current, accepted, rejected) in cases {
            for input in *accepted {
                assert_eq!(
                    validate(&mongo, &field, current, Some(input)),
                    Ok(()),
                    "{current:?} takes {input:?}"
                );
            }
            for input in *rejected {
                assert!(
                    validate(&mongo, &field, current, Some(input)).is_err(),
                    "{current:?} refuses {input:?}"
                );
            }
        }
        // The column's name for its type means nothing to a store with none.
        let odd = ColumnDef {
            type_name: "integer".into(),
            ..field
        };
        assert_eq!(validate(&mongo, &odd, &text, Some("abc")), Ok(()));
    }

    #[test]
    fn a_key_column_a_binary_value_and_a_nested_document_are_not_editable() {
        let sql = dialect("postgres");
        let mongo = dialect("mongodb");
        let mut key = column("integer");
        key.primary_key = true;
        let number = Cell::Number { value: "1".into() };
        let json = Cell::Json { value: "{}".into() };
        let binary = Cell::Binary { byte_len: 4 };

        assert_eq!(
            editability(&sql, &key, &number),
            Err(NotEditable::PrimaryKey)
        );
        assert_eq!(
            editability(&mongo, &key, &number),
            Err(NotEditable::PrimaryKey)
        );
        assert_eq!(
            editability(&sql, &column("bytea"), &binary),
            Err(NotEditable::Binary)
        );
        assert_eq!(
            editability(&mongo, &column(""), &binary),
            Err(NotEditable::Binary)
        );
        assert_eq!(
            editability(&mongo, &column(""), &json),
            Err(NotEditable::NestedDocument)
        );
        assert_eq!(
            editability(&sql, &column("jsonb"), &json),
            Ok(()),
            "json text is editable"
        );
        assert_eq!(editability(&sql, &column("integer"), &number), Ok(()));
        assert_eq!(editability(&sql, &column("text"), &Cell::Null), Ok(()));
        assert_eq!(
            editability(&sql, &key, &binary),
            Err(NotEditable::PrimaryKey),
            "the key is the reason given first"
        );
    }

    #[test]
    fn every_refusal_has_a_reason_to_show() {
        assert_eq!(NotEditable::PrimaryKey.reason(), "part of the primary key");
        for reason in [NotEditable::Binary, NotEditable::NestedDocument] {
            assert!(reason.reason().ends_with("cannot be edited here"));
        }
    }

    #[test]
    fn enum_values_are_read_with_doubled_quotes_and_commas_inside() {
        assert_eq!(
            quoted_values("'a','b''c','d,e',''"),
            vec!["a", "b'c", "d,e", ""]
        );
        assert!(quoted_values("").is_empty());
    }
}
