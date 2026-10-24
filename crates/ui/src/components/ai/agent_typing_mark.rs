use std::time::Duration;

use gpui::{Animation, AnimationExt, Hsla, Transformation, point, px};

use crate::BASE_REM_SIZE_IN_PX;
use crate::prelude::*;
use crate::traits::transformable::Transformable;
use crate::utils::ensure_minimum_contrast;

const TYPING_CYCLE: Duration = Duration::from_millis(1200);
const BOB_AMPLITUDE_PX: f32 = 1.0;
const DOT_OFFSETS: [f32; 3] = [0.0, 0.15, 0.30];
const KEY_ATTACK: f32 = 0.08;
const KEY_DECAY: f32 = 0.25;
const RESTING_ALPHA: f32 = 0.3;
const DOT_MINIMUM_CONTRAST: f32 = 45.0;

struct TypingFrameValues {
    lift: f32,
    dot_alphas: [f32; 3],
}

fn typing_frame(delta: f32) -> TypingFrameValues {
    let lift = 0.5 - 0.5 * (std::f32::consts::TAU * delta).cos();
    // The last key is released well before the cycle ends. Without that all-dim
    // pause the dots read as a marquee instead of typing.
    let dot_alphas = DOT_OFFSETS.map(|offset| {
        let local = (delta - offset).rem_euclid(1.0);
        let brightness = if local < KEY_ATTACK {
            local / KEY_ATTACK
        } else if local < KEY_ATTACK + KEY_DECAY {
            1.0 - (local - KEY_ATTACK) / KEY_DECAY
        } else {
            0.0
        };
        RESTING_ALPHA + (1.0 - RESTING_ALPHA) * brightness
    });
    TypingFrameValues { lift, dot_alphas }
}

/// An agent's glyph "typing" while it answers: the glyph bobs by a pixel and three
/// dots under it light in turn. The box is exactly `size` square, so it can stand
/// in for an [`Icon`] of the same size.
#[derive(IntoElement)]
pub struct AgentTypingMark {
    id: ElementId,
    icon: IconName,
    color: Color,
    size: IconSize,
    background: Option<Hsla>,
}

impl AgentTypingMark {
    pub fn new(id: impl Into<ElementId>, icon: IconName, color: Color) -> Self {
        Self {
            id: id.into(),
            icon,
            color,
            size: IconSize::Small,
            background: None,
        }
    }

    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// What the mark is drawn on, so the dots can be kept legible against it.
    /// The panel background when unset; a tab, or anything else that paints its
    /// own, has to say so.
    pub fn background(mut self, background: Hsla) -> Self {
        self.background = Some(background);
        self
    }

    fn dot_color(&self, cx: &App) -> Hsla {
        ensure_minimum_contrast(
            self.color.color(cx),
            self.background
                .unwrap_or_else(|| cx.theme().colors().panel_background),
            DOT_MINIMUM_CONTRAST,
        )
    }
}

impl RenderOnce for AgentTypingMark {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let frame = TypingFrame {
            id: self.id.clone(),
            icon: self.icon,
            color: self.color,
            dot_color: self.dot_color(cx),
            size: self.size,
            phase: 0.0,
        };
        frame.with_animation(
            self.id,
            Animation::new(TYPING_CYCLE).repeat(),
            |mut frame, delta| {
                frame.phase = delta;
                frame
            },
        )
    }
}

#[derive(IntoElement)]
struct TypingFrame {
    id: ElementId,
    icon: IconName,
    color: Color,
    dot_color: Hsla,
    size: IconSize,
    phase: f32,
}

impl RenderOnce for TypingFrame {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let values = typing_frame(self.phase);
        let side = self.size.rems().0 * BASE_REM_SIZE_IN_PX;
        let lift = rems_from_px(BOB_AMPLITUDE_PX).to_pixels(window.rem_size()) * values.lift;
        let dot_color = self.dot_color;
        let id = self.id;

        div()
            .debug_selector(move || format!("agent-typing-mark:{id}"))
            .relative()
            .flex_none()
            .size(rems_from_px(side))
            .child(
                div()
                    .absolute()
                    .top(rems_from_px(1.))
                    .left(rems_from_px(2.))
                    .child(
                        Icon::new(self.icon)
                            .size(IconSize::Custom(rems_from_px(side - 4.)))
                            .color(self.color)
                            .transform(Transformation::translate(point(px(0.), -lift))),
                    ),
            )
            .child(
                h_flex()
                    .absolute()
                    .bottom_0()
                    .left(rems_from_px((side - 8.) / 2.))
                    .gap(rems_from_px(1.))
                    .children(values.dot_alphas.map(|alpha| {
                        div()
                            .size(rems_from_px(2.))
                            .rounded_full()
                            .bg(dot_color.opacity(alpha))
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, hsla};

    use super::*;
    use crate::utils::apca_contrast;

    #[test]
    fn the_dots_light_in_order_then_rest_together() {
        for (peak_delta, lit) in [(0.08, 0), (0.23, 1), (0.38, 2)] {
            let alphas = typing_frame(peak_delta).dot_alphas;
            assert!(
                (alphas[lit] - 1.0).abs() < 1e-4,
                "dot {lit} should peak at {peak_delta}, got {alphas:?}"
            );
            for (index, alpha) in alphas.iter().enumerate() {
                if index != lit {
                    assert!(
                        *alpha < alphas[lit],
                        "dot {index} outshines dot {lit} at {peak_delta}: {alphas:?}"
                    );
                }
            }
        }
        assert_eq!(typing_frame(0.85).dot_alphas, [RESTING_ALPHA; 3]);

        assert!(typing_frame(0.0).lift.abs() < 1e-6);
        assert!((typing_frame(0.5).lift - 1.0).abs() < 1e-6);
        for step in 0..100 {
            let lift = typing_frame(step as f32 / 100.0).lift;
            assert!((0.0..=1.0).contains(&lift), "lift {lift} out of range");
        }
    }

    #[gpui::test]
    fn the_dots_are_kept_legible_against_the_background_the_mark_sits_on(cx: &mut TestAppContext) {
        cx.update(|cx| {
            theme::init(theme::LoadThemes::JustBase, cx);

            let pale = hsla(0.1, 0.8, 0.8, 1.0);
            let dark = hsla(0.0, 0.0, 0.05, 1.0);
            let light = hsla(0.0, 0.0, 0.95, 1.0);
            let mark = || AgentTypingMark::new("mark", IconName::Sparkle, Color::Custom(pale));

            let on_dark = mark().background(dark).dot_color(cx);
            let on_light = mark().background(light).dot_color(cx);

            assert_eq!(on_dark, pale, "already legible on dark, so left alone");
            assert_ne!(on_light, pale, "pale on near-white has to be darkened");
            assert!(
                apca_contrast(on_light, light).abs() >= DOT_MINIMUM_CONTRAST,
                "{on_light:?} is below the floor on the background it was given"
            );

            assert_eq!(
                mark().dot_color(cx),
                ensure_minimum_contrast(
                    pale,
                    cx.theme().colors().panel_background,
                    DOT_MINIMUM_CONTRAST
                ),
                "no background given means the panel's"
            );
        });
    }
}
