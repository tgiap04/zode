//! Hiding values in anything an environment file is rendered into.
//!
//! Every surface that shows a `.env` shows it through here: the diff window,
//! the vault list, a tooltip. The reason is narrow and practical — the moment
//! a user hits a problem they take a screenshot and paste it into a chat, and
//! a diff that renders `STRIPE_SECRET_KEY=sk_live_…` in full has leaked the
//! credential to everyone in that room.
//!
//! What is deliberately NOT hidden is the key name. Someone deciding whether
//! to overwrite their production environment has to see *which* variables
//! change; a diff of forty masked lines is a diff nobody can act on. The names
//! are also already on this machine, in this file, in this editor — hiding
//! them here would buy nothing.

/// What replaces a value.
///
/// A fixed width rather than one dot per character: the length of a secret is
/// itself a clue, and a mask that tracks it tells a reader whether they are
/// looking at a 32-character token or a four-character password.
const MASK: &str = "••••••••";

/// Rewrites one line of an environment file, or of a diff of one.
///
/// Leading diff markers survive: `+`, `-` and a space are what make a unified
/// diff readable, and stripping them to find the assignment would render a
/// diff that no longer looks like one.
///
/// That makes one case ambiguous, and the ambiguity is resolved in favour of
/// the diff: a raw line beginning `-----BEGIN PRIVATE KEY-----` has its first
/// dash read as a removal marker. The result is `-••••••••` rather than
/// `••••••••` — cosmetically wrong on raw input, and safe either way, because
/// the branch it lands in masks the whole line. A reading that got this
/// "right" would have to guess at whether its input is a diff, and guessing
/// wrong in the other direction means printing a diff line as a secret.
pub fn mask_line(line: &str) -> String {
    let (marker, body) = split_marker(line);

    // A comment holds no assignment, and people put real notes in them.
    if body.trim_start().starts_with('#') || body.trim().is_empty() {
        return line.to_string();
    }

    let Some(equals) = body.find('=') else {
        // Not an assignment. Continuation lines of a multi-line value land
        // here, and they are masked whole rather than passed through — the
        // second line of a private key is still the private key.
        return format!("{marker}{MASK}");
    };

    let name = &body[..equals];
    // Anything that is not a plausible variable name means this line is not
    // the assignment it appears to be. Masked whole rather than guessed at.
    if !is_assignment_name(name) {
        return format!("{marker}{MASK}");
    }

    let value = &body[equals + 1..];
    if value.trim().is_empty() {
        // `KEY=` is not a secret, and showing it empty tells the reader
        // something true: the variable is declared with no value.
        return line.to_string();
    }

    format!("{marker}{name}={MASK}")
}

/// Rewrites a whole rendered block.
pub fn mask_block(text: &str) -> String {
    // `split_inclusive` keeps the line endings, so a masked diff still ends
    // the way the original did and a trailing newline is not invented or lost.
    text.split_inclusive('\n')
        .map(|line| {
            let (body, ending) = match line.strip_suffix('\n') {
                Some(body) => (
                    body.strip_suffix('\r').unwrap_or(body),
                    if body.ends_with('\r') { "\r\n" } else { "\n" },
                ),
                None => (line, ""),
            };
            format!("{}{ending}", mask_line(body))
        })
        .collect()
}

/// Splits a leading unified-diff marker off a line.
fn split_marker(line: &str) -> (&str, &str) {
    match line.as_bytes().first() {
        Some(b'+' | b'-' | b' ') => line.split_at(1),
        _ => ("", line),
    }
}

/// Whether this looks like the left side of an environment assignment.
///
/// `export ` is accepted because `.env` files are frequently sourced by a
/// shell and written that way. Nothing else is: a line like
/// `SELECT * FROM t WHERE a=1` inside a heredoc must NOT be treated as an
/// assignment whose "value" is safe to show.
fn is_assignment_name(raw: &str) -> bool {
    let name = raw.trim().strip_prefix("export ").unwrap_or(raw.trim());
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// The value on a line, for the one place that is allowed to show it.
///
/// Used only after the platform has confirmed who is at the keyboard. Returns
/// `None` for anything that is not a plain assignment, so the reveal path
/// cannot be talked into printing a line it does not understand.
pub fn value_of(line: &str) -> Option<&str> {
    let (_, body) = split_marker(line);
    if body.trim_start().starts_with('#') {
        return None;
    }
    let equals = body.find('=')?;
    is_assignment_name(&body[..equals]).then(|| body[equals + 1..].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_never_survives_masking() {
        assert_eq!(
            mask_line("STRIPE_KEY=sk_live_abc123"),
            "STRIPE_KEY=••••••••"
        );
        assert_eq!(
            mask_line("DATABASE_URL=postgres://user:pw@host/db"),
            "DATABASE_URL=••••••••"
        );
    }

    #[test]
    fn the_variable_name_survives_because_the_decision_needs_it() {
        assert!(mask_line("STRIPE_KEY=sk_live_abc").starts_with("STRIPE_KEY="));
    }

    #[test]
    fn diff_markers_survive() {
        assert_eq!(mask_line("+API_KEY=new"), "+API_KEY=••••••••");
        assert_eq!(mask_line("-API_KEY=old"), "-API_KEY=••••••••");
        assert_eq!(mask_line(" API_KEY=same"), " API_KEY=••••••••");
    }

    #[test]
    fn the_mask_does_not_leak_the_length() {
        // Two secrets of very different length must render identically.
        assert_eq!(
            mask_line("A=x"),
            mask_line("A=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
        );
    }

    #[test]
    fn comments_and_blank_lines_pass_through() {
        assert_eq!(
            mask_line("# the staging database"),
            "# the staging database"
        );
        assert_eq!(mask_line("  # indented note"), "  # indented note");
        assert_eq!(mask_line(""), "");
        assert_eq!(mask_line("   "), "   ");
        assert_eq!(mask_line("+# added a note"), "+# added a note");
    }

    #[test]
    fn an_empty_value_is_shown_as_empty() {
        // True and useful: the variable is declared with nothing in it.
        assert_eq!(mask_line("DEBUG="), "DEBUG=");
        assert_eq!(mask_line("+DEBUG="), "+DEBUG=");
    }

    #[test]
    fn export_prefixed_assignments_are_recognised() {
        assert_eq!(mask_line("export TOKEN=abc"), "export TOKEN=••••••••");
    }

    #[test]
    fn a_line_that_is_not_an_assignment_is_masked_whole() {
        // The conservative direction. A continuation line of a PEM block, or
        // anything else unrecognised, must not be printed on the theory that
        // it is probably harmless.
        assert_eq!(mask_line("MIIEpAIBAAKCAQEA7uXm"), "••••••••");
        assert_eq!(
            mask_line("SELECT * FROM users WHERE id=1"),
            "••••••••",
            "a name with spaces and punctuation is not an assignment"
        );
    }

    #[test]
    fn a_pem_header_is_masked_even_though_its_dash_reads_as_a_diff_marker() {
        // The documented ambiguity. `-----BEGIN…` keeps one dash because the
        // first one is taken for a removal marker. What matters is the part
        // after it, and that is gone.
        for line in [
            "-----BEGIN PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        ] {
            let masked = mask_line(line);
            assert!(masked.ends_with(MASK), "{line:?} -> {masked:?}");
            assert!(!masked.contains("BEGIN"), "{line:?} -> {masked:?}");
            assert!(!masked.contains("END"), "{line:?} -> {masked:?}");
        }
    }

    #[test]
    fn masking_a_block_keeps_its_shape() {
        let input = "# staging\nAPI_KEY=secret\n\nDEBUG=\n";
        assert_eq!(mask_block(input), "# staging\nAPI_KEY=••••••••\n\nDEBUG=\n");
    }

    #[test]
    fn masking_a_block_preserves_line_endings_and_the_last_line() {
        assert_eq!(mask_block("A=1\r\nB=2"), "A=••••••••\r\nB=••••••••");
        assert_eq!(mask_block("A=1"), "A=••••••••");
        assert_eq!(mask_block(""), "");
    }

    #[test]
    fn no_secret_from_a_realistic_file_survives_the_block() {
        let file = "\
# production
DATABASE_URL=postgres://admin:hunter2@db.internal/app
STRIPE_SECRET_KEY=sk_live_51Hxxxxxxxxxxxx
export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG
JWT_PRIVATE_KEY=-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA7uXm
-----END RSA PRIVATE KEY-----
DEBUG=
";
        let masked = mask_block(file);
        for secret in [
            "hunter2",
            "sk_live_51H",
            "wJalrXUtnFEMI",
            "MIIEpAIBAAKCAQEA7uXm",
            "BEGIN RSA PRIVATE KEY",
        ] {
            assert!(
                !masked.contains(secret),
                "{secret:?} survived masking:\n{masked}"
            );
        }
        // And the names a person needs to make a decision are still there.
        for name in [
            "DATABASE_URL",
            "STRIPE_SECRET_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "DEBUG",
        ] {
            assert!(masked.contains(name), "{name} was lost");
        }
    }

    #[test]
    fn reveal_returns_a_value_only_for_a_plain_assignment() {
        assert_eq!(value_of("API_KEY=sk_live_abc"), Some("sk_live_abc"));
        assert_eq!(value_of("+API_KEY= spaced  "), Some("spaced"));
        assert_eq!(value_of("export TOKEN=abc"), Some("abc"));
        assert_eq!(value_of("# API_KEY=not really"), None);
        assert_eq!(value_of("-----BEGIN PRIVATE KEY-----"), None);
        assert_eq!(value_of("no equals here"), None);
    }
}
