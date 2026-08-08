// This crate was essentially pulled out verbatim from main `zed` crate to avoid having to run RustEmbed macro whenever zed has to be rebuilt. It saves a second or two on an incremental build.

use anyhow::Context as _;
use gpui::{App, AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "fonts/**/*"]
#[include = "icons/**/*"]
#[include = "images/**/*"]
#[include = "themes/**/*"]
#[exclude = "themes/src/*"]
#[include = "*.md"]
#[exclude = "*.DS_Store"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Self::get(path)
            .map(|f| Some(f.data))
            .with_context(|| format!("loading asset at path {path:?}"))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter_map(|p| {
                if p.starts_with(path) {
                    Some(p.into())
                } else {
                    None
                }
            })
            .collect())
    }
}

impl Assets {
    /// Populate the [`TextSystem`] of the given [`AppContext`] with all `.ttf` fonts in the `fonts` directory.
    pub fn load_fonts(&self, cx: &App) -> anyhow::Result<()> {
        let font_paths = self.list("fonts")?;
        let mut embedded_fonts = Vec::new();
        for font_path in font_paths {
            if font_path.ends_with(".ttf") {
                let font_bytes = cx
                    .asset_source()
                    .load(&font_path)?
                    .expect("Assets should never return None");
                embedded_fonts.push(font_bytes);
            }
        }

        cx.text_system().add_fonts(embedded_fonts)
    }

    #[cfg(test)]
    fn embedded_font_paths() -> Vec<String> {
        Self::iter()
            .filter(|path| path.ends_with(".ttf"))
            .map(|path| path.to_string())
            .collect()
    }

    pub fn load_test_fonts(&self, cx: &App) {
        cx.text_system()
            .add_fonts(vec![
                self.load("fonts/lilex/Lilex-Regular.ttf").unwrap().unwrap(),
            ])
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A font the settings default to is only real once it reaches the binary.
    // The include globs above take whole directories, so a new family arrives by
    // side effect -- or silently fails to, and the app quietly renders something
    // else on every machine except the one that installed the font by hand.
    #[test]
    fn the_default_font_families_are_embedded() {
        let embedded = Assets::embedded_font_paths();

        for family in ["JetBrainsMono", "JetBrainsMonoNL"] {
            for style in ["Regular", "Italic", "Bold", "BoldItalic"] {
                let path = format!("fonts/jetbrains-mono/{family}-{style}.ttf");
                assert!(
                    embedded.contains(&path),
                    "not embedded: {path}\nembedded fonts: {embedded:#?}"
                );
            }
        }
    }
}
