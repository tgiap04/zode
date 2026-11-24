use std::borrow::Cow;

use crate::untrusted_text::reveal_hidden_characters;

#[test]
fn a_right_to_left_override_in_a_title_shows_a_marker() {
    assert_eq!(
        reveal_hidden_characters("Fix login \u{202E}fdp.exe"),
        "Fix login <U+202E>fdp.exe"
    );
    assert_eq!(
        reveal_hidden_characters("\u{2067}isolate\u{2069}"),
        "<U+2067>isolate<U+2069>"
    );
}

#[test]
fn invisible_and_control_characters_show_markers() {
    assert_eq!(
        reveal_hidden_characters("main\u{200B}branch\u{FEFF}"),
        "main<U+200B>branch<U+FEFF>"
    );
    assert_eq!(reveal_hidden_characters("bell\u{7}"), "bell<U+0007>");
    assert_eq!(
        reveal_hidden_characters("ad\u{200D}min"),
        "ad<U+200D>min",
        "a joiner between letters hides nothing worth keeping"
    );
    assert_eq!(
        reveal_hidden_characters("x\u{E0041}\u{E0042}"),
        "x<U+E0041><U+E0042>",
        "tag characters smuggle text"
    );
}

#[test]
fn plain_text_and_emoji_are_left_alone() {
    assert!(matches!(
        reveal_hidden_characters("Crash on start (日本語, café)"),
        Cow::Borrowed(_)
    ));
    let family = "👩\u{200D}💻 dev";
    assert_eq!(reveal_hidden_characters(family), family);
    let england = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";
    assert_eq!(reveal_hidden_characters(england), england);
}

#[test]
fn line_separators_and_stray_variation_selectors_show_markers() {
    assert_eq!(
        reveal_hidden_characters("Fix\u{2028}Merge\u{2029}"),
        "Fix<U+2028>Merge<U+2029>"
    );
    assert_eq!(
        reveal_hidden_characters("a\u{FE00}b\u{E0100}c\u{1D173}"),
        "a<U+FE00>b<U+E0100>c<U+1D173>"
    );
    let heart = "\u{2764}\u{FE0F} thanks";
    assert_eq!(
        reveal_hidden_characters(heart),
        heart,
        "the emoji presentation selector after a pictograph is kept"
    );
}
