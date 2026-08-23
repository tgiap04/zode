//! Picking a colour: a saturation/value area, a hue bar, and a hex field.
//!
//! Written because there was nothing to reuse — no picker, no slider, no colour
//! component anywhere in this crate. Deliberately small: no alpha, because
//! nothing asking for a colour here wants transparency, and a control nobody
//! uses still has to be maintained.
//!
//! The geometry lives in free functions at the top. Every off-by-one in a picker
//! is in the mapping between a point and a colour, and that mapping is the only
//! part that can be tested without a window.

use crate::prelude::*;
use gpui::{
    Bounds, DismissEvent, EventEmitter, Hsla, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Size, canvas, hsla, linear_color_stop, linear_gradient, px,
};

/// Where a colour sits inside the saturation/value area, as two 0..=1 fractions.
///
/// Clamped rather than refused: a drag that leaves the area keeps painting from
/// the nearest edge, which is what a pointer held down outside the square is
/// asking for.
pub fn position_to_saturation_value(local: Point<Pixels>, size: Size<Pixels>) -> (f32, f32) {
    let width = f32::from(size.width).max(1.);
    let height = f32::from(size.height).max(1.);
    let saturation = (f32::from(local.x) / width).clamp(0., 1.);
    // Down is darker, the way every picker of this shape reads.
    let value = 1. - (f32::from(local.y) / height).clamp(0., 1.);
    (saturation, value)
}

/// The inverse, for drawing the handle where the current colour actually is.
pub fn saturation_value_to_position(
    saturation: f32,
    value: f32,
    size: Size<Pixels>,
) -> Point<Pixels> {
    Point {
        x: px(saturation.clamp(0., 1.) * f32::from(size.width)),
        y: px((1. - value.clamp(0., 1.)) * f32::from(size.height)),
    }
}

/// A hue in 0..=1 from a position along the bar.
pub fn position_to_hue(local_x: Pixels, width: Pixels) -> f32 {
    (f32::from(local_x) / f32::from(width).max(1.)).clamp(0., 1.)
}

/// The six stops a hue bar is built from.
///
/// Six pairs rather than one gradient: `linear_gradient` carries exactly two
/// stops, so the bar is six abutting gradients. Six quads, which is the whole
/// cost of drawing it.
pub fn hue_bar_stops() -> [(Hsla, Hsla); 6] {
    let at = |sixth: f32| hsla(sixth / 6., 1., 0.5, 1.);
    [
        (at(0.), at(1.)),
        (at(1.), at(2.)),
        (at(2.), at(3.)),
        (at(3.), at(4.)),
        (at(4.), at(5.)),
        (at(5.), at(6.)),
    ]
}

/// HSV, which is what the square and the bar describe, into the HSL gpui paints.
///
/// The two are not the same space, and treating them as the same is why pickers
/// come out with a top edge that is white instead of the pure hue.
pub fn hsv_to_hsla(hue: f32, saturation: f32, value: f32) -> Hsla {
    let lightness = value * (1. - saturation / 2.);
    let hsl_saturation = if lightness <= 0. || lightness >= 1. {
        0.
    } else {
        (value - lightness) / lightness.min(1. - lightness)
    };
    hsla(
        hue,
        hsl_saturation.clamp(0., 1.),
        lightness.clamp(0., 1.),
        1.,
    )
}

/// And back, so an existing colour lands the handle in the right place.
pub fn hsla_to_hsv(colour: Hsla) -> (f32, f32, f32) {
    let value = colour.l + colour.s * colour.l.min(1. - colour.l);
    let saturation = if value <= 0. {
        0.
    } else {
        2. * (1. - colour.l / value)
    };
    (colour.h, saturation.clamp(0., 1.), value.clamp(0., 1.))
}

/// What the picker is being dragged by, if anything.
#[derive(Copy, Clone, PartialEq)]
enum Dragging {
    Area,
    Hue,
}

pub struct ColorPicker {
    hue: f32,
    saturation: f32,
    value: f32,
    dragging: Option<Dragging>,
    area_bounds: Option<Bounds<Pixels>>,
    hue_bounds: Option<Bounds<Pixels>>,
    hex_error: bool,
}

/// Emitted whenever the colour moves, including during a drag, so a caller can
/// show a live preview on the thing being coloured rather than only in here.
pub struct ColorChanged(pub Hsla);

impl EventEmitter<ColorChanged> for ColorPicker {}
impl EventEmitter<DismissEvent> for ColorPicker {}

const AREA_HEIGHT: Pixels = px(140.);
const HUE_HEIGHT: Pixels = px(14.);
const HANDLE_SIZE: Pixels = px(12.);
/// One arrow press. Small enough to tune with, large enough to cross the area
/// without holding the key for a minute.
pub const COLOR_PICKER_KEY_STEP: f32 = 0.02;

impl ColorPicker {
    pub fn new(initial: Option<Hsla>) -> Self {
        let (hue, saturation, value) = initial.map(hsla_to_hsv).unwrap_or((0., 0., 1.));
        Self {
            hue,
            saturation,
            value,
            dragging: None,
            area_bounds: None,
            hue_bounds: None,
            hex_error: false,
        }
    }

    pub fn colour(&self) -> Hsla {
        hsv_to_hsla(self.hue, self.saturation, self.value)
    }

    pub fn set_colour(&mut self, colour: Hsla, cx: &mut Context<Self>) {
        let (hue, saturation, value) = hsla_to_hsv(colour);
        self.hue = hue;
        self.saturation = saturation;
        self.value = value;
        self.hex_error = false;
        cx.emit(ColorChanged(self.colour()));
        cx.notify();
    }

    /// Reads a hex string into the picker, marking the field rather than
    /// silently keeping the old colour.
    pub fn set_hex(&mut self, hex: &str, cx: &mut Context<Self>) {
        match theme::try_parse_color(hex) {
            Ok(colour) => self.set_colour(colour, cx),
            Err(_) => {
                self.hex_error = true;
                cx.notify();
            }
        }
    }

    pub fn hex_was_refused(&self) -> bool {
        self.hex_error
    }

    fn track_area(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.area_bounds else {
            return;
        };
        let local = position - bounds.origin;
        let (saturation, value) = position_to_saturation_value(local, bounds.size);
        self.saturation = saturation;
        self.value = value;
        cx.emit(ColorChanged(self.colour()));
        cx.notify();
    }

    fn track_hue(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.hue_bounds else {
            return;
        };
        self.hue = position_to_hue(position.x - bounds.origin.x, bounds.size.width);
        cx.emit(ColorChanged(self.colour()));
        cx.notify();
    }

    /// One arrow-key step across the area.
    ///
    /// Driven from outside because key events travel the focus path and this
    /// element is not focusable -- an `on_key_down` on the picker's own root
    /// would never fire. Whoever hosts the picker owns the focus and forwards.
    pub fn nudge(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        self.saturation = (self.saturation + dx).clamp(0., 1.);
        self.value = (self.value + dy).clamp(0., 1.);
        cx.emit(ColorChanged(self.colour()));
        cx.notify();
    }

    pub fn nudge_hue(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.hue = (self.hue + delta).rem_euclid(1.);
        cx.emit(ColorChanged(self.colour()));
        cx.notify();
    }
}

impl Render for ColorPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let pure_hue = hsla(self.hue, 1., 0.5, 1.);
        let current = self.colour();

        v_flex()
            .key_context("ColorPicker")
            .w_full()
            .gap_2()
            .child(
                // Two stacked gradients rather than a grid of swatches: white to
                // the pure hue across, transparent to black down. Two quads for
                // the whole square.
                div()
                    .id("colour-area")
                    .debug_selector(|| "colour-picker-area".into())
                    .relative()
                    .w_full()
                    .h(AREA_HEIGHT)
                    .rounded_sm()
                    .overflow_hidden()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(hsla(0., 0., 1., 1.), 0.),
                        linear_color_stop(pure_hue, 1.),
                    ))
                    .child(div().absolute().inset_0().bg(linear_gradient(
                        180.,
                        linear_color_stop(hsla(0., 0., 0., 0.), 0.),
                        linear_color_stop(hsla(0., 0., 0., 1.), 1.),
                    )))
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .size(HANDLE_SIZE)
                            .rounded_full()
                            .border_2()
                            .border_color(hsla(0., 0., 1., 1.))
                            .bg(current)
                            .map(|handle| {
                                let at = self
                                    .area_bounds
                                    .map(|bounds| {
                                        saturation_value_to_position(
                                            self.saturation,
                                            self.value,
                                            bounds.size,
                                        )
                                    })
                                    .unwrap_or_default();
                                handle
                                    .left(at.x - HANDLE_SIZE / 2.)
                                    .top(at.y - HANDLE_SIZE / 2.)
                            }),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                            this.dragging = Some(Dragging::Area);
                            this.track_area(event.position, cx);
                        }),
                    )
                    // The area's real bounds, read off the frame that drew it.
                    // A `canvas` child is the way to learn them: styles say what
                    // was asked for, layout says what happened, and turning a
                    // window position into a fraction needs the latter.
                    .child(
                        canvas(
                            {
                                let picker = cx.entity().downgrade();
                                move |bounds, _window, cx| {
                                    picker
                                        .update(cx, |picker, _| picker.area_bounds = Some(bounds))
                                        .ok();
                                }
                            },
                            {
                                let picker = cx.entity().downgrade();
                                move |_, _, window, cx| {
                                    track_drag(picker, Dragging::Area, window, cx);
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .child(
                h_flex()
                    .id("hue-bar")
                    .debug_selector(|| "colour-picker-hue".into())
                    .w_full()
                    .h(HUE_HEIGHT)
                    .rounded_sm()
                    .overflow_hidden()
                    .children(hue_bar_stops().into_iter().map(|(from, to)| {
                        div().flex_1().h_full().bg(linear_gradient(
                            90.,
                            linear_color_stop(from, 0.),
                            linear_color_stop(to, 1.),
                        ))
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                            this.dragging = Some(Dragging::Hue);
                            this.track_hue(event.position, cx);
                        }),
                    )
                    .child(
                        canvas(
                            {
                                let picker = cx.entity().downgrade();
                                move |bounds, _window, cx| {
                                    picker
                                        .update(cx, |picker, _| picker.hue_bounds = Some(bounds))
                                        .ok();
                                }
                            },
                            {
                                let picker = cx.entity().downgrade();
                                move |_, _, window, cx| {
                                    track_drag(picker, Dragging::Hue, window, cx);
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .justify_between()
                    .child(
                        div()
                            .size(px(20.))
                            .rounded_sm()
                            .border_1()
                            .border_color(colors.border)
                            .bg(current),
                    )
                    .child(
                        Label::new(workspace_hex(current))
                            .size(LabelSize::Small)
                            .color(if self.hex_error {
                                Color::Error
                            } else {
                                Color::Muted
                            }),
                    ),
            )
    }
}

/// Follows the pointer for the rest of a drag, wherever it goes.
///
/// Registered from `paint` rather than from the mouse-down handler, because
/// `Window::on_mouse_event` lives for the frame that registers it — the same
/// shape the scrollbar's thumb uses. And it has to be window-level: a `div`'s own
/// `on_mouse_move` is hitbox-filtered, so the colour would freeze the instant the
/// hand left the square, which is most of any real drag.
fn track_drag(
    picker: gpui::WeakEntity<ColorPicker>,
    which: Dragging,
    window: &mut Window,
    cx: &mut App,
) {
    let is_dragging = picker
        .read_with(cx, |picker, _| picker.dragging == Some(which))
        .unwrap_or(false);
    if !is_dragging {
        return;
    }

    let moving = picker.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
        if phase != gpui::DispatchPhase::Capture {
            return;
        }
        moving
            .update(cx, |picker, cx| match which {
                Dragging::Area => picker.track_area(event.position, cx),
                Dragging::Hue => picker.track_hue(event.position, cx),
            })
            .ok();
    });

    let releasing = picker;
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
        if phase != gpui::DispatchPhase::Capture {
            return;
        }
        releasing
            .update(cx, |picker, cx| {
                picker.dragging = None;
                cx.notify();
            })
            .ok();
    });
}

/// The hex a person reads, from the same helper the record uses, so the string
/// shown and the string stored can never disagree.
fn workspace_hex(colour: Hsla) -> String {
    let rgba = gpui::Rgba::from(colour);
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.r * 255.).round().clamp(0., 255.) as u8,
        (rgba.g * 255.).round().clamp(0., 255.) as u8,
        (rgba.b * 255.).round().clamp(0., 255.) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_in_the_area_reads_as_saturation_and_value() {
        let size = Size {
            width: px(100.),
            height: px(100.),
        };
        // Top-left is white: no saturation, full value.
        assert_eq!(
            position_to_saturation_value(
                Point {
                    x: px(0.),
                    y: px(0.)
                },
                size
            ),
            (0., 1.)
        );
        // Top-right is the pure hue.
        assert_eq!(
            position_to_saturation_value(
                Point {
                    x: px(100.),
                    y: px(0.)
                },
                size
            ),
            (1., 1.)
        );
        // The bottom edge is black whatever the saturation.
        assert_eq!(
            position_to_saturation_value(
                Point {
                    x: px(50.),
                    y: px(100.)
                },
                size
            ),
            (0.5, 0.)
        );
    }

    /// A pointer held down and dragged outside the square keeps painting from
    /// the nearest edge. Refusing the input instead would make the colour stick
    /// the moment the hand overshoots.
    #[test]
    fn a_point_outside_the_area_is_clamped_not_refused() {
        let size = Size {
            width: px(100.),
            height: px(100.),
        };
        assert_eq!(
            position_to_saturation_value(
                Point {
                    x: px(-40.),
                    y: px(-40.)
                },
                size
            ),
            (0., 1.)
        );
        assert_eq!(
            position_to_saturation_value(
                Point {
                    x: px(400.),
                    y: px(400.)
                },
                size
            ),
            (1., 0.)
        );
    }

    #[test]
    fn a_zero_sized_area_does_not_divide_by_zero() {
        let size = Size {
            width: px(0.),
            height: px(0.),
        };
        let (saturation, value) = position_to_saturation_value(
            Point {
                x: px(5.),
                y: px(5.),
            },
            size,
        );
        assert!(saturation.is_finite() && value.is_finite());
    }

    #[test]
    fn the_handle_lands_back_where_the_colour_came_from() {
        let size = Size {
            width: px(120.),
            height: px(80.),
        };
        for (saturation, value) in [(0., 1.), (1., 1.), (0.25, 0.75), (1., 0.)] {
            let at = saturation_value_to_position(saturation, value, size);
            let (round_saturation, round_value) = position_to_saturation_value(at, size);
            assert!(
                (round_saturation - saturation).abs() < 0.001
                    && (round_value - value).abs() < 0.001,
                "({saturation}, {value}) came back as ({round_saturation}, {round_value})"
            );
        }
    }

    /// HSV and HSL are different spaces, and the square describes HSV. Reading
    /// the square's numbers straight into `hsla` puts white along the top edge
    /// where the pure hue belongs.
    #[test]
    fn full_saturation_and_value_is_the_pure_hue() {
        let red = hsv_to_hsla(0., 1., 1.);
        assert!(
            (red.l - 0.5).abs() < 0.001 && (red.s - 1.).abs() < 0.001,
            "a fully saturated, fully bright red is hsl(0, 100%, 50%), got {red:?}"
        );
        assert_eq!(hsv_to_hsla(0.3, 1., 0.).l, 0., "no value is black");
        assert_eq!(hsv_to_hsla(0.3, 0., 1.).s, 0., "no saturation is grey");
    }

    #[test]
    fn a_colour_survives_the_trip_through_hsv() {
        for colour in [
            hsla(0., 1., 0.5, 1.),
            hsla(0.6, 0.9, 0.6, 1.),
            hsla(0., 0., 1., 1.),
            hsla(0., 0., 0., 1.),
        ] {
            let (hue, saturation, value) = hsla_to_hsv(colour);
            let back = hsv_to_hsla(hue, saturation, value);
            assert!(
                (back.s - colour.s).abs() < 0.01 && (back.l - colour.l).abs() < 0.01,
                "{colour:?} came back as {back:?}"
            );
        }
    }

    #[test]
    fn the_hue_bar_covers_the_circle_once() {
        let stops = hue_bar_stops();
        assert_eq!(stops.len(), 6);
        assert!((stops[0].0.h - 0.).abs() < 0.001, "it starts at red");
        assert!(
            (stops[5].1.h - 1.).abs() < 0.001,
            "and ends back at red, got {:?}",
            stops[5].1.h
        );
        // Each stop's end is the next stop's start, or the bar has seams.
        for pair in stops.windows(2) {
            assert!(
                (pair[0].1.h - pair[1].0.h).abs() < 0.001,
                "a seam between {:?} and {:?}",
                pair[0].1.h,
                pair[1].0.h
            );
        }
    }
}
