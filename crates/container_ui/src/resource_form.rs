//! What every form in this crate asks of the values typed into it, before the
//! backend is ever called.
//!
//! The backend refuses the same values again -- these are not the safety net,
//! they are the reason a person sees "ports are 1 to 65535" beside the field
//! instead of a refusal after pressing the button. The rules are the engine's
//! own, written once here so a dialog cannot invent a looser one.

use gpui::{App, Entity, Window};
use ui::prelude::*;
use ui_input::InputField;

/// What a field accepts. An empty optional field is always fine; an empty
/// required one is only reported once somebody has tried to submit.
#[derive(Copy, Clone)]
pub(crate) struct Rule {
    pub(crate) required: bool,
    pub(crate) check: fn(&str) -> Result<(), String>,
}

impl Rule {
    pub(crate) const fn required(check: fn(&str) -> Result<(), String>) -> Self {
        Self {
            required: true,
            check,
        }
    }

    pub(crate) const fn optional(check: fn(&str) -> Result<(), String>) -> Self {
        Self {
            required: false,
            check,
        }
    }

    /// Why `text` is not acceptable, or `None`. `attempted` is whether a submit
    /// has been tried, which is when an empty required field starts to count.
    pub(crate) fn problem(&self, text: &str, attempted: bool) -> Option<String> {
        let text = text.trim();
        if text.is_empty() {
            return (self.required && attempted).then(|| "Required".to_string());
        }
        (self.check)(text).err()
    }
}

/// One labelled single-line input of a form.
pub(crate) struct Field {
    pub(crate) label: &'static str,
    pub(crate) input: Entity<InputField>,
    pub(crate) rule: Rule,
}

impl Field {
    pub(crate) fn new(
        label: &'static str,
        placeholder: &str,
        initial: &str,
        rule: Rule,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let input = cx.new(|cx| InputField::new(window, cx, placeholder));
        let field = Self { label, input, rule };
        if !initial.is_empty() {
            field.set_text(initial, window, cx);
        }
        field
    }

    /// Replaces what is typed. The editor is cloned out first: reading the
    /// input and writing through it in one expression borrows `cx` twice.
    pub(crate) fn set_text(&self, text: &str, window: &mut Window, cx: &mut App) {
        let editor = self.input.read(cx).editor().clone();
        editor.set_text(text, window, cx);
    }

    pub(crate) fn text(&self, cx: &App) -> String {
        self.input.read(cx).text(cx).trim().to_string()
    }

    pub(crate) fn problem(&self, attempted: bool, cx: &App) -> Option<String> {
        self.rule.problem(&self.text(cx), attempted)
    }
}

/// A failure in the words the engine or the form used, with the glyph that says
/// it is one.
pub(crate) fn error_line(message: impl Into<SharedString>) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .child(
            Icon::new(IconName::XCircle)
                .size(IconSize::XSmall)
                .color(Color::Error),
        )
        .child(
            div().min_w_0().child(
                Label::new(message)
                    .size(LabelSize::Small)
                    .color(Color::Error),
            ),
        )
}

/// The label over a field, its input, and what is wrong with it when something
/// is.
pub(crate) fn field_view(
    field: &Field,
    attempted: bool,
    cx: &App,
    selector: String,
) -> impl IntoElement {
    let problem = field.problem(attempted, cx);
    v_flex()
        .gap_1()
        .child(
            Label::new(field.label)
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            div()
                .debug_selector(move || selector)
                .child(field.input.clone()),
        )
        .children(problem.map(error_line))
}

fn refuse(field: &str, reason: &str) -> Result<(), String> {
    Err(format!("{field} {reason}"))
}

/// `[a-zA-Z0-9][a-zA-Z0-9_.-]*`: a container, volume, network or driver name.
/// The one shape that cannot start with a dash, so it cannot be an option.
pub(crate) fn check_name(value: &str) -> Result<(), String> {
    let mut chars = value.chars();
    let starts_well = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric());
    if starts_well && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        Ok(())
    } else {
        refuse(
            "Names are",
            "letters, digits, `_`, `.` and `-`, starting with a letter or digit",
        )
    }
}

/// An image reference (`repo/name:tag@sha256:..`): no whitespace, no dash first.
pub(crate) fn check_reference(value: &str) -> Result<(), String> {
    if value.starts_with('-') {
        return refuse("A reference", "must not start with `-`");
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return refuse("A reference", "must not contain whitespace");
    }
    Ok(())
}

/// A port a mapping can use. 0 means "pick one" to the engine, which is not a
/// mapping, so it is out.
pub(crate) fn check_port(value: &str) -> Result<(), String> {
    match value.parse::<u32>() {
        Ok(port) if (1..=65_535).contains(&port) => Ok(()),
        _ => refuse("A port is", "a number from 1 to 65535"),
    }
}

pub(crate) fn parse_port(value: &str) -> Option<u16> {
    value.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

/// `[A-Za-z_][A-Za-z0-9_]*`.
pub(crate) fn check_env_key(value: &str) -> Result<(), String> {
    let mut chars = value.chars();
    let starts_well = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
    if starts_well && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(())
    } else {
        refuse(
            "A variable name is",
            "letters, digits and `_`, not starting with a digit",
        )
    }
}

/// A volume name or a host path for the source of a mount, or a path inside the
/// container for its target. No dash first, no control characters, and no `:`
/// -- it would split `source:target` in the wrong place -- except a Windows
/// drive letter, which is how a host path there starts.
pub(crate) fn check_mount_path(value: &str) -> Result<(), String> {
    if value.starts_with('-') {
        return refuse("A path", "must not start with `-`");
    }
    if value.chars().any(char::is_control) {
        return refuse("A path", "must not contain control characters");
    }
    let drive = matches!(
        value.as_bytes(),
        [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic()
    );
    let rest = if drive {
        value.get(2..).unwrap_or_default()
    } else {
        value
    };
    if rest.contains(':') {
        return refuse("A path", "must not contain `:`");
    }
    Ok(())
}

/// `10.5.0.0/16`, `fd00::/64`.
pub(crate) fn check_subnet(value: &str) -> Result<(), String> {
    let valid = value.contains('/')
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() || matches!(c, '.' | ':' | '/'));
    if valid {
        Ok(())
    } else {
        refuse("A subnet", "looks like `10.5.0.0/16`")
    }
}

/// The words of a command line, split the way a person typing one expects:
/// whitespace separates, and quotes keep a space inside a word. No expansion
/// of any kind, and no backslash escapes: a backslash is a character like any
/// other -- the words become an argument list, never a shell string.
pub(crate) fn split_command(text: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, c) => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("A quote is never closed".to_string());
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

/// `program args` as one line to read, with a word that holds a space quoted so
/// the line says where words begin and end. Display only: nothing is ever run
/// from this text.
pub(crate) fn command_line(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(|word| {
            if word.is_empty() || word.chars().any(char::is_whitespace) {
                format!("{word:?}")
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_engines_rule_and_never_start_with_a_dash() {
        for good in ["web", "web-1", "a.b_c", "9lives", "A"] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in ["-rf", "_x", ".x", "a b", "a/b", "a;b", "é"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn references_refuse_whitespace_and_a_leading_dash_only() {
        for good in [
            "nginx",
            "nginx:1.27",
            "ghcr.io/org/app:v1",
            "app@sha256:ab12",
        ] {
            assert!(check_reference(good).is_ok(), "{good}");
        }
        for bad in ["--rm", "a b", "a\tb", "a\nb"] {
            assert!(check_reference(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_port_is_one_to_sixty_five_thousand_five_hundred_thirty_five() {
        for good in ["1", "80", "65535"] {
            assert!(check_port(good).is_ok(), "{good}");
            assert!(parse_port(good).is_some(), "{good}");
        }
        for bad in ["0", "65536", "-1", "http", "80.5", "99999999999"] {
            assert!(check_port(bad).is_err(), "{bad}");
            assert!(parse_port(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn variable_names_may_not_start_with_a_digit_or_hold_an_equals_sign() {
        assert!(check_env_key("POSTGRES_DB").is_ok());
        assert!(check_env_key("_private").is_ok());
        for bad in ["1A", "A=B", "A B", "A-B"] {
            assert!(check_env_key(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_mount_path_may_not_split_source_from_target() {
        for good in ["pgdata", "/var/lib/data", "./here", r"C:\data", "C:/data"] {
            assert!(check_mount_path(good).is_ok(), "{good}");
        }
        for bad in ["-v", "a:b", "/data:ro", "x\ny"] {
            assert!(check_mount_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_subnet_has_a_prefix_length() {
        assert!(check_subnet("10.5.0.0/16").is_ok());
        assert!(check_subnet("fd00::/64").is_ok());
        for bad in ["10.5.0.0", "net/16", "10.5.0.0/16 ", "--x/1"] {
            assert!(check_subnet(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_required_field_only_complains_about_being_empty_after_an_attempt() {
        let rule = Rule::required(check_name);
        assert_eq!(rule.problem("", false), None);
        assert_eq!(rule.problem("  ", true).as_deref(), Some("Required"));
        assert!(
            rule.problem("-x", false).is_some(),
            "wrong is wrong at once"
        );
        assert_eq!(Rule::optional(check_name).problem("", true), None);
    }

    #[test]
    fn a_command_splits_on_whitespace_and_keeps_quoted_words_whole() {
        assert_eq!(split_command("").unwrap(), Vec::<String>::new());
        assert_eq!(split_command("  sh   -c ").unwrap(), ["sh", "-c"]);
        assert_eq!(
            split_command(r#"sh -c "echo  hi there""#).unwrap(),
            ["sh", "-c", "echo  hi there"]
        );
        assert_eq!(split_command("echo ''").unwrap(), ["echo", ""]);
        assert_eq!(split_command("a'b c'd").unwrap(), ["ab cd"]);
        assert!(split_command("echo \"oops").is_err());
    }

    #[test]
    fn a_backslash_is_an_ordinary_character_in_a_command() {
        assert_eq!(split_command(r"echo a\ b").unwrap(), ["echo", r"a\", "b"]);
        assert_eq!(
            split_command(r#"echo "a\" b"#).unwrap(),
            ["echo", r"a\", "b"]
        );
    }

    #[test]
    fn a_command_line_quotes_only_what_holds_a_space() {
        let args = vec!["run".to_string(), "a b".to_string(), String::new()];
        assert_eq!(command_line("docker", &args), r#"docker run "a b" """#);
    }
}
