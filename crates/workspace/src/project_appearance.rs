//! How a project looks in the sidebar: its own initials and its own colour.
//!
//! Pure functions, deliberately. Every off-by-one and every unreadable-text bug
//! in this feature lives in the conversions here, and this is the only place
//! they can be tested without a window.

use gpui::Hsla;

/// The most initials an avatar can draw.
///
/// The square is drawn for two characters. Accepting ten and letting the label
/// overflow would be this module's fault, not the typist's, so the cut happens
/// here — at the one door every caller goes through.
pub const MAX_INITIALS: usize = 2;

/// Trims and caps what someone typed, and reads an empty string as "go back to
/// the default" rather than as an empty label.
pub fn sanitize_initials(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_INITIALS).collect())
}

/// `#RRGGBB`, the form the record on disk carries.
///
/// Stored as text rather than as the three floats of an `Hsla`: a float
/// round-trip through JSON drifts, and a colour that comes back one bit off
/// every launch is a bug nobody can see but everybody's diff can.
pub fn colour_to_hex(colour: Hsla) -> String {
    let rgba = gpui::Rgba::from(colour);
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.r * 255.).round().clamp(0., 255.) as u8,
        (rgba.g * 255.).round().clamp(0., 255.) as u8,
        (rgba.b * 255.).round().clamp(0., 255.) as u8,
    )
}

/// Reads a hex colour from a record, refusing rather than guessing.
///
/// The string comes off disk, where anything can be written by hand, so a
/// failure here is expected input rather than a bug: the caller drops the colour
/// and the project falls back to its default.
pub fn colour_from_hex(hex: &str) -> Option<Hsla> {
    theme::try_parse_color(hex).ok()
}

/// Whether text on this background should be light or dark.
///
/// Perceived brightness, not `Hsla::l`: lightness in HSL says a saturated yellow
/// and a saturated blue at the same `l` are equally bright, and they are not —
/// black reads fine on the yellow and not at all on the blue. The weights are
/// the usual luminance ones (ITU-R BT.601), and the 0.55 threshold is where the
/// two candidates below have equal contrast against a mid grey.
pub fn text_is_light_on(background: Hsla) -> bool {
    let rgba = gpui::Rgba::from(background);
    let luminance = 0.299 * rgba.r + 0.587 * rgba.g + 0.114 * rgba.b;
    luminance < 0.55
}

/// The label colour to draw over `background`.
pub fn label_colour_for(background: Hsla) -> Hsla {
    if text_is_light_on(background) {
        gpui::hsla(0., 0., 1., 1.)
    } else {
        gpui::hsla(0., 0., 0.08, 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_are_trimmed_capped_and_may_be_cleared() {
        assert_eq!(sanitize_initials("ABCDE").as_deref(), Some("AB"));
        assert_eq!(sanitize_initials("  zo  ").as_deref(), Some("zo"));
        assert_eq!(sanitize_initials("Z").as_deref(), Some("Z"));
        // Clearing the field is how a project goes back to initials derived
        // from its name, so an empty string is an answer rather than a value.
        assert_eq!(sanitize_initials("").as_deref(), None);
        assert_eq!(sanitize_initials("   ").as_deref(), None);
        // Counted in characters, not bytes: two of these are 4 bytes each.
        assert_eq!(sanitize_initials("日本語").as_deref(), Some("日本"));
    }

    #[test]
    fn a_colour_survives_the_trip_to_disk_and_back() {
        for hex in ["#000000", "#ffffff", "#3b82f6", "#7f1d1d"] {
            let colour = colour_from_hex(hex).expect("a well-formed hex parses");
            assert_eq!(
                colour_to_hex(colour),
                hex,
                "{hex} did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_record_written_by_hand_can_be_refused() {
        for junk in ["", "not-a-colour", "#12", "#gggggg", "rgb(1,2,3)"] {
            assert!(
                colour_from_hex(junk).is_none(),
                "{junk:?} must be refused, not guessed at"
            );
        }
    }

    #[test]
    fn text_follows_the_brightness_of_its_background() {
        let light_on = |hex: &str| text_is_light_on(colour_from_hex(hex).unwrap());
        assert!(light_on("#101010"), "light text on near-black");
        assert!(!light_on("#f5f5f5"), "dark text on near-white");
        // The pair that makes the point: same HSL lightness, opposite answers.
        // Reading `Hsla::l` alone would give both the same text colour and put
        // white on the yellow, where it disappears.
        assert!(!light_on("#ffff00"), "dark text on saturated yellow");
        assert!(light_on("#0000ff"), "light text on saturated blue");
    }
}
