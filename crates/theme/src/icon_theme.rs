use std::sync::{Arc, LazyLock};

use collections::HashMap;
use gpui::SharedString;

use crate::Appearance;
use crate::icon_theme_material::{
    DIRECTORY_ICON_COLLAPSED, DIRECTORY_ICON_EXPANDED, material_file_icons, material_file_stems,
    material_file_suffixes, material_named_directory_icons,
};

/// A family of icon themes.
pub struct IconThemeFamily {
    /// The unique ID for the icon theme family.
    pub id: String,
    /// The name of the icon theme family.
    pub name: SharedString,
    /// The author of the icon theme family.
    pub author: SharedString,
    /// The list of icon themes in the family.
    pub themes: Vec<IconTheme>,
}

/// An icon theme.
#[derive(Debug, PartialEq)]
pub struct IconTheme {
    /// The unique ID for the icon theme.
    pub id: String,
    /// The name of the icon theme.
    pub name: SharedString,
    /// The appearance of the icon theme (e.g., light or dark).
    pub appearance: Appearance,
    /// The icons used for directories.
    pub directory_icons: DirectoryIcons,
    /// The icons used for named directories.
    pub named_directory_icons: HashMap<String, DirectoryIcons>,
    /// The icons used for chevrons.
    pub chevron_icons: ChevronIcons,
    /// The mapping of file stems to their associated icon keys.
    pub file_stems: HashMap<String, String>,
    /// The mapping of file suffixes to their associated icon keys.
    pub file_suffixes: HashMap<String, String>,
    /// The mapping of icon keys to icon definitions.
    pub file_icons: HashMap<String, IconDefinition>,
}

/// The icons used for directories.
#[derive(Debug, PartialEq, Clone)]
pub struct DirectoryIcons {
    /// The path to the icon to use for a collapsed directory.
    pub collapsed: Option<SharedString>,
    /// The path to the icon to use for an expanded directory.
    pub expanded: Option<SharedString>,
}

/// The icons used for chevrons.
#[derive(Debug, PartialEq)]
pub struct ChevronIcons {
    /// The path to the icon to use for a collapsed chevron.
    pub collapsed: Option<SharedString>,
    /// The path to the icon to use for an expanded chevron.
    pub expanded: Option<SharedString>,
}

/// An icon definition.
#[derive(Debug, PartialEq)]
pub struct IconDefinition {
    /// The path to the icon file.
    pub path: SharedString,
}

/// The name of the default icon theme.
///
/// This is the Material Icon Theme (vendored under
/// `assets/icons/file_icons/material/`), not zode's original hand-authored icon set.
/// See `icon_theme_material.rs` for the generated mapping tables and their provenance.
pub const DEFAULT_ICON_THEME_NAME: &str = "Material Icon Theme";

static DEFAULT_ICON_THEME: LazyLock<Arc<IconTheme>> = LazyLock::new(|| {
    Arc::new(IconTheme {
        id: "material".into(),
        name: DEFAULT_ICON_THEME_NAME.into(),
        appearance: Appearance::Dark,
        directory_icons: DirectoryIcons {
            collapsed: Some(DIRECTORY_ICON_COLLAPSED.into()),
            expanded: Some(DIRECTORY_ICON_EXPANDED.into()),
        },
        named_directory_icons: HashMap::from_iter(material_named_directory_icons().into_iter().map(
            |(name, (collapsed, expanded))| {
                (
                    name,
                    DirectoryIcons {
                        collapsed: Some(collapsed.into()),
                        expanded: Some(expanded.into()),
                    },
                )
            },
        )),
        // Material has no equivalent of zode's disclosure-triangle chevrons (VS Code
        // relies on the OS/tree-widget chrome for that, not a themed icon), so these
        // keep pointing at zode's own bundled chevron assets, unchanged.
        chevron_icons: ChevronIcons {
            collapsed: Some("icons/file_icons/chevron_right.svg".into()),
            expanded: Some("icons/file_icons/chevron_down.svg".into()),
        },
        file_stems: material_file_stems(),
        file_suffixes: material_file_suffixes(),
        file_icons: HashMap::from_iter(material_file_icons().into_iter().map(|(ty, path)| {
            (ty, IconDefinition { path: path.into() })
        })),
    })
});

/// Returns the default icon theme.
pub fn default_icon_theme() -> Arc<IconTheme> {
    DEFAULT_ICON_THEME.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icon_theme_is_named_material() {
        let theme = default_icon_theme();
        assert_eq!(theme.name.as_ref(), DEFAULT_ICON_THEME_NAME);
        assert_eq!(theme.name.as_ref(), "Material Icon Theme");
    }

    #[test]
    fn default_icon_theme_has_a_default_file_icon() {
        let theme = default_icon_theme();
        let default_icon = theme
            .file_icons
            .get("default")
            .expect("default icon theme must define a fallback \"default\" file icon");
        assert!(
            default_icon.path.starts_with("icons/file_icons/material/"),
            "expected the default file icon to be a vendored Material asset, got {}",
            default_icon.path
        );
    }

    #[test]
    fn default_icon_theme_resolves_common_file_suffixes() {
        let theme = default_icon_theme();

        let rust_key = theme
            .file_suffixes
            .get("rs")
            .expect("\"rs\" suffix must map to an icon key");
        let rust_icon = theme
            .file_icons
            .get(rust_key)
            .expect("the icon key for \"rs\" must have a registered file icon");
        assert!(rust_icon.path.starts_with("icons/file_icons/material/"));

        let toml_key = theme
            .file_suffixes
            .get("toml")
            .expect("\"toml\" suffix must map to an icon key");
        assert!(theme.file_icons.contains_key(toml_key));
    }

    #[test]
    fn default_icon_theme_resolves_docker_file_stem() {
        let theme = default_icon_theme();
        // The table is intentionally lowercase-only: `FileIcons::get_icon`
        // (crates/file_icons/src/file_icons.rs) lowercases the real filename before
        // looking it up here, so "Dockerfile" resolves via "dockerfile".
        assert!(
            theme.file_stems.contains_key("dockerfile"),
            "expected \"dockerfile\" to be recognized by file stem, not just suffix"
        );
        assert!(
            !theme.file_stems.contains_key("Dockerfile"),
            "the table must hold exactly one canonical (lowercase) spelling; \
             case-insensitivity is the lookup's job, not the table's"
        );
    }

    #[test]
    fn default_icon_theme_has_populated_directory_and_chevron_icons() {
        let theme = default_icon_theme();

        let collapsed = theme
            .directory_icons
            .collapsed
            .as_ref()
            .expect("default directory icon (collapsed) must be set");
        let expanded = theme
            .directory_icons
            .expanded
            .as_ref()
            .expect("default directory icon (expanded) must be set");
        assert!(collapsed.starts_with("icons/file_icons/material/"));
        assert!(expanded.starts_with("icons/file_icons/material/"));

        // Material has no chevron equivalent; zode's own bundled chevrons are kept.
        let chevron_collapsed = theme
            .chevron_icons
            .collapsed
            .as_ref()
            .expect("chevron icon (collapsed) must be set");
        let chevron_expanded = theme
            .chevron_icons
            .expanded
            .as_ref()
            .expect("chevron icon (expanded) must be set");
        assert!(!chevron_collapsed.starts_with("icons/file_icons/material/"));
        assert!(!chevron_expanded.starts_with("icons/file_icons/material/"));
    }

    #[test]
    fn default_icon_theme_has_named_directory_icons() {
        let theme = default_icon_theme();
        assert!(
            !theme.named_directory_icons.is_empty(),
            "Material provides named-folder icons; the table must not be empty"
        );

        let src = theme
            .named_directory_icons
            .get("src")
            .expect("\"src\" is a common folder name Material has an icon for");
        assert!(src.collapsed.as_ref().unwrap().starts_with("icons/file_icons/material/"));
        assert!(src.expanded.as_ref().unwrap().starts_with("icons/file_icons/material/"));
    }
}
