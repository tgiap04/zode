use std::sync::Arc;
use std::{fmt::Debug, path::Path};

use anyhow::Result;
use collections::HashMap;
use gpui::{App, AssetSource, Global, SharedString};
use parking_lot::RwLock;
use thiserror::Error;

use crate::{
    Appearance, AppearanceContent, ChevronIcons, DEFAULT_ICON_THEME_NAME, DirectoryIcons,
    IconDefinition, IconTheme, IconThemeFamilyContent, Theme, ThemeFamily, default_icon_theme,
};

/// The metadata for a theme.
#[derive(Debug, Clone)]
pub struct ThemeMeta {
    /// The name of the theme.
    pub name: SharedString,
    /// The appearance of the theme.
    pub appearance: Appearance,
}

/// An error indicating that the theme with the given name was not found.
#[derive(Debug, Error, Clone)]
#[error("theme not found: {0}")]
pub struct ThemeNotFoundError(pub SharedString);

/// An error indicating that the icon theme with the given name was not found.
#[derive(Debug, Error, Clone)]
#[error("icon theme not found: {0}")]
pub struct IconThemeNotFoundError(pub SharedString);

/// The global [`ThemeRegistry`].
///
/// This newtype exists for obtaining a unique [`TypeId`](std::any::TypeId) when
/// inserting the [`ThemeRegistry`] into the context as a global.
///
/// This should not be exposed outside of this module.
#[derive(Default)]
struct GlobalThemeRegistry(Arc<ThemeRegistry>);

impl std::ops::DerefMut for GlobalThemeRegistry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl std::ops::Deref for GlobalThemeRegistry {
    type Target = Arc<ThemeRegistry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Global for GlobalThemeRegistry {}

struct ThemeRegistryState {
    themes: HashMap<SharedString, Arc<Theme>>,
    icon_themes: HashMap<SharedString, Arc<IconTheme>>,
    /// Whether the extensions have been loaded yet.
    extensions_loaded: bool,
}

/// The registry for themes.
pub struct ThemeRegistry {
    state: RwLock<ThemeRegistryState>,
    assets: Box<dyn AssetSource>,
}

impl ThemeRegistry {
    /// Returns the global [`ThemeRegistry`].
    pub fn global(cx: &App) -> Arc<Self> {
        cx.global::<GlobalThemeRegistry>().0.clone()
    }

    /// Returns the global [`ThemeRegistry`].
    ///
    /// Inserts a default [`ThemeRegistry`] if one does not yet exist.
    pub fn default_global(cx: &mut App) -> Arc<Self> {
        cx.default_global::<GlobalThemeRegistry>().0.clone()
    }

    /// Returns the global [`ThemeRegistry`] if it exists.
    pub fn try_global(cx: &mut App) -> Option<Arc<Self>> {
        cx.try_global::<GlobalThemeRegistry>().map(|t| t.0.clone())
    }

    /// Sets the global [`ThemeRegistry`].
    pub(crate) fn set_global(assets: Box<dyn AssetSource>, cx: &mut App) {
        cx.set_global(GlobalThemeRegistry(Arc::new(ThemeRegistry::new(assets))));
    }

    /// Returns the asset source used by this registry.
    pub fn assets(&self) -> &dyn AssetSource {
        self.assets.as_ref()
    }

    /// Creates a new [`ThemeRegistry`] with the given [`AssetSource`].
    pub fn new(assets: Box<dyn AssetSource>) -> Self {
        let registry = Self {
            state: RwLock::new(ThemeRegistryState {
                themes: HashMap::default(),
                icon_themes: HashMap::default(),
                extensions_loaded: false,
            }),
            assets,
        };

        // We're loading the Zed default theme, as we need a theme to be loaded
        // for tests.
        registry.insert_theme_families([crate::fallback_themes::zed_default_themes()]);

        let default_icon_theme = crate::default_icon_theme();
        registry
            .state
            .write()
            .icon_themes
            .insert(default_icon_theme.name.clone(), default_icon_theme);

        registry
    }

    /// Returns whether the extensions have been loaded.
    pub fn extensions_loaded(&self) -> bool {
        self.state.read().extensions_loaded
    }

    /// Sets the flag indicating that the extensions have loaded.
    pub fn set_extensions_loaded(&self) {
        self.state.write().extensions_loaded = true;
    }

    /// Inserts the given theme families into the registry.
    pub fn insert_theme_families(&self, families: impl IntoIterator<Item = ThemeFamily>) {
        for family in families.into_iter() {
            self.insert_themes(family.themes);
        }
    }

    /// Registers theme families for use in tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn register_test_themes(&self, families: impl IntoIterator<Item = ThemeFamily>) {
        self.insert_theme_families(families);
    }

    /// Registers icon themes for use in tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn register_test_icon_themes(&self, icon_themes: impl IntoIterator<Item = IconTheme>) {
        let mut state = self.state.write();
        for icon_theme in icon_themes {
            state
                .icon_themes
                .insert(icon_theme.name.clone(), Arc::new(icon_theme));
        }
    }

    /// Inserts the given themes into the registry.
    pub fn insert_themes(&self, themes: impl IntoIterator<Item = Theme>) {
        let mut state = self.state.write();
        for theme in themes.into_iter() {
            state.themes.insert(theme.name.clone(), Arc::new(theme));
        }
    }

    /// Removes the themes with the given names from the registry.
    pub fn remove_user_themes(&self, themes_to_remove: &[SharedString]) {
        self.state
            .write()
            .themes
            .retain(|name, _| !themes_to_remove.contains(name))
    }

    /// Removes all themes from the registry.
    pub fn clear(&self) {
        self.state.write().themes.clear();
    }

    /// Returns the names of all themes in the registry.
    pub fn list_names(&self) -> Vec<SharedString> {
        let mut names = self.state.read().themes.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    /// Returns the metadata of all themes in the registry.
    pub fn list(&self) -> Vec<ThemeMeta> {
        self.state
            .read()
            .themes
            .values()
            .map(|theme| ThemeMeta {
                name: theme.name.clone(),
                appearance: theme.appearance(),
            })
            .collect()
    }

    /// Returns the theme with the given name.
    pub fn get(&self, name: &str) -> Result<Arc<Theme>, ThemeNotFoundError> {
        self.state
            .read()
            .themes
            .get(name)
            .ok_or_else(|| ThemeNotFoundError(name.to_string().into()))
            .cloned()
    }

    /// Returns the default icon theme.
    pub fn default_icon_theme(&self) -> Result<Arc<IconTheme>, IconThemeNotFoundError> {
        self.get_icon_theme(DEFAULT_ICON_THEME_NAME)
    }

    /// Returns the metadata of all icon themes in the registry.
    ///
    /// Always exactly one: this build ships a single icon theme and does not offer
    /// a choice of them — see [`Self::get_icon_theme`].
    pub fn list_icon_themes(&self) -> Vec<ThemeMeta> {
        self.default_icon_theme()
            .map(|theme| {
                vec![ThemeMeta {
                    name: theme.name.clone(),
                    appearance: theme.appearance,
                }]
            })
            .unwrap_or_default()
    }

    /// Returns this build's icon theme, whatever was asked for.
    ///
    /// **The name is deliberately ignored.** The icon set is fixed: a `icon_theme`
    /// written into settings, an icon theme contributed by an extension, and the
    /// icon-theme picker all resolve here, and all get the same answer. That is the
    /// intent — the file icons are part of what this build *is*, not a preference.
    ///
    /// Locked here rather than by deleting the setting and every control that
    /// reaches it: the resolution path is shared with the *colour* theme, which is
    /// freely configurable, and pulling the icon half out of those functions would
    /// put a working feature at risk to remove a dead one.
    pub fn get_icon_theme(&self, _name: &str) -> Result<Arc<IconTheme>, IconThemeNotFoundError> {
        self.state
            .read()
            .icon_themes
            .get(DEFAULT_ICON_THEME_NAME)
            .ok_or_else(|| IconThemeNotFoundError(DEFAULT_ICON_THEME_NAME.into()))
            .cloned()
    }

    /// Removes the icon themes with the given names from the registry.
    ///
    /// The built-in icon theme is not removable. Since [`Self::get_icon_theme`]
    /// resolves everything to that one entry, dropping it would leave every file
    /// and folder in the editor with no icon at all — and an extension being
    /// uninstalled is enough to reach this with the reserved name in hand.
    pub fn remove_icon_themes(&self, icon_themes_to_remove: &[SharedString]) {
        self.state.write().icon_themes.retain(|name, _| {
            name.as_ref() == DEFAULT_ICON_THEME_NAME || !icon_themes_to_remove.contains(name)
        })
    }

    /// Loads the icon theme from the icon theme family and adds it to the registry.
    ///
    /// The `icons_root_dir` parameter indicates the root directory from which
    /// the relative paths to icons in the theme should be resolved against.
    pub fn load_icon_theme(
        &self,
        icon_theme_family: IconThemeFamilyContent,
        icons_root_dir: &Path,
    ) -> Result<()> {
        let resolve_icon_path = |path: SharedString| {
            icons_root_dir
                .join(path.as_ref())
                .to_string_lossy()
                .to_string()
                .into()
        };

        let default_icon_theme = default_icon_theme();

        let mut state = self.state.write();
        for icon_theme in icon_theme_family.themes {
            let mut file_stems = default_icon_theme.file_stems.clone();
            file_stems.extend(icon_theme.file_stems);

            let mut file_suffixes = default_icon_theme.file_suffixes.clone();
            file_suffixes.extend(icon_theme.file_suffixes);

            let mut named_directory_icons = default_icon_theme.named_directory_icons.clone();
            named_directory_icons.extend(icon_theme.named_directory_icons.into_iter().map(
                |(key, value)| {
                    (
                        key,
                        DirectoryIcons {
                            collapsed: value.collapsed.map(resolve_icon_path),
                            expanded: value.expanded.map(resolve_icon_path),
                        },
                    )
                },
            ));

            let icon_theme = IconTheme {
                id: uuid::Uuid::new_v4().to_string(),
                name: icon_theme.name.into(),
                appearance: match icon_theme.appearance {
                    AppearanceContent::Light => Appearance::Light,
                    AppearanceContent::Dark => Appearance::Dark,
                },
                directory_icons: DirectoryIcons {
                    collapsed: icon_theme.directory_icons.collapsed.map(resolve_icon_path),
                    expanded: icon_theme.directory_icons.expanded.map(resolve_icon_path),
                },
                named_directory_icons,
                chevron_icons: ChevronIcons {
                    collapsed: icon_theme.chevron_icons.collapsed.map(resolve_icon_path),
                    expanded: icon_theme.chevron_icons.expanded.map(resolve_icon_path),
                },
                file_stems,
                file_suffixes,
                file_icons: icon_theme
                    .file_icons
                    .into_iter()
                    .map(|(key, icon)| {
                        (
                            key,
                            IconDefinition {
                                path: resolve_icon_path(icon.path),
                            },
                        )
                    })
                    .collect(),
            };

            // The built-in name is reserved. Every lookup resolves to this one key
            // now (see `get_icon_theme`), so an extension shipping an icon theme
            // that happens to carry the same name would replace every file icon in
            // the editor for anyone who installs it — and with the user's own
            // `icon_theme` setting no longer consulted, there would be no way to
            // pick something else. Loading it is fine; taking the name is not.
            if icon_theme.name.as_ref() == DEFAULT_ICON_THEME_NAME {
                log::warn!(
                    "an extension contributed an icon theme named {DEFAULT_ICON_THEME_NAME:?}, \
                     which is the built-in set's reserved name -- ignoring it"
                );
                continue;
            }

            state
                .icon_themes
                .insert(icon_theme.name.clone(), Arc::new(icon_theme));
        }

        Ok(())
    }
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::new(Box::new(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IconThemeContent;

    /// An icon theme family the way an extension ships one — enough to register,
    /// nothing more.
    fn contributed_family(theme_name: &str) -> IconThemeFamilyContent {
        IconThemeFamilyContent {
            name: "Contributed".into(),
            author: "an extension".into(),
            themes: vec![IconThemeContent {
                name: theme_name.into(),
                appearance: AppearanceContent::Dark,
                directory_icons: Default::default(),
                named_directory_icons: Default::default(),
                chevron_icons: Default::default(),
                file_stems: Default::default(),
                file_suffixes: Default::default(),
                file_icons: Default::default(),
            }],
        }
    }

    /// The locked contract, asserted directly rather than inferred from the
    /// icons that come out the other end.
    #[test]
    fn get_icon_theme_ignores_the_name_argument() {
        let registry = ThemeRegistry::default();
        registry
            .load_icon_theme(contributed_family("Something Else"), Path::new("icons"))
            .unwrap();

        for asked_for in [
            "Something Else",
            "a name nothing registered",
            "",
            DEFAULT_ICON_THEME_NAME,
        ] {
            assert_eq!(
                registry.get_icon_theme(asked_for).unwrap().name.as_ref(),
                DEFAULT_ICON_THEME_NAME,
                "asking for {asked_for:?} must still resolve to the built-in set"
            );
        }
    }

    /// A registered-but-unreachable theme must not be advertised as a choice —
    /// this is what the settings JSON schema enumerates.
    #[test]
    fn list_icon_themes_reports_exactly_one() {
        let registry = ThemeRegistry::default();
        registry
            .load_icon_theme(contributed_family("Something Else"), Path::new("icons"))
            .unwrap();

        let listed = registry.list_icon_themes();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name.as_ref(), DEFAULT_ICON_THEME_NAME);
    }

    /// The name is reserved. An extension taking it would silently replace every
    /// file icon in the editor, and with the user's setting no longer consulted
    /// there would be nothing to switch back to.
    #[test]
    fn an_extension_cannot_take_the_built_in_name() {
        let registry = ThemeRegistry::default();
        let built_in = registry.get_icon_theme("").unwrap();

        registry
            .load_icon_theme(
                contributed_family(DEFAULT_ICON_THEME_NAME),
                Path::new("icons"),
            )
            .unwrap();

        let after = registry.get_icon_theme("").unwrap();
        assert_eq!(after.id, built_in.id, "the built-in set must still be the one served");
        assert!(!after.file_suffixes.is_empty(), "and it must still carry its associations");
    }

    /// Uninstalling an extension must not be able to take the icons with it.
    #[test]
    fn the_built_in_icon_theme_cannot_be_removed() {
        let registry = ThemeRegistry::default();
        registry.remove_icon_themes(&[DEFAULT_ICON_THEME_NAME.into()]);

        assert_eq!(
            registry.get_icon_theme("").unwrap().name.as_ref(),
            DEFAULT_ICON_THEME_NAME
        );
    }
}
