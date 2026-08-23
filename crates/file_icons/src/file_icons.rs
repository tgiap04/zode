use std::sync::Arc;
use std::{path::Path, str};

use gpui::{App, SharedString};
use theme::{GlobalTheme, IconTheme, ThemeRegistry};
use util::paths::PathExt;

#[derive(Debug)]
pub struct FileIcons {
    icon_theme: Arc<IconTheme>,
}

/// The icon key an association names, matched **case-insensitively**.
///
/// This is what VS Code does, and file names on disk do not agree on case:
/// `Dockerfile`, `Makefile`, `LICENSE` and `README` are conventionally capitalised
/// while the associations that name them are not, so an exact-case lookup returns
/// the default icon for every one of them. It also fixes `.PNG` and `README.MD`,
/// which miss their icon today.
///
/// The lowercasing lives here, once, rather than in the icon theme's tables:
/// registering every plausible spelling of every association is the same rule
/// written out thousands of times, and the spelling nobody thought of is still
/// wrong. Icon themes therefore carry one canonical lowercase key per association.
///
/// A free function rather than a closure so the rule can be asserted without
/// standing up an `App` and a theme registry.
fn icon_key_for<'a>(icon_theme: &'a IconTheme, name: &str) -> Option<&'a String> {
    let name = name.to_lowercase();
    icon_theme
        .file_stems
        .get(&name)
        .or_else(|| icon_theme.file_suffixes.get(&name))
}

impl FileIcons {
    pub fn get(cx: &App) -> Self {
        Self {
            icon_theme: GlobalTheme::icon_theme(cx).clone(),
        }
    }

    pub fn get_icon(path: &Path, cx: &App) -> Option<SharedString> {
        let this = Self::get(cx);

        let get_icon_from_suffix = |suffix: &str| -> Option<SharedString> {
            icon_key_for(&this.icon_theme, suffix).and_then(|typ| this.get_icon_for_type(typ, cx))
        };
        // TODO: Associate a type with the languages and have the file's language
        //       override these associations

        if let Some(mut typ) = path.file_name().and_then(|typ| typ.to_str()) {
            // check if file name is in suffixes
            // e.g. catch file named `eslint.config.js` instead of `.eslint.config.js`
            let maybe_path = get_icon_from_suffix(typ);
            if maybe_path.is_some() {
                return maybe_path;
            }

            // check if suffix based on first dot is in suffixes
            // e.g. consider `module.js` as suffix to angular's module file named `auth.module.js`
            while let Some((_, suffix)) = typ.split_once('.') {
                let maybe_path = get_icon_from_suffix(suffix);
                if maybe_path.is_some() {
                    return maybe_path;
                }
                typ = suffix;
            }
        }

        // handle cases where the file extension is made up of multiple important
        // parts (e.g Component.stories.tsx) that refer to an alternative icon style
        if let Some(suffix) = path.multiple_extensions() {
            let maybe_path = get_icon_from_suffix(suffix.as_str());
            if maybe_path.is_some() {
                return maybe_path;
            }
        }

        // primary case: check if the files extension or the hidden file name
        // matches some icon path
        if let Some(suffix) = path.extension_or_hidden_file_name() {
            let maybe_path = get_icon_from_suffix(suffix);
            if maybe_path.is_some() {
                return maybe_path;
            }
        }

        // this _should_ only happen when the file is hidden (has leading '.')
        // and is not a "special" file we have an icon (e.g. not `.eslint.config.js`)
        // that should be caught above. In the remaining cases, we want to check
        // for a normal supported extension e.g. `.data.json` -> `json`
        let extension = path.extension().and_then(|ext| ext.to_str());
        if let Some(extension) = extension {
            let maybe_path = get_icon_from_suffix(extension);
            if maybe_path.is_some() {
                return maybe_path;
            }
        }
        this.get_icon_for_type("default", cx)
    }

    fn default_icon_theme(cx: &App) -> Option<Arc<IconTheme>> {
        let theme_registry = ThemeRegistry::global(cx);
        theme_registry.default_icon_theme().ok()
    }

    pub fn get_icon_for_type(&self, typ: &str, cx: &App) -> Option<SharedString> {
        fn get_icon_for_type(icon_theme: &Arc<IconTheme>, typ: &str) -> Option<SharedString> {
            icon_theme
                .file_icons
                .get(typ)
                .map(|icon_definition| icon_definition.path.clone())
        }

        get_icon_for_type(GlobalTheme::icon_theme(cx), typ).or_else(|| {
            Self::default_icon_theme(cx).and_then(|icon_theme| get_icon_for_type(&icon_theme, typ))
        })
    }

    pub fn get_folder_icon(expanded: bool, path: &Path, cx: &App) -> Option<SharedString> {
        fn get_folder_icon(
            icon_theme: &Arc<IconTheme>,
            path: &Path,
            expanded: bool,
        ) -> Option<SharedString> {
            let name = path.file_name()?.to_str()?.trim();
            if name.is_empty() {
                return None;
            }

            // Lowercased for the same reason file associations are -- see
            // `icon_key_for`. A folder called `Components` or `Documents` is the
            // same folder as `components` or `documents` as far as an icon goes.
            let directory_icons = icon_theme.named_directory_icons.get(&name.to_lowercase())?;

            if expanded {
                directory_icons.expanded.clone()
            } else {
                directory_icons.collapsed.clone()
            }
        }

        get_folder_icon(GlobalTheme::icon_theme(cx), path, expanded)
            .or_else(|| {
                Self::default_icon_theme(cx)
                    .and_then(|icon_theme| get_folder_icon(&icon_theme, path, expanded))
            })
            .or_else(|| {
                // If we can't find a specific folder icon for the folder at the given path, fall back to the generic folder
                // icon.
                Self::get_generic_folder_icon(expanded, cx)
            })
    }

    fn get_generic_folder_icon(expanded: bool, cx: &App) -> Option<SharedString> {
        fn get_generic_folder_icon(
            icon_theme: &Arc<IconTheme>,
            expanded: bool,
        ) -> Option<SharedString> {
            if expanded {
                icon_theme.directory_icons.expanded.clone()
            } else {
                icon_theme.directory_icons.collapsed.clone()
            }
        }

        get_generic_folder_icon(GlobalTheme::icon_theme(cx), expanded).or_else(|| {
            Self::default_icon_theme(cx)
                .and_then(|icon_theme| get_generic_folder_icon(&icon_theme, expanded))
        })
    }

    pub fn get_chevron_icon(expanded: bool, cx: &App) -> Option<SharedString> {
        fn get_chevron_icon(icon_theme: &Arc<IconTheme>, expanded: bool) -> Option<SharedString> {
            if expanded {
                icon_theme.chevron_icons.expanded.clone()
            } else {
                icon_theme.chevron_icons.collapsed.clone()
            }
        }

        get_chevron_icon(GlobalTheme::icon_theme(cx), expanded).or_else(|| {
            Self::default_icon_theme(cx)
                .and_then(|icon_theme| get_chevron_icon(&icon_theme, expanded))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use collections::HashMap;
    use theme::{Appearance, ChevronIcons, DirectoryIcons, IconDefinition};

    /// An icon theme carrying exactly the associations under test, all lowercase —
    /// the canonical spelling a generated theme emits.
    fn theme_with(stems: &[&str], suffixes: &[&str]) -> IconTheme {
        IconTheme {
            id: "test".into(),
            name: "Test".into(),
            appearance: Appearance::Dark,
            directory_icons: DirectoryIcons {
                collapsed: None,
                expanded: None,
            },
            named_directory_icons: HashMap::default(),
            chevron_icons: ChevronIcons {
                collapsed: None,
                expanded: None,
            },
            file_stems: stems
                .iter()
                .map(|name| ((*name).to_string(), format!("{name}-icon")))
                .collect(),
            file_suffixes: suffixes
                .iter()
                .map(|name| ((*name).to_string(), format!("{name}-icon")))
                .collect(),
            file_icons: HashMap::default(),
        }
    }

    /// The names that made this rule necessary.
    ///
    /// Every one of these is a file people really have, spelled the way they
    /// really spell it, against an association spelled the way icon themes really
    /// spell it. An exact-case lookup silently gives all of them the default icon.
    #[test]
    fn conventionally_capitalised_file_names_still_find_their_icon() {
        let theme = theme_with(&["dockerfile", "makefile", "license", "readme"], &[]);

        for name in ["Dockerfile", "Makefile", "LICENSE", "README", "ReadMe"] {
            assert_eq!(
                icon_key_for(&theme, name).map(String::as_str),
                Some(format!("{}-icon", name.to_lowercase()).as_str()),
                "{name} has to resolve"
            );
        }
    }

    /// An uppercase extension is the same extension. This was wrong before the
    /// Material set arrived and would have stayed wrong.
    #[test]
    fn an_uppercase_extension_is_the_same_extension() {
        let theme = theme_with(&[], &["png", "md"]);

        assert_eq!(
            icon_key_for(&theme, "PNG").map(String::as_str),
            Some("png-icon")
        );
        assert_eq!(
            icon_key_for(&theme, "Md").map(String::as_str),
            Some("md-icon")
        );
    }

    /// A stem beats a suffix when both name the same string — the precedence the
    /// original lookup had, and which the lowercasing must not disturb.
    #[test]
    fn a_stem_still_wins_over_a_suffix() {
        let mut theme = theme_with(&[], &[]);
        theme.file_stems.insert("json".into(), "stem-icon".into());
        theme
            .file_suffixes
            .insert("json".into(), "suffix-icon".into());

        assert_eq!(
            icon_key_for(&theme, "JSON").map(String::as_str),
            Some("stem-icon")
        );
    }

    /// An association nothing knows about resolves to nothing, so the caller can
    /// fall through to the default icon rather than being handed a wrong one.
    #[test]
    fn an_unknown_name_resolves_to_nothing() {
        let theme = theme_with(&["dockerfile"], &["rs"]);
        assert_eq!(icon_key_for(&theme, "something-else"), None);
    }

    /// `IconDefinition` is imported so this module compiles against the real
    /// shape; a change to it should break here rather than only in the generator.
    #[test]
    fn the_icon_theme_shape_is_the_real_one() {
        let definition = IconDefinition {
            path: "icons/file_icons/material/rust.svg".into(),
        };
        assert!(definition.path.ends_with(".svg"));
    }
}
