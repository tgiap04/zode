//! Turning what a person typed into a cell back into BSON.
//!
//! A MongoDB field has no declared type -- the type is whatever the document
//! already holds -- so the target type is read off the document being edited
//! rather than off a schema. Everything here is pure so that every refusal can
//! be tested without a server.
//!
//! Values are only ever built as BSON values. Nothing typed is parsed as JSON
//! or extended JSON, so no text can introduce an operator.

use mongodb::bson::{Bson, DateTime, Document, oid::ObjectId};

/// Why a name cannot be a field of an `update_rows` change, or `None`.
///
/// `$`-prefixed names are operators and a `.` is a path into a nested
/// document: either would let a column name reach somewhere the grid never
/// showed.
pub fn field_name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("a field name cannot be empty".to_string());
    }
    if name.starts_with('$') {
        return Some(format!(
            "`{name}` starts with `$`, which MongoDB reads as an operator"
        ));
    }
    if name.contains('.') {
        return Some(format!(
            "`{name}` contains `.`; only top-level fields can be edited"
        ));
    }
    if name.contains('\0') {
        return Some("a field name cannot contain a NUL character".to_string());
    }
    None
}

/// The text converted to the type `current` holds, or a sentence naming the
/// type that was expected.
///
/// `current` is `None` for a field nothing is known about, and a field holding
/// null says nothing about what its type was meant to be: both take a string,
/// the only type that accepts anything typed. A caller that can tell what the
/// rest of the collection holds there passes that instead (see
/// [`dominant_type`]).
pub fn convert(text: &str, current: Option<&Bson>) -> Result<Bson, String> {
    let trimmed = text.trim();
    match current {
        None | Some(Bson::Null | Bson::Undefined | Bson::String(_)) => {
            Ok(Bson::String(text.to_string()))
        }
        Some(Bson::Int32(_)) => trimmed
            .parse()
            .map(Bson::Int32)
            .map_err(|_| "expected a whole number between -2147483648 and 2147483647".to_string()),
        Some(Bson::Int64(_)) => trimmed
            .parse()
            .map(Bson::Int64)
            .map_err(|_| "expected a whole number that fits in 64 bits".to_string()),
        Some(Bson::Double(_)) => trimmed
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(Bson::Double)
            .ok_or_else(|| "expected a number like 3.14".to_string()),
        Some(Bson::Decimal128(_)) => trimmed
            .parse()
            .map(Bson::Decimal128)
            .map_err(|_| "expected a decimal number like 19.99".to_string()),
        Some(Bson::Boolean(_)) => match trimmed.to_ascii_lowercase().as_str() {
            "true" => Ok(Bson::Boolean(true)),
            "false" => Ok(Bson::Boolean(false)),
            _ => Err("expected true or false".to_string()),
        },
        Some(Bson::DateTime(_)) => DateTime::parse_rfc3339_str(trimmed)
            .map(Bson::DateTime)
            .map_err(|_| "expected a date like 2026-11-06T07:20:07Z".to_string()),
        Some(Bson::ObjectId(_)) => ObjectId::parse_str(trimmed)
            .map(Bson::ObjectId)
            .map_err(|_| {
                "expected a 24-character hex id like 65a1f2c3d4e5f60718293a4b".to_string()
            }),
        Some(other) => Err(format!(
            "a field holding {} cannot be edited as text",
            super::values::type_name(other)
        )),
    }
}

/// The `$type` alias the server reads for a value's type.
///
/// Not [`values::type_name`], which names types for a column heading and folds
/// `undefined` into `null` and both kinds of JavaScript into one: a filter that
/// says `null` for a value stored as `undefined` matches nothing.
pub fn type_alias(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::Binary(_) => "binData",
        Bson::Undefined => "undefined",
        Bson::ObjectId(_) => "objectId",
        Bson::Boolean(_) => "bool",
        Bson::DateTime(_) => "date",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::DbPointer(_) => "dbPointer",
        Bson::JavaScriptCode(_) => "javascript",
        Bson::Symbol(_) => "symbol",
        Bson::JavaScriptCodeWithScope(_) => "javascriptWithScope",
        Bson::Int32(_) => "int",
        Bson::Timestamp(_) => "timestamp",
        Bson::Int64(_) => "long",
        Bson::Decimal128(_) => "decimal",
        Bson::MinKey => "minKey",
        Bson::MaxKey => "maxKey",
    }
}

/// A value of the type `field` mostly holds across `sample`, nulls ignored.
///
/// What a field that the document being edited lacks (or holds as null) should
/// become: its own value says nothing about the type, but the rest of the
/// collection does. A tie goes to the type seen first, and a field nobody in the
/// sample holds gets `None`, which [`convert`] reads as a string.
pub fn dominant_type<'a>(field: &str, sample: &'a [Document]) -> Option<&'a Bson> {
    let mut tally: Vec<(&'static str, usize, &'a Bson)> = Vec::new();
    for value in sample.iter().filter_map(|document| document.get(field)) {
        if matches!(value, Bson::Null | Bson::Undefined) {
            continue;
        }
        let alias = type_alias(value);
        match tally.iter_mut().find(|(seen, ..)| *seen == alias) {
            Some(entry) => entry.1 += 1,
            None => tally.push((alias, 1, value)),
        }
    }
    let mut best: Option<(usize, &'a Bson)> = None;
    for (_, count, exemplar) in tally {
        if best.is_none_or(|(most, _)| count > most) {
            best = Some((count, exemplar));
        }
    }
    best.map(|(_, exemplar)| exemplar)
}

/// Every `_id` the displayed key text could stand for, in the order to try.
///
/// The grid shows an `_id` as plain text, so its stored type is lost: a 24-hex
/// string is an ObjectId far more often than not, then comes an integer, and a
/// string is what is left. The caller looks each one up and accepts the key
/// only when exactly one document answers.
pub fn id_candidates(text: &str) -> Vec<Bson> {
    let mut candidates = Vec::with_capacity(3);
    if let Ok(id) = ObjectId::parse_str(text) {
        candidates.push(Bson::ObjectId(id));
    }
    if let Ok(number) = text.parse::<i64>() {
        // An Int64 matches a stored Int32 of the same value, so one candidate
        // covers both integer widths.
        candidates.push(Bson::Int64(number));
    }
    candidates.push(Bson::String(text.to_string()));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::Decimal128;

    fn text_of(value: &str, current: Bson) -> Result<Bson, String> {
        convert(value, Some(&current))
    }

    #[test]
    fn an_int_field_takes_a_whole_number() {
        assert_eq!(text_of("42", Bson::Int32(1)), Ok(Bson::Int32(42)));
        assert_eq!(text_of(" -7 ", Bson::Int32(1)), Ok(Bson::Int32(-7)));
    }

    #[test]
    fn an_int_field_refuses_what_is_not_a_whole_number_that_fits() {
        for bad in ["4.5", "abc", "", "2147483648"] {
            let message = text_of(bad, Bson::Int32(1)).expect_err(bad);
            assert!(message.contains("whole number"), "{bad}: {message}");
        }
    }

    #[test]
    fn a_long_field_keeps_every_digit() {
        assert_eq!(
            text_of("9007199254740993", Bson::Int64(1)),
            Ok(Bson::Int64(9_007_199_254_740_993))
        );
        assert!(text_of("9223372036854775808", Bson::Int64(1)).is_err());
    }

    #[test]
    fn a_double_field_takes_a_finite_number() {
        assert_eq!(text_of("3.25", Bson::Double(1.0)), Ok(Bson::Double(3.25)));
        assert_eq!(
            text_of("-1e3", Bson::Double(1.0)),
            Ok(Bson::Double(-1000.0))
        );
        for bad in ["abc", "NaN", "inf", ""] {
            let message = text_of(bad, Bson::Double(1.0)).expect_err(bad);
            assert!(message.contains("number"), "{bad}: {message}");
        }
    }

    #[test]
    fn a_decimal_field_keeps_its_digits() {
        let converted = text_of("19.990", Bson::Decimal128(Decimal128::from_bytes([0; 16])));
        let Ok(Bson::Decimal128(value)) = converted else {
            panic!("expected a decimal, got {converted:?}");
        };
        assert_eq!(value.to_string(), "19.990");
        let message = text_of("ten", Bson::Decimal128(Decimal128::from_bytes([0; 16])))
            .expect_err("not a number");
        assert!(message.contains("decimal"), "{message}");
    }

    #[test]
    fn a_bool_field_takes_true_or_false_only() {
        assert_eq!(
            text_of("true", Bson::Boolean(false)),
            Ok(Bson::Boolean(true))
        );
        assert_eq!(
            text_of("False", Bson::Boolean(true)),
            Ok(Bson::Boolean(false))
        );
        for bad in ["yes", "1", ""] {
            let message = text_of(bad, Bson::Boolean(true)).expect_err(bad);
            assert!(message.contains("true or false"), "{bad}: {message}");
        }
    }

    #[test]
    fn a_date_field_takes_rfc3339_and_names_the_shape_otherwise() {
        let converted = text_of("2026-11-06T07:20:07Z", Bson::DateTime(DateTime::now()));
        assert_eq!(
            converted,
            Ok(Bson::DateTime(
                DateTime::parse_rfc3339_str("2026-11-06T07:20:07Z").expect("fixture")
            ))
        );
        let message = text_of("06/11/2026", Bson::DateTime(DateTime::now())).expect_err("not iso");
        assert!(message.contains("2026-11-06T07:20:07Z"), "{message}");
    }

    #[test]
    fn an_object_id_field_takes_24_hex_characters() {
        let id = ObjectId::parse_str("65a1f2c3d4e5f60718293a4b").expect("fixture");
        assert_eq!(
            text_of("65a1f2c3d4e5f60718293a4b", Bson::ObjectId(ObjectId::new())),
            Ok(Bson::ObjectId(id))
        );
        let message = text_of("65a1", Bson::ObjectId(ObjectId::new())).expect_err("too short");
        assert!(message.contains("24"), "{message}");
    }

    #[test]
    fn a_string_field_keeps_the_text_exactly() {
        assert_eq!(
            text_of("  padded $set  ", Bson::String("x".into())),
            Ok(Bson::String("  padded $set  ".into()))
        );
        assert_eq!(
            text_of("", Bson::String("x".into())),
            Ok(Bson::String(String::new()))
        );
    }

    #[test]
    fn a_missing_or_null_field_takes_a_string() {
        assert_eq!(convert("5", None), Ok(Bson::String("5".into())));
        assert_eq!(text_of("5", Bson::Null), Ok(Bson::String("5".into())));
    }

    #[test]
    fn a_field_that_holds_structure_is_not_edited_as_text() {
        for current in [
            Bson::Document(mongodb::bson::doc! { "a": 1 }),
            Bson::Array(vec![Bson::Int32(1)]),
            Bson::Binary(mongodb::bson::Binary {
                subtype: mongodb::bson::spec::BinarySubtype::Generic,
                bytes: vec![1],
            }),
        ] {
            let message = text_of("{\"$set\": 1}", current).expect_err("not editable as text");
            assert!(message.contains("cannot be edited"), "{message}");
        }
    }

    #[test]
    fn field_names_that_reach_operators_or_paths_are_refused() {
        assert_eq!(field_name_problem("email"), None);
        assert_eq!(field_name_problem("first name"), None);
        for bad in ["$set", "a.b", "", "a\0b"] {
            assert!(field_name_problem(bad).is_some(), "{bad:?}");
        }
    }

    #[test]
    fn a_key_that_looks_like_an_object_id_is_tried_as_one_first() {
        let candidates = id_candidates("65a1f2c3d4e5f60718293a4b");
        let id = ObjectId::parse_str("65a1f2c3d4e5f60718293a4b").expect("fixture");
        assert_eq!(
            candidates,
            vec![
                Bson::ObjectId(id),
                Bson::String("65a1f2c3d4e5f60718293a4b".into())
            ]
        );
    }

    #[test]
    fn an_integer_key_is_tried_as_a_number_before_a_string() {
        assert_eq!(
            id_candidates("42"),
            vec![Bson::Int64(42), Bson::String("42".into())]
        );
    }

    #[test]
    fn any_other_key_is_only_a_string() {
        assert_eq!(id_candidates("user-7"), vec![Bson::String("user-7".into())]);
        assert_eq!(id_candidates(""), vec![Bson::String(String::new())]);
    }

    #[test]
    fn a_field_the_collection_mostly_holds_as_a_number_is_a_number_here_too() {
        use mongodb::bson::doc;
        let sample = vec![
            doc! { "n": "oops" },
            doc! { "n": 3 },
            doc! { "other": 1 },
            doc! { "n": 4 },
            doc! { "n": Bson::Null },
        ];
        assert_eq!(dominant_type("n", &sample), Some(&Bson::Int32(3)));
        let converted = convert("5", dominant_type("n", &sample));
        assert_eq!(converted, Ok(Bson::Int32(5)));
        assert!(convert("five", dominant_type("n", &sample)).is_err());
    }

    #[test]
    fn a_field_nobody_holds_takes_a_string_and_a_tie_goes_to_the_first_seen() {
        use mongodb::bson::doc;
        let sample = vec![doc! { "a": 1 }, doc! { "n": Bson::Null }, doc! { "b": 2 }];
        assert_eq!(dominant_type("n", &sample), None);
        assert_eq!(dominant_type("missing", &[]), None);
        assert_eq!(
            convert("x", dominant_type("n", &sample)),
            Ok(Bson::String("x".into()))
        );
        let tied = vec![doc! { "t": "s" }, doc! { "t": 7 }];
        assert_eq!(dominant_type("t", &tied), Some(&Bson::String("s".into())));
    }

    #[test]
    fn int_and_long_are_different_types_to_the_server_and_so_to_the_sample() {
        use mongodb::bson::doc;
        let sample = vec![doc! { "v": 1_i64 }, doc! { "v": 2_i64 }, doc! { "v": 3 }];
        assert_eq!(dominant_type("v", &sample), Some(&Bson::Int64(1)));
    }
}
