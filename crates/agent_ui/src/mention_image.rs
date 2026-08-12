//! Turning a pasted or mentioned image into something an agent can read.
//!
//! Upstream lived in `language_model`, whose `LanguageModelImage` carried the
//! same two things an ACP image block needs: base64 PNG bytes and the size it
//! ended up. That crate went with the native agent, so the conversion comes
//! across here rather than being reinvented — the size limits below are the
//! provider constraints upstream had already learned.

use anyhow::Result;
use base64::write::EncoderWriter;
use gpui::{App, AppContext as _, DevicePixels, Image, ImageFormat, ObjectFit, Task, point, px, size};
use image::{GenericImageView as _, codecs::png::PngEncoder};
use std::io::{Cursor, Write as _};
use std::sync::Arc;
use util::ResultExt as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone)]
pub struct MentionImageData {
    pub size: Option<ImageSize>,
    pub source: std::sync::Arc<str>,
}

/// Anthropic wants uploaded images to be smaller than this in both dimensions.
const ANTHROPIC_SIZE_LIMIT: f32 = 1568.;

/// Default per-image hard limit (in bytes) for the encoded image payload we send upstream.
///
/// NOTE: `LanguageModelImage.source` is base64-encoded PNG bytes (without the `data:` prefix).
/// This limit is enforced on the encoded PNG bytes *before* base64 encoding.
const DEFAULT_IMAGE_MAX_BYTES: usize = 5 * 1024 * 1024;

/// Conservative cap on how many times we'll attempt to shrink/re-encode an image to fit
/// `DEFAULT_IMAGE_MAX_BYTES`.
const MAX_IMAGE_DOWNSCALE_PASSES: usize = 8;

/// Extension trait for `LanguageModelImage` that provides GPUI-dependent functionality.
pub trait MentionImageExt {
    const FORMAT: ImageFormat;
    fn from_image(data: Arc<Image>, cx: &mut App) -> Task<Option<MentionImageData>>;
}

impl MentionImageExt for MentionImageData {
    const FORMAT: ImageFormat = ImageFormat::Png;

    fn from_image(data: Arc<Image>, cx: &mut App) -> Task<Option<MentionImageData>> {
        cx.background_spawn(async move {
            let image_bytes = Cursor::new(data.bytes());
            let dynamic_image = match data.format() {
                ImageFormat::Png => image::codecs::png::PngDecoder::new(image_bytes)
                    .and_then(image::DynamicImage::from_decoder),
                ImageFormat::Jpeg => image::codecs::jpeg::JpegDecoder::new(image_bytes)
                    .and_then(image::DynamicImage::from_decoder),
                ImageFormat::Webp => image::codecs::webp::WebPDecoder::new(image_bytes)
                    .and_then(image::DynamicImage::from_decoder),
                ImageFormat::Gif => image::codecs::gif::GifDecoder::new(image_bytes)
                    .and_then(image::DynamicImage::from_decoder),
                ImageFormat::Bmp => image::codecs::bmp::BmpDecoder::new(image_bytes)
                    .and_then(image::DynamicImage::from_decoder),
                ImageFormat::Tiff => image::codecs::tiff::TiffDecoder::new(image_bytes)
                    .and_then(image::DynamicImage::from_decoder),
                _ => return None,
            }
            .log_err()?;

            let width = dynamic_image.width();
            let height = dynamic_image.height();
            let image_size = size(DevicePixels(width as i32), DevicePixels(height as i32));

            // First apply any provider-specific dimension constraints we know about (Anthropic).
            let mut processed_image = if image_size.width.0 > ANTHROPIC_SIZE_LIMIT as i32
                || image_size.height.0 > ANTHROPIC_SIZE_LIMIT as i32
            {
                let new_bounds = ObjectFit::ScaleDown.get_bounds(
                    gpui::Bounds {
                        origin: point(px(0.0), px(0.0)),
                        size: size(px(ANTHROPIC_SIZE_LIMIT), px(ANTHROPIC_SIZE_LIMIT)),
                    },
                    image_size,
                );
                dynamic_image.resize(
                    new_bounds.size.width.into(),
                    new_bounds.size.height.into(),
                    image::imageops::FilterType::Triangle,
                )
            } else {
                dynamic_image
            };

            // Then enforce a default per-image size cap on the encoded PNG bytes.
            //
            // We always send PNG bytes (either original PNG bytes, or re-encoded PNG) base64'd.
            // The upstream provider limit we want to respect is effectively on the binary image
            // payload size, so we enforce against the encoded PNG bytes before base64 encoding.
            let mut encoded_png = encode_png_bytes(&processed_image).log_err()?;
            for _pass in 0..MAX_IMAGE_DOWNSCALE_PASSES {
                if encoded_png.len() <= DEFAULT_IMAGE_MAX_BYTES {
                    break;
                }

                // Scale down geometrically to converge quickly. We don't know the final PNG size
                // as a function of pixels, so we iteratively shrink.
                let (w, h) = processed_image.dimensions();
                if w <= 1 || h <= 1 {
                    break;
                }

                // Shrink by ~15% each pass (0.85). This is a compromise between speed and
                // preserving image detail.
                let new_w = ((w as f32) * 0.85).round().max(1.0) as u32;
                let new_h = ((h as f32) * 0.85).round().max(1.0) as u32;

                processed_image =
                    processed_image.resize(new_w, new_h, image::imageops::FilterType::Triangle);
                encoded_png = encode_png_bytes(&processed_image).log_err()?;
            }

            if encoded_png.len() > DEFAULT_IMAGE_MAX_BYTES {
                // Still too large after multiple passes; treat as non-convertible for now.
                // (Provider-specific handling can be introduced later.)
                return None;
            }

            // Now base64 encode the PNG bytes.
            let base64_image = encode_bytes_as_base64(encoded_png.as_slice()).log_err()?;

            // SAFETY: The base64 encoder should not produce non-UTF8.
            let source = unsafe { String::from_utf8_unchecked(base64_image) };

            let (final_width, final_height) = processed_image.dimensions();

            Some(MentionImageData {
                size: Some(ImageSize {
                    width: final_width as i32,
                    height: final_height as i32,
                }),
                source: source.into(),
            })
        })
    }
}

fn encode_png_bytes(image: &image::DynamicImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    image.write_with_encoder(PngEncoder::new(&mut png))?;
    Ok(png)
}

fn encode_bytes_as_base64(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut base64_image = Vec::new();
    {
        let mut base64_encoder = EncoderWriter::new(
            Cursor::new(&mut base64_image),
            &base64::engine::general_purpose::STANDARD,
        );
        base64_encoder.write_all(bytes)?;
    }
    Ok(base64_image)
}
