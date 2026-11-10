//! The background image (`window_background_image`), laid over the window's
//! contents at `1 - window_opacity`.
//!
//! Read from the path the user chose and never copied: the file stays theirs.
//! Its folder is watched, so deleting, moving or replacing the file shows up in
//! every window without a restart -- a deleted image simply goes away.
//!
//! With vibrancy on (`window_vibrancy`) the image is blurred here, once, on a background
//! thread: GPUI has no blur for an element, and blurring a downscaled copy is
//! both what makes it cheap and what a backdrop needs -- nobody reads detail
//! through a blur.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use fs::Fs;
use futures::StreamExt as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Global, ImageSource, IntoElement as _,
    ObjectFit, ParentElement as _, RenderImage, Styled as _, StyledImage as _, Task, div, img,
};
use image::{Frame, RgbaImage, imageops};
use settings::{Settings as _, SettingsStore};
use smallvec::SmallVec;
use theme_settings::ThemeSettings;

/// The long side the image is scaled down to before it is blurred. A backdrop
/// is seen through translucent surfaces, mostly blurred, and stretched to the
/// window: past this size the extra pixels cost memory and blur time and show
/// nothing.
const MAX_SIDE: u32 = 1920;

/// How strongly vibrancy blurs the image, as a gaussian sigma in pixels of the
/// scaled-down image: enough that the picture reads as colour and light behind
/// the text, not as detail competing with it.
const VIBRANCY_SIGMA: f32 = 20.;

/// How long a burst of file events is gathered before the image is read again,
/// so an editor saving the file in several writes is read once.
const WATCH_LATENCY: Duration = Duration::from_millis(250);

struct GlobalWindowBackdrop(Entity<WindowBackdrop>);
impl Global for GlobalWindowBackdrop {}

pub struct WindowBackdrop {
    fs: Arc<dyn Fs>,
    /// The path the image was last read for, and whether it was blurred.
    /// `None` when no image is set.
    wanted: Option<(PathBuf, bool)>,
    image: Option<Arc<RenderImage>>,
    _load: Option<Task<()>>,
    _watch: Option<Task<()>>,
}

pub fn init(fs: Arc<dyn Fs>, cx: &mut App) {
    let backdrop = cx.new(|cx| {
        let mut backdrop = WindowBackdrop {
            fs,
            wanted: None,
            image: None,
            _load: None,
            _watch: None,
        };
        backdrop.sync_with_settings(cx);
        backdrop
    });
    cx.observe_global::<SettingsStore>({
        let backdrop = backdrop.downgrade();
        move |cx| {
            backdrop
                .update(cx, |backdrop, cx| backdrop.sync_with_settings(cx))
                .ok();
        }
    })
    .detach();
    cx.set_global(GlobalWindowBackdrop(backdrop));
}

impl WindowBackdrop {
    fn sync_with_settings(&mut self, cx: &mut Context<Self>) {
        let settings = ThemeSettings::get_global(cx);
        let wanted = settings
            .window_background_image
            .clone()
            .map(|path| (path, settings.window_vibrancy));
        if wanted == self.wanted {
            return;
        }
        let path_changed =
            wanted.as_ref().map(|(path, _)| path) != self.wanted.as_ref().map(|(path, _)| path);
        self.wanted = wanted.clone();

        let Some((path, _)) = wanted else {
            self.image = None;
            self._load = None;
            self._watch = None;
            cx.refresh_windows();
            return;
        };
        if path_changed {
            self._watch = Some(self.watch(path, cx));
        }
        self.reload(cx);
    }

    /// Reads and blurs the image again, replacing it once ready. A file that
    /// is gone, or is not an image, leaves the window with no backdrop.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some((path, blurred)) = self.wanted.clone() else {
            return;
        };
        let fs = self.fs.clone();
        self._load = Some(cx.spawn(async move |this, cx| {
            let bytes = fs.load_bytes(&path).await;
            let image = match bytes {
                Ok(bytes) => cx
                    .background_spawn(async move { prepare_backdrop(&bytes, blurred) })
                    .await
                    .with_context(|| format!("reading the background image {}", path.display()))
                    .map_err(|error| log::warn!("{error:#}"))
                    .ok(),
                // Not an error to report: a deleted image is how a user takes it away.
                Err(_) => None,
            };
            this.update(cx, |this, cx| {
                this.image = image.map(Arc::new);
                cx.refresh_windows();
            })
            .ok();
        }));
    }

    /// Watches the image's folder rather than the file, so a file that is
    /// deleted and written again, as editors and downloads do, is still seen.
    fn watch(&self, path: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        let fs = self.fs.clone();
        cx.spawn(async move |this, cx| {
            let Some(folder) = path.parent().map(Path::to_path_buf) else {
                return;
            };
            let (mut events, _watcher) = fs.watch(&folder, WATCH_LATENCY).await;
            while let Some(batch) = events.next().await {
                if !batch.iter().any(|event| event.path == path) {
                    continue;
                }
                if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    break;
                }
            }
        })
    }
}

/// Decodes `bytes`, scales the image down to [`MAX_SIDE`], blurs it when
/// `blurred`, and lays it out the way GPUI draws images (BGRA).
pub fn prepare_backdrop(bytes: &[u8], blurred: bool) -> Result<RenderImage> {
    let mut image = image::load_from_memory(bytes)
        .context("not an image this build can read")?
        .into_rgba8();
    if image.width().max(image.height()) > MAX_SIDE {
        image = imageops::thumbnail(&image, scaled(image.width(), image.height()).0, {
            scaled(image.width(), image.height()).1
        });
    }
    if blurred {
        image = imageops::fast_blur(&image, VIBRANCY_SIGMA);
    }
    Ok(render_image(image))
}

/// The size that brings the long side to [`MAX_SIDE`], keeping the ratio.
fn scaled(width: u32, height: u32) -> (u32, u32) {
    let scale = MAX_SIDE as f32 / width.max(height) as f32;
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

fn render_image(mut image: RgbaImage) -> RenderImage {
    for pixel in image.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    RenderImage::new(SmallVec::from_elem(Frame::new(image), 1))
}

/// The background image, blurred, once it has been read. `None` when none is
/// set or it could not be read.
pub fn backdrop_image(cx: &App) -> Option<Arc<RenderImage>> {
    cx.try_global::<GlobalWindowBackdrop>()?
        .0
        .read(cx)
        .image
        .clone()
}

/// Where the platform cannot draw the image behind the window's backgrounds
/// (`Window::supports_vibrancy`), it is laid over the window's contents at
/// `ThemeSettings::background_image_opacity` instead: the window at
/// `opacity` over the image looks like the image at `1 - opacity` over the
/// window, evenly everywhere, text included.
///
/// Draws no hitbox, so the pointer reaches the window beneath it.
pub fn render_window_backdrop(cx: &App) -> Option<AnyElement> {
    let opacity = ThemeSettings::get_global(cx).background_image_opacity();
    if opacity <= 0. {
        return None;
    }
    let image = backdrop_image(cx)?;
    Some(
        div()
            .absolute()
            .inset_0()
            .size_full()
            .opacity(opacity)
            .child(
                img(ImageSource::Render(image))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba};
    use std::io::Cursor;

    /// A PNG whose left half is black and right half white: a sharp edge a
    /// blur has to soften.
    fn half_black_png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_fn(width, height, |x, _| {
            if x < width / 2 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .expect("encodes");
        bytes
    }

    fn pixel(image: &RenderImage, x: u32, y: u32) -> [u8; 4] {
        let frame = image.as_bytes(0).expect("one frame");
        let width = image.size(0).width.0 as u32;
        let at = ((y * width + x) * 4) as usize;
        [frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]
    }

    #[test]
    fn no_blur_keeps_the_edge_sharp() {
        let image = prepare_backdrop(&half_black_png(64, 16), false).expect("reads");
        assert_eq!(pixel(&image, 31, 8), [0, 0, 0, 255]);
        assert_eq!(pixel(&image, 32, 8), [255, 255, 255, 255]);
    }

    #[test]
    fn vibrancy_softens_the_edge() {
        let image = prepare_backdrop(&half_black_png(64, 16), true).expect("reads");
        let next_to_the_edge = pixel(&image, 31, 8);
        assert!(
            next_to_the_edge[0] > 0 && next_to_the_edge[0] < 255,
            "the pixel beside the edge is grey once blurred, got {next_to_the_edge:?}"
        );
    }

    #[test]
    fn a_large_image_is_scaled_down_keeping_its_shape() {
        let image = prepare_backdrop(&half_black_png(3840, 960), false).expect("reads");
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (1920, 480));
    }

    #[test]
    fn a_file_that_is_not_an_image_is_an_error_not_a_panic() {
        assert!(prepare_backdrop(b"not a picture", true).is_err());
    }

    fn has_image(cx: &mut gpui::TestAppContext) -> bool {
        cx.read(|cx| {
            cx.global::<GlobalWindowBackdrop>()
                .0
                .read(cx)
                .image
                .is_some()
        })
    }

    fn set_image(path: Option<&str>, cx: &mut gpui::TestAppContext) {
        use gpui::UpdateGlobal as _;

        let path = path.map(|path| settings::WindowBackgroundImagePath(path.to_string()));
        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.theme.window_background_image = path.clone();
                });
            });
        });
        cx.run_until_parked();
    }

    /// The image is the user's file, read where it lies: deleting it takes
    /// it out of the window, and clearing the setting does too.
    #[gpui::test]
    async fn deleting_the_file_or_clearing_the_setting_removes_the_image(
        cx: &mut gpui::TestAppContext,
    ) {
        let fs = fs::FakeFs::new(cx.executor());
        fs.insert_tree("/pictures", serde_json::json!({})).await;
        fs.insert_file("/pictures/wall.png", half_black_png(64, 16))
            .await;
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            init(fs.clone(), cx);
        });

        set_image(Some("/pictures/wall.png"), cx);
        assert!(has_image(cx), "the chosen file is read");

        fs.remove_file(Path::new("/pictures/wall.png"), Default::default())
            .await
            .expect("removes");
        cx.executor().advance_clock(WATCH_LATENCY * 2);
        cx.run_until_parked();
        assert!(!has_image(cx), "a deleted file leaves the window");

        fs.insert_file("/pictures/wall.png", half_black_png(64, 16))
            .await;
        cx.executor().advance_clock(WATCH_LATENCY * 2);
        cx.run_until_parked();
        assert!(has_image(cx), "and putting it back brings it back");

        set_image(None, cx);
        assert!(!has_image(cx), "clearing the setting removes it too");
    }
}
