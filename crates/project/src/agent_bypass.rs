//! Whether the agent CLI that is actually installed still takes the flag this
//! editor means to pass it.
//!
//! The flag vocabulary lives on `BuiltinAgent`, and it goes stale: between the
//! consultation that wrote this table and the plan that read it, on the same
//! afternoon, Codex's `--full-auto` had already become `--approve-for-me`. A
//! mapping that has gone stale must surface as a refusal to launch, never as a
//! launch that quietly keeps asking for permission the reader thought they had
//! turned off.
//!
//! Everything here fails closed. Only a confirmed `Present` may be treated as
//! permission to add a flag; a spawn error, a timeout, an unreadable binary and
//! an agent with no mapping all land on values that are not `Present`.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use collections::HashMap;

/// How long a `--help` probe may run before it counts as no answer.
///
/// Measured warm on an Apple Silicon box, mean of three: codex 29 ms, agy 78 ms,
/// claude 91 ms, opencode 361 ms, copilot 397 ms. Cold is worse, which is what
/// the margin is for; a wedged binary must not hold a launch open forever.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The answer to "may this launch carry its bypass flag".
///
/// Deliberately has no `Default` and is not a `bool`: with a `bool` the unsafe
/// direction is the one you get by forgetting something.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BypassCheck {
    /// The installed binary offers the flag.
    Present,
    /// The binary was read and does not offer it -- a rename, or a flag removed.
    Absent { flag: &'static str, binary: PathBuf },
    /// No answer could be obtained. Not the same as `Absent`, and reported
    /// differently, but it refuses just the same.
    Unprobed { reason: String },
}

/// Both streams, merged.
///
/// Two of the five CLIs write their help to stderr and nothing at all to stdout
/// (measured: `agy` 0 bytes out against 2759 on err, `opencode` 0 against 4285).
/// A probe that reads stdout alone finds an empty page and refuses every
/// Antigravity and opencode launch for the life of the app.
pub fn help_text(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    if !text.is_empty() && !stderr.is_empty() {
        text.push('\n');
    }
    text.push_str(&String::from_utf8_lossy(stderr));
    text
}

/// Whether `help` offers `flag` itself, rather than some longer flag it is only
/// the opening of.
///
/// `--allow-all` begins four other copilot flags -- `--allow-all-tools`,
/// `--allow-all-paths`, `--allow-all-urls`, `--allow-all-mcp-server-instructions`
/// -- and copilot's help carries that prefix fifteen times. A `contains` check
/// keeps answering yes after `--allow-all` itself has been removed.
pub fn flag_present(help: &str, flag: &str) -> bool {
    // Option lines only. A flag named in prose -- "`--old-flag` is deprecated,
    // use `--new-flag`" -- satisfies the boundary test below and would answer
    // `Present` for a flag the binary no longer accepts, which is this guard's
    // central promise with a hole in it. Every one of the five CLIs measured
    // lists its flags on lines whose first non-space character is `-`, and no
    // help text begins a sentence that way.
    help.lines()
        .filter(|line| line.trim_start().starts_with('-'))
        .any(|line| {
            line.match_indices(flag).any(|(at, _)| {
                line[at + flag.len()..]
                    .chars()
                    .next()
                    .is_none_or(|next| !next.is_alphanumeric() && next != '-' && next != '_')
            })
        })
}

/// A binary's identity on disk.
///
/// Not its version. `ExternalAgentServer::version` answers `None` for every
/// implementation in this fork, and builtin agents never get an entry at all --
/// so the version is not merely unknown on first launch, it is never known.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    binary: PathBuf,
    modified_nanos: u128,
    len: u64,
}

impl CacheKey {
    /// `None` when the file cannot be read or its filesystem reports no
    /// modification time. Both mean "probe again", never "assume unchanged" --
    /// a key that matched on absent metadata would cache an answer about a
    /// binary nobody had identified.
    pub fn for_binary(binary: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(binary).ok()?;
        Some(Self {
            binary: binary.to_path_buf(),
            modified_nanos: metadata
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_nanos(),
            len: metadata.len(),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(binary: &str, modified_nanos: u128, len: u64) -> Self {
        Self {
            binary: PathBuf::from(binary),
            modified_nanos,
            len,
        }
    }
}

/// Probe answers for this run of the app.
///
/// In memory and not persisted: the only invalidation signal available is the
/// binary's mtime and size, which a persisted cache would have to re-stat on
/// every read anyway. A restart is the coarse invalidation, and the cost of that
/// is one probe per agent per run.
#[derive(Default)]
pub struct BypassFlagCache {
    probed: HashMap<CacheKey, bool>,
}

impl BypassFlagCache {
    pub fn get(&self, key: &CacheKey) -> Option<bool> {
        self.probed.get(key).copied()
    }

    pub fn insert(&mut self, key: CacheKey, present: bool) {
        self.probed.insert(key, present);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A help page as `claude --help` prints it, with the flag renamed to
    /// something else -- which is what a vendor rename looks like from here.
    const HELP_WITH_FLAG_RENAMED: &str = "\
  --add-dir <directories...>            Additional directories to allow tool
  --skip-permission-checks              Bypass all permission checks.
  --permission-mode <mode>              Permission mode to use for the session
";

    /// copilot's real shape: every longer flag present, the short one gone.
    const HELP_WITH_ONLY_LONGER_FLAGS: &str = "\
  --allow-all-mcp-server-instructions   Include initialization instructions
  --allow-all-paths                     Disable file path verification
  --allow-all-tools                     Allow all tools to run automatically
";

    #[test]
    fn a_flag_absent_from_help_is_refused() {
        assert!(
            !flag_present(HELP_WITH_FLAG_RENAMED, "--dangerously-skip-permissions"),
            "a renamed flag must read as absent -- this is the whole guard"
        );
        assert!(
            flag_present(HELP_WITH_FLAG_RENAMED, "--skip-permission-checks"),
            "and the flag that IS there must still be found, or the guard \
             refuses everything and is indistinguishable from being broken"
        );
    }

    #[test]
    fn a_flag_that_is_only_a_prefix_of_another_is_refused() {
        assert!(
            !flag_present(HELP_WITH_ONLY_LONGER_FLAGS, "--allow-all"),
            "`--allow-all` is the opening of four other copilot flags; matching \
             on the prefix keeps answering yes after the flag itself is gone"
        );
        assert!(flag_present(
            HELP_WITH_ONLY_LONGER_FLAGS,
            "--allow-all-paths"
        ));
    }

    /// A flag named in prose is not a flag the binary accepts.
    ///
    /// A deprecation note is the realistic case, and it is the guard's central
    /// promise with a hole in it: answering `Present` here means the CLI either
    /// rejects the argv with a confusing error or accepts and ignores it and
    /// runs asking for permission the reader thought they had turned off.
    #[test]
    fn a_flag_only_mentioned_in_prose_is_not_offered() {
        let help = "\
Usage: claude [options]

  Note: --dangerously-skip-permissions is deprecated; use --permission-mode instead.

  --permission-mode <mode>   Permission mode to use for the session
";
        assert!(!flag_present(help, "--dangerously-skip-permissions"));
        assert!(flag_present(help, "--permission-mode"));
    }

    #[test]
    fn a_flag_at_the_very_end_of_the_help_is_found() {
        // No trailing byte at all. An implementation that indexes the next
        // character without checking for the end panics here rather than
        // failing. On an option line, because a `usage:` line is prose and the
        // parser now declines prose.
        assert!(flag_present("  --auto", "--auto"));
    }

    #[test]
    fn help_written_only_to_stderr_is_still_read() {
        let stderr = b"  --auto  auto-approve permissions that are not explicitly denied";
        let merged = help_text(b"", stderr);
        assert!(
            flag_present(&merged, "--auto"),
            "agy and opencode write help to stderr and nothing to stdout; \
             dropping stderr from the merge refuses both for good"
        );
    }

    #[test]
    fn both_streams_survive_the_merge() {
        let merged = help_text(b"from stdout", b"from stderr");
        assert!(merged.contains("from stdout") && merged.contains("from stderr"));
    }

    /// Pins the cache's own contract: an answer recorded for a binary is found
    /// again without a second probe. The store's use of it is held by
    /// compilation; what can rot here is the lookup, and removing the cache
    /// entirely makes this count two.
    #[test]
    fn a_second_check_of_an_unchanged_binary_does_not_probe_again() {
        let mut cache = BypassFlagCache::default();
        let key = CacheKey::for_test("/usr/local/bin/claude", 1_700_000_000, 42);
        let mut probes = 0;

        let mut check = |cache: &mut BypassFlagCache| match cache.get(&key) {
            Some(hit) => hit,
            None => {
                probes += 1;
                cache.insert(key.clone(), true);
                true
            }
        };

        assert!(check(&mut cache));
        assert!(check(&mut cache));
        assert_eq!(probes, 1, "the second check must not re-probe");
    }

    #[test]
    fn a_binary_that_changed_on_disk_is_probed_again() {
        let mut cache = BypassFlagCache::default();
        cache.insert(CacheKey::for_test("/usr/local/bin/claude", 1, 42), true);
        assert_eq!(
            cache.get(&CacheKey::for_test("/usr/local/bin/claude", 2, 42)),
            None,
            "a rebuilt binary at the same path is a different binary"
        );
    }

    #[test]
    fn an_unreadable_binary_has_no_cache_key() {
        assert!(
            CacheKey::for_binary(Path::new("/nonexistent/agent-binary")).is_none(),
            "no metadata means probe again, never assume unchanged"
        );
    }
}
