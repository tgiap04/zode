use imara_diff::{Algorithm, UnifiedDiffBuilder, diff, intern::InternedInput};

/// A rendered difference between what is on this machine and what is on the
/// server.
///
/// Rendered here rather than in the UI so the modal has nothing to compute and
/// nothing to get wrong. `is_empty` is the caller's cue that two files differ
/// only in ways the diff does not show — which, for a byte-for-byte comparison
/// upstream, should never happen; if it does, showing an empty diff is more
/// honest than showing nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDiff {
    pub unified: String,
    pub added: usize,
    pub removed: usize,
}

impl TextDiff {
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.removed == 0
    }
}

/// Builds a unified diff from local to remote.
///
/// Direction matters and is fixed: removals are what the local file has and
/// the remote does not, additions are what pulling would bring in. A modal
/// that reversed this would show the user the exact opposite of what pressing
/// the button does.
pub fn between(local: &str, remote: &str) -> TextDiff {
    let input = InternedInput::new(local, remote);
    let unified = diff(
        Algorithm::Histogram,
        &input,
        UnifiedDiffBuilder::new(&input),
    );

    let mut added = 0;
    let mut removed = 0;
    for line in unified.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        match line.as_bytes().first() {
            Some(b'+') => added += 1,
            Some(b'-') => removed += 1,
            _ => {}
        }
    }

    TextDiff {
        unified,
        added,
        removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_content_produces_an_empty_diff() {
        let same = "{\n  \"a\": 1\n}\n";
        assert!(between(same, same).is_empty());
    }

    #[test]
    fn additions_are_what_pulling_would_bring_in() {
        let result = between("{\n}\n", "{\n  \"theme\": \"One Dark\"\n}\n");
        assert!(
            result.unified.contains("+  \"theme\""),
            "{}",
            result.unified
        );
        assert_eq!(result.added, 1);
        assert_eq!(result.removed, 0);
    }

    #[test]
    fn removals_are_what_the_local_file_would_lose() {
        let result = between("{\n  \"secret\": \"local\"\n}\n", "{\n}\n");
        assert!(
            result.unified.contains("-  \"secret\""),
            "{}",
            result.unified
        );
        assert_eq!(result.removed, 1);
        assert_eq!(result.added, 0);
    }

    #[test]
    fn comments_show_up_as_ordinary_lines() {
        // The whole file travels as text, so comments are diffable content
        // rather than something a JSON parser would have silently dropped.
        let result = between("// mine\n{}\n", "// theirs\n{}\n");
        assert!(result.unified.contains("-// mine"));
        assert!(result.unified.contains("+// theirs"));
    }
}
