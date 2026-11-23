//! The "Details" link of a check. CI services and apps pick it, so it is not
//! opened unasked unless it is GitHub's own page, and never over plain http.

use gpui::{App, PromptLevel, Window};
use url::Url;

use crate::pull_requests::pr_detail_view::PullRequestDetailView;
use crate::untrusted_text::reveal_hidden_characters;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckLink {
    /// github.com or one of its subdomains, over https.
    Open(Url),
    /// Another host over https: the user is shown where it leads first.
    Confirm { host: String, url: Url },
    /// Not https, or not an address at all.
    Refused,
}

pub fn check_link(address: &str) -> CheckLink {
    let Ok(url) = Url::parse(address.trim()) else {
        return CheckLink::Refused;
    };
    let Some(host) = url.host_str().map(str::to_string) else {
        return CheckLink::Refused;
    };
    if url.scheme() != "https" {
        return CheckLink::Refused;
    }
    let plain = url.port().is_none() && url.username().is_empty() && url.password().is_none();
    let github = host == "github.com" || host.ends_with(".github.com");
    if github && plain {
        CheckLink::Open(url)
    } else {
        CheckLink::Confirm { host, url }
    }
}

/// The host a check's link leads to, for the button's tooltip.
pub fn check_link_host(address: &str) -> String {
    Url::parse(address.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .map_or_else(
            || "an unreadable address".to_string(),
            |host| reveal_hidden_characters(&host).into_owned(),
        )
}

impl PullRequestDetailView {
    pub fn open_check_details(&self, address: &str, window: &mut Window, cx: &mut App) {
        match check_link(address) {
            CheckLink::Open(url) => cx.open_url(url.as_str()),
            CheckLink::Confirm { host, url } => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    &format!("This check's link leads to {host}"),
                    Some(&format!(
                        "A CI service chose it, not GitHub. It opens {url}. Open it anyway?"
                    )),
                    &["Open", "Cancel"],
                    cx,
                );
                cx.spawn(async move |cx| {
                    if answer.await == Ok(0) {
                        cx.update(|cx| cx.open_url(url.as_str()));
                    }
                })
                .detach();
            }
            CheckLink::Refused => {
                let shown = reveal_hidden_characters(address.trim()).into_owned();
                // The answer is not needed: the prompt only informs.
                drop(window.prompt(
                    PromptLevel::Warning,
                    "This check's link was not opened",
                    Some(&format!(
                        "Only https addresses are opened. It reads {shown}."
                    )),
                    &["OK"],
                    cx,
                ));
            }
        }
    }
}
