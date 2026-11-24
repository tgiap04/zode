//! Account pictures, the only remote images the GitHub tab loads.
//!
//! A URL is loaded only when it is `https` on GitHub's avatar host; anything
//! else, or a picture that fails to load, shows the account's initials.

use gpui::{AnyElement, App, Pixels, SharedUri, img, px};
use ui::prelude::*;
use url::Url;

pub const AVATAR_HOST: &str = "avatars.githubusercontent.com";

/// `url` when it may be loaded: `https`, GitHub's avatar host exactly, no
/// credentials and no other port.
pub fn trusted_avatar_url(url: &str) -> Option<SharedUri> {
    let parsed = Url::parse(url).ok()?;
    let trusted = parsed.scheme() == "https"
        && parsed.host_str() == Some(AVATAR_HOST)
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none();
    trusted.then(|| SharedUri::from(parsed.to_string()))
}

/// Up to two letters standing for an account: the first letters of the
/// profile name's first two words, or of the login.
pub fn initials(login: &str, name: Option<&str>) -> String {
    let first_letter = |word: &str| {
        word.chars()
            .find(|character| character.is_alphanumeric())
            .map(|character| character.to_uppercase().collect::<String>())
    };
    let from_name = name
        .into_iter()
        .flat_map(str::split_whitespace)
        .filter_map(first_letter)
        .take(2)
        .collect::<String>();
    if !from_name.is_empty() {
        return from_name;
    }
    first_letter(login).unwrap_or_else(|| "?".to_string())
}

fn initials_circle(text: String, size: Pixels, cx: &App) -> AnyElement {
    h_flex()
        .size(size)
        .flex_none()
        .justify_center()
        .rounded_full()
        .bg(cx.theme().colors().element_background)
        .border_1()
        .border_color(cx.theme().colors().border_variant)
        .child(
            Label::new(text)
                .size(if size >= px(20.) {
                    LabelSize::XSmall
                } else {
                    LabelSize::Custom(rems_from_px(8.))
                })
                .color(Color::Muted),
        )
        .into_any_element()
}

/// A round picture of `size`, or the account's initials.
pub(crate) fn render_avatar(
    login: &str,
    name: Option<&str>,
    avatar_url: Option<&str>,
    size: Pixels,
    cx: &App,
) -> AnyElement {
    let text = initials(login, name);
    match avatar_url.and_then(trusted_avatar_url) {
        Some(url) => {
            let fallback = text;
            let border = cx.theme().colors().border_variant;
            let background = cx.theme().colors().element_background;
            div()
                .size(size)
                .flex_none()
                .rounded_full()
                .bg(background)
                .border_1()
                .border_color(border)
                .overflow_hidden()
                .child(img(url).size_full().rounded_full().with_fallback(move || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Label::new(fallback.clone())
                                .size(LabelSize::Custom(rems_from_px(8.)))
                                .color(Color::Muted),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        }
        None => initials_circle(text, size, cx),
    }
}

/// The logins drawn beside a facepile: one or two by name, then the first
/// and how many more.
pub(crate) fn people_label(logins: &[String]) -> String {
    match logins {
        [] => String::new(),
        [one] => one.clone(),
        [one, two] => format!("{one}, {two}"),
        [first, rest @ ..] => format!("{first} +{}", rest.len()),
    }
}

/// Faces and logins of the people assigned, every login on hover. `people`
/// are `(login, avatar URL)` with the logins already made safe to draw.
pub(crate) fn render_people(
    id: impl Into<ElementId>,
    people: &[(String, Option<String>)],
    max_width: Pixels,
    cx: &App,
) -> Option<AnyElement> {
    if people.is_empty() {
        return None;
    }
    let logins = people
        .iter()
        .map(|(login, _)| login.clone())
        .collect::<Vec<_>>();
    let names = logins
        .iter()
        .map(|login| format!("@{login}"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(
        h_flex()
            .id(id.into())
            .flex_none()
            .min_w_0()
            .max_w(max_width)
            .gap_1()
            .tooltip(ui::Tooltip::text(format!("Assigned to {names}")))
            .child(render_facepile(people, 3, px(16.), cx))
            .child(
                Label::new(people_label(&logins))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .single_line()
                    .truncate(),
            )
            .into_any_element(),
    )
}

/// Up to `shown` overlapping pictures, then `+N` for the rest.
pub(crate) fn render_facepile(
    people: &[(String, Option<String>)],
    shown: usize,
    size: Pixels,
    cx: &App,
) -> AnyElement {
    let hidden = people.len().saturating_sub(shown);
    let surface = cx.theme().colors().editor_background;
    h_flex()
        .flex_none()
        .children(
            people
                .iter()
                .take(shown)
                .enumerate()
                .map(|(index, (login, url))| {
                    div()
                        .when(index > 0, |face| face.ml(px(-4.)))
                        .rounded_full()
                        .border_1()
                        .border_color(surface)
                        .child(render_avatar(login, None, url.as_deref(), size, cx))
                }),
        )
        .when(hidden > 0, |pile| {
            pile.child(
                div().pl_1().child(
                    Label::new(format!("+{hidden}"))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                ),
            )
        })
        .into_any_element()
}
