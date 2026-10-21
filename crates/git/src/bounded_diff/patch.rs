//! Cutting a patch down to the files that may be shown.
//!
//! Git is asked for the diff of the files that passed the caller's filter, but
//! a name given to git as a path also names everything beneath it, so a file
//! that became a folder brings the folder's contents along. What git prints is
//! therefore judged again, file by file, from the header that opens each one.

use crate::repository::RepoPath;

const HEADER: &[u8] = b"diff --git ";

/// `patch` without the files `allowed` turns down. A file whose header cannot
/// be read is turned down as well: nothing could judge whether it may be shown.
///
/// The patch must have been made with `--no-renames`, so that both sides of a
/// header name one path, and with the prefixes `a/` and `b/`.
pub(super) fn retain_allowed_files(patch: &[u8], allowed: impl Fn(&RepoPath) -> bool) -> Vec<u8> {
    let mut retained = Vec::with_capacity(patch.len());
    let mut keeping = false;
    for line in patch.split_inclusive(|byte| *byte == b'\n') {
        // Every other line of a patch starts with a space, `+`, `-`, `\` or a
        // keyword, so this one only ever opens a file.
        if let Some(header) = line.strip_prefix(HEADER) {
            keeping = file_of_header(header).is_some_and(|path| allowed(&path));
        }
        if keeping {
            retained.extend_from_slice(line);
        }
    }
    retained
}

/// The path in `a/<path> b/<path>`, which git writes in quotes, with escapes,
/// when the path holds a character that would be hard to read back.
fn file_of_header(header: &[u8]) -> Option<RepoPath> {
    let header = header.strip_suffix(b"\n").unwrap_or(header);
    let path = if header.first() == Some(&b'"') {
        let (old, rest) = unquote(header)?;
        let (new, rest) = unquote(rest.strip_prefix(b" ")?)?;
        if !rest.is_empty() {
            return None;
        }
        let old = old.strip_prefix(b"a/")?;
        if old != new.strip_prefix(b"b/")? {
            return None;
        }
        old.to_vec()
    } else {
        // `<path> b/<path>`: the two halves are the same text, which is what
        // says where the first ends when the path holds spaces.
        let paths = header.strip_prefix(b"a/")?;
        let (old, rest) = paths.split_at_checked(paths.len().checked_sub(3)? / 2)?;
        if old != rest.strip_prefix(b" b/")? {
            return None;
        }
        old.to_vec()
    };
    if path.is_empty() {
        return None;
    }
    RepoPath::new(std::str::from_utf8(&path).ok()?).ok()
}

/// The text of the quoted name at the start of `quoted`, and what follows it.
fn unquote(quoted: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let body = quoted.strip_prefix(b"\"")?;
    let mut text = Vec::with_capacity(body.len());
    let mut index = 0;
    while let Some(&byte) = body.get(index) {
        index += 1;
        match byte {
            b'"' => return Some((text, body.get(index..)?)),
            b'\\' => {
                let escape = *body.get(index)?;
                index += 1;
                text.push(match escape {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'v' => 0x0b,
                    b'\\' | b'"' => escape,
                    // A byte as three octal digits.
                    b'0'..=b'3' => {
                        let digits = body.get(index - 1..index + 2)?;
                        index += 2;
                        octal_byte(digits)?
                    }
                    _ => return None,
                });
            }
            _ => text.push(byte),
        }
    }
    None
}

fn octal_byte(digits: &[u8]) -> Option<u8> {
    let mut value = 0u32;
    for digit in digits {
        if !(b'0'..=b'7').contains(digit) {
            return None;
        }
        value = value * 8 + u32::from(digit - b'0');
    }
    u8::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_of(patch: &[u8]) -> Vec<String> {
        let kept = retain_allowed_files(patch, |_| true);
        String::from_utf8_lossy(&kept)
            .lines()
            .filter_map(|line| line.strip_prefix("diff --git "))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn the_path_is_read_from_a_plain_header() {
        let path = file_of_header(b"a/src/main.rs b/src/main.rs\n").expect("a path");
        assert_eq!(path.as_unix_str(), "src/main.rs");
    }

    #[test]
    fn a_path_with_spaces_is_read_whole() {
        let path = file_of_header(b"a/my docs/a b.txt b/my docs/a b.txt\n").expect("a path");
        assert_eq!(path.as_unix_str(), "my docs/a b.txt");
    }

    #[test]
    fn a_path_that_looks_like_a_header_is_read_whole() {
        let path = file_of_header(b"a/x b/y b/x b/y\n").expect("a path");
        assert_eq!(path.as_unix_str(), "x b/y");
    }

    #[test]
    fn a_quoted_path_is_unescaped() {
        let path = file_of_header(
            b"\"a/caf\\303\\251 \\\"x\\\"\\t.txt\" \"b/caf\\303\\251 \\\"x\\\"\\t.txt\"\n",
        )
        .expect("a path");
        assert_eq!(path.as_unix_str(), "café \"x\"\t.txt");
    }

    #[test]
    fn a_header_that_does_not_read_as_one_path_is_not_read() {
        for header in [
            &b"a/old b/new\n"[..],
            b"a/x\n",
            b"x b/x\n",
            b"\"a/x\" b/x\n",
            b"\"a/x\" \"b/y\"\n",
            b"\"a/x\" \"b/x\" extra\n",
            b"\"a/unterminated b/x\n",
            b"\"a/bad\\q\" \"b/bad\\q\"\n",
            b"\"a/\\777\" \"b/\\777\"\n",
            b"a/\xff b/\xff\n",
            b"a/ b/\n",
            b"\n",
        ] {
            assert!(
                file_of_header(header).is_none(),
                "{}",
                String::from_utf8_lossy(header)
            );
        }
    }

    #[test]
    fn files_that_are_turned_down_are_cut_out_whole_and_the_rest_are_untouched() {
        let patch = b"diff --git a/one b/one\n--- a/one\n+++ b/one\n@@ -1 +1 @@\n-a\n+b\n\
diff --git a/two b/two\nnew file mode 100644\n--- /dev/null\n+++ b/two\n@@ -0,0 +1 @@\n+secret\n\
diff --git a/three b/three\nBinary files a/three and b/three differ\n";
        let kept = retain_allowed_files(patch, |path| path.as_unix_str() != "two");
        let kept = String::from_utf8(kept).expect("text");
        assert!(kept.starts_with("diff --git a/one b/one\n"));
        assert!(kept.contains("-a\n+b\n"));
        assert!(kept.ends_with("Binary files a/three and b/three differ\n"));
        assert!(!kept.contains("secret"), "{kept}");
        assert!(!kept.contains("two"), "{kept}");
    }

    #[test]
    fn a_line_of_a_file_that_looks_like_a_header_does_not_open_one() {
        let patch = b"diff --git a/one b/one\n@@ -0,0 +1,2 @@\n+diff --git a/two b/two\n+more\n";
        assert_eq!(paths_of(patch), ["a/one b/one"]);
        let kept = retain_allowed_files(patch, |path| path.as_unix_str() == "one");
        assert_eq!(kept, patch);
        let none = retain_allowed_files(patch, |path| path.as_unix_str() == "two");
        assert!(none.is_empty());
    }

    #[test]
    fn what_comes_before_the_first_header_and_unreadable_headers_are_dropped() {
        let patch = b"stray\ndiff --git a/old b/new\n+renamed\ndiff --git a/ok b/ok\n+fine\n";
        let kept = retain_allowed_files(patch, |_| true);
        assert_eq!(kept, b"diff --git a/ok b/ok\n+fine\n");
    }

    #[test]
    fn a_patch_cut_in_the_middle_of_a_header_loses_that_header() {
        let patch = b"diff --git a/one b/one\n+x\ndiff --git a/tw";
        let kept = retain_allowed_files(patch, |_| true);
        assert_eq!(kept, b"diff --git a/one b/one\n+x\n");
    }
}
