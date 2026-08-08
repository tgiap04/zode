pub use env_var::{EnvVar, bool_env_var, env_var};
use std::sync::LazyLock;

/// Whether Zode is running in stateless mode.
/// When true, Zode will use in-memory databases instead of persistent storage.
pub static ZED_STATELESS: LazyLock<bool> = bool_env_var!("ZED_STATELESS");

/// Lets every `ZED_*` variable also be spelled `ZODE_*`, by copying the latter
/// onto the former before anything reads them.
///
/// Doing it here rather than at each read site is deliberate: there are around
/// thirty places that read a `ZED_*` variable, spread over fifteen crates, and
/// all of them are upstream Zed code. Teaching each one a second name would
/// create thirty permanent rebase conflicts to spare users a collision that only
/// happens when they export a variable by hand. This costs one.
///
/// `ZODE_*` wins when both are set — the more specific name is the one the user
/// reached for.
///
/// Build-time variables (`env!` / `option_env!`, things like `ZED_COMMIT_SHA`)
/// are untouched on purpose: the build sets those, not the user.
///
/// # Panics
///
/// Never, but it must be called as the very first thing `main` does. Anything
/// that reads the environment before this runs — `RELEASE_CHANNEL_NAME` is the
/// easiest to trip over — will simply not see the `ZODE_*` spelling, with no
/// error to show for it.
pub fn bridge_zode_env_vars() {
    // Collected before any write: mutating the environment while iterating it is
    // not sound on every platform.
    let overrides = zode_overrides(std::env::vars());

    for (key, value) in overrides {
        // SAFETY: `set_var` is unsafe because another thread may be reading the
        // environment concurrently. This runs as the first statement of `main`,
        // before the process has spawned any thread, so there is no such reader.
        unsafe { std::env::set_var(key, value) };
    }
}

fn zode_overrides(vars: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.filter_map(|(key, value)| {
        key.strip_prefix("ZODE_")
            .map(|suffix| (format!("ZED_{suffix}"), value))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::zode_overrides;

    fn overrides(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut out = zode_overrides(
            pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        );
        out.sort();
        out
    }

    #[test]
    fn a_zode_variable_is_copied_onto_its_zed_name() {
        assert_eq!(
            overrides(&[("ZODE_STATELESS", "1")]),
            [("ZED_STATELESS".to_string(), "1".to_string())]
        );
    }

    /// The more specific spelling is the one the user reached for, so it has to
    /// win rather than defer to a `ZED_*` that may just be left over.
    #[test]
    fn zode_wins_when_both_spellings_are_set() {
        assert_eq!(
            overrides(&[("ZED_STATELESS", "1"), ("ZODE_STATELESS", "0")]),
            [("ZED_STATELESS".to_string(), "0".to_string())]
        );
    }

    /// The bridge writes into the process environment, so anything it touches
    /// beyond the `ZODE_` prefix would be clobbering unrelated state.
    #[test]
    fn nothing_outside_the_zode_prefix_is_touched() {
        assert!(
            overrides(&[
                ("PATH", "/usr/bin"),
                ("ZED_STATELESS", "1"),
                ("ZODESTALE", "x"),
                ("MY_ZODE_VAR", "x"),
            ])
            .is_empty()
        );
    }

    /// A bare `ZODE_` would map to a bare `ZED_`, which is a real variable name
    /// nothing should be assigning by accident.
    #[test]
    fn a_bare_prefix_maps_to_a_bare_prefix() {
        assert_eq!(
            overrides(&[("ZODE_", "x")]),
            [("ZED_".to_string(), "x".to_string())]
        );
    }
}
