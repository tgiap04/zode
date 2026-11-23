//! Markdown that strangers wrote — issue and pull request bodies, comments —
//! drawn without HTML, without fetching or decoding any image, with only
//! http(s) links, and with `#123` references made into links.
//!
//! A link opens inside Zode only when its address is exactly a github.com
//! issue or pull request. A link whose text shows one address while it leads
//! to another names the real host and asks before opening.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use github::{GithubBackend, RepositoryId};
use gpui::{App, AppContext as _, Context, Entity, IntoElement, PromptLevel, Render, Window};
use markdown::parser::PARSE_OPTIONS;
use markdown::{
    DataImages, Markdown, MarkdownElement, MarkdownFont, MarkdownOptions, MarkdownStyle,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use url::Url;
use workspace::Workspace;

use crate::github_view::DetailKey;
use crate::github_view_session::{SessionStamp, is_live_session};
use crate::issues::open_issue_detail;
use crate::pull_requests::open_pull_request_detail;
use crate::untrusted_text::reveal_hidden_characters;

/// No HTML, no diagrams, no `data:` images: a body is drawn from its text
/// alone. With no image resolver either, every image shows its alt text.
pub fn github_markdown_options() -> MarkdownOptions {
    MarkdownOptions {
        parse_links_only: false,
        parse_html: false,
        render_mermaid_diagrams: false,
        parse_heading_slugs: false,
        data_images: DataImages::Off,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Issue(DetailKey),
    PullRequest(DetailKey),
    Web(Url),
    /// Not http(s): `javascript:`, `file:`, `data:`, relative paths…
    Refused,
}

/// Where a link may lead. Decided on the parsed address, never on a prefix
/// of the text, so `github.com.evil.com` or `evil.com/github.com/…` stay on
/// the web.
pub fn classify_link(destination: &str) -> LinkTarget {
    let Ok(url) = Url::parse(destination.trim()) else {
        return LinkTarget::Refused;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return LinkTarget::Refused;
    }
    let plain_github = url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if plain_github && let Some(target) = github_item(&url) {
        return target;
    }
    LinkTarget::Web(url)
}

/// `/{owner}/{repo}/(issues|pull)/{number}` and nothing more.
fn github_item(url: &Url) -> Option<LinkTarget> {
    let segments = url.path_segments()?.collect::<Vec<_>>();
    let [owner, name, kind, number] = segments.as_slice() else {
        return None;
    };
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let key = DetailKey {
        repository: RepositoryId::new(owner, name)?,
        number: number.parse().ok().filter(|number| *number > 0)?,
    };
    match *kind {
        "issues" => Some(LinkTarget::Issue(key)),
        "pull" => Some(LinkTarget::PullRequest(key)),
        _ => None,
    }
}

/// The host `target` really leads to, when `shown` (a link's text) reads as
/// an address on another host.
pub fn shown_host_mismatch(shown: &str, target: &Url) -> Option<String> {
    let shown = shown.trim();
    if shown.is_empty() || shown.contains(char::is_whitespace) {
        return None;
    }
    let candidate = if shown.contains("://") {
        shown.to_string()
    } else {
        format!("https://{shown}")
    };
    let shown_url = Url::parse(&candidate).ok()?;
    let shown_host = shown_url.host_str()?;
    if !shown_host.contains('.') {
        return None;
    }
    let target_host = target.host_str()?;
    (shown_host != target_host).then(|| target_host.to_string())
}

/// Every link's text, by the address it leads to.
pub fn link_texts(source: &str) -> HashMap<String, Vec<String>> {
    let mut texts: HashMap<String, Vec<String>> = HashMap::default();
    let mut open: Option<(String, String)> = None;
    for event in Parser::new_ext(source, PARSE_OPTIONS) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                open = Some((dest_url.into_string(), String::new()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, shown)) = open.as_mut() {
                    shown.push_str(&text);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((destination, shown)) = open.take() {
                    texts.entry(destination).or_default().push(shown);
                }
            }
            _ => {}
        }
    }
    texts
}

/// `#123` and `owner/repo#123` made into links to those issues, except in
/// code, links, images and HTML, where they are left as written.
pub fn link_issue_references(source: &str, repository: &RepositoryId) -> String {
    let mut skipped_depth = 0usize;
    let mut text_ranges: Vec<Range<usize>> = Vec::new();
    for (event, range) in Parser::new_ext(source, PARSE_OPTIONS).into_offset_iter() {
        match event {
            Event::Start(
                Tag::CodeBlock(_)
                | Tag::Link { .. }
                | Tag::Image { .. }
                | Tag::HtmlBlock
                | Tag::MetadataBlock(_),
            ) => skipped_depth += 1,
            Event::End(
                TagEnd::CodeBlock
                | TagEnd::Link
                | TagEnd::Image
                | TagEnd::HtmlBlock
                | TagEnd::MetadataBlock(_),
            ) => skipped_depth = skipped_depth.saturating_sub(1),
            Event::Text(_) if skipped_depth == 0 => text_ranges.push(range),
            _ => {}
        }
    }
    let mut linked = String::with_capacity(source.len());
    let mut copied_up_to = 0;
    for range in text_ranges {
        let Some(text) = source.get(range.clone()) else {
            continue;
        };
        for reference in issue_references(text) {
            let start = range.start + reference.range.start;
            let end = range.start + reference.range.end;
            if start < copied_up_to {
                continue;
            }
            // What precedes the text matters too: `\#5` is an escaped `#`,
            // which the parser hands over without its backslash.
            let before = start
                .checked_sub(1)
                .and_then(|index| source.as_bytes().get(index));
            if before.is_some_and(|byte| is_blocking_byte(*byte)) {
                continue;
            }
            let target = reference.repository.as_ref().unwrap_or(repository);
            linked.push_str(&source[copied_up_to..start]);
            linked.push_str(&format!(
                "[{}](https://github.com/{target}/issues/{})",
                &source[start..end],
                reference.number
            ));
            copied_up_to = end;
        }
    }
    linked.push_str(&source[copied_up_to..]);
    linked
}

struct IssueReference {
    range: Range<usize>,
    repository: Option<RepositoryId>,
    number: u64,
}

/// Longer is not an issue number GitHub hands out, and keeps the parse safe.
const MAX_REFERENCE_DIGITS: usize = 10;

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// A reference right after one of these is part of something else: a word,
/// an HTML entity (`&#39;`), an escape, a path, a query or fragment
/// (`?#5`, `a=#5`), a scheme or label (`see:#5`), or another reference.
fn is_blocking_byte(byte: u8) -> bool {
    is_word_byte(byte) || b"&\\/#.-=?:".contains(&byte)
}

fn issue_references(text: &str) -> Vec<IssueReference> {
    let bytes = text.as_bytes();
    let mut references = Vec::new();
    for (hash, _) in text.match_indices('#') {
        let digits_end = bytes[hash + 1..]
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .map_or(bytes.len(), |offset| hash + 1 + offset);
        let digit_count = digits_end - hash - 1;
        if digit_count == 0 || digit_count > MAX_REFERENCE_DIGITS {
            continue;
        }
        if bytes
            .get(digits_end)
            .is_some_and(|byte| is_word_byte(*byte))
        {
            continue;
        }
        let Ok(number) = text[hash + 1..digits_end].parse::<u64>() else {
            continue;
        };
        let (start, repository) = match repository_before(text, hash) {
            Some((start, repository)) => (start, Some(repository)),
            None => (hash, None),
        };
        let before = start.checked_sub(1).and_then(|index| bytes.get(index));
        if before.is_some_and(|byte| is_blocking_byte(*byte)) {
            continue;
        }
        references.push(IssueReference {
            range: start..digits_end,
            repository,
            number,
        });
    }
    references
}

/// The `owner/repo` written right before the `#` at `hash`, if any.
fn repository_before(text: &str, hash: usize) -> Option<(usize, RepositoryId)> {
    let bytes = text.as_bytes();
    let is_name_byte = |byte: &u8| byte.is_ascii_alphanumeric() || b"._-".contains(byte);
    let name_start = bytes[..hash]
        .iter()
        .rposition(|byte| !is_name_byte(byte))
        .map_or(0, |index| index + 1);
    let slash = name_start.checked_sub(1)?;
    if bytes.get(slash) != Some(&b'/') {
        return None;
    }
    let owner_start = bytes[..slash]
        .iter()
        .rposition(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'-'))
        .map_or(0, |index| index + 1);
    let repository = RepositoryId::new(&text[owner_start..slash], &text[name_start..hash])?;
    Some((owner_start, repository))
}

/// What a click on a link does; cloned into the element's click handler.
#[derive(Clone)]
struct LinkOpener {
    link_texts: Arc<HashMap<String, Vec<String>>>,
    backend: Arc<dyn GithubBackend>,
    /// The sign-in `backend` belongs to. Once it is over, an issue or pull
    /// request link opens on github.com rather than in a tab that would
    /// hold the old backend.
    session: SessionStamp,
}

impl LinkOpener {
    fn open(&self, destination: &str, window: &mut Window, cx: &mut App) {
        let in_app = is_live_session(&self.session, cx);
        match classify_link(destination) {
            LinkTarget::Refused => {
                log::warn!("not opening a GitHub link that is not http(s)");
            }
            LinkTarget::Issue(key) => {
                let workspace = Workspace::for_window(window, cx).filter(|_| in_app);
                let Some(workspace) = workspace else {
                    cx.open_url(&issue_page(&key));
                    return;
                };
                let backend = self.backend.clone();
                let session = self.session.clone();
                // The click arrives while the markdown is being updated; the
                // tab is opened once that is over.
                window.defer(cx, move |window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        open_issue_detail(workspace, backend, session, key, window, cx);
                    });
                });
            }
            LinkTarget::PullRequest(key) => {
                let workspace = Workspace::for_window(window, cx).filter(|_| in_app);
                let Some(workspace) = workspace else {
                    cx.open_url(&format!(
                        "https://github.com/{}/pull/{}",
                        key.repository, key.number
                    ));
                    return;
                };
                let backend = self.backend.clone();
                let session = self.session.clone();
                window.defer(cx, move |window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        open_pull_request_detail(workspace, backend, session, key, window, cx);
                    });
                });
            }
            LinkTarget::Web(url) => self.open_on_the_web(destination, url, window, cx),
        }
    }

    fn open_on_the_web(&self, destination: &str, url: Url, window: &mut Window, cx: &mut App) {
        let shown = self
            .link_texts
            .get(destination)
            .into_iter()
            .flatten()
            .find_map(|shown| shown_host_mismatch(shown, &url).map(|host| (shown.clone(), host)));
        let Some((shown, host)) = shown else {
            cx.open_url(url.as_str());
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("This link leads to {host}"),
            Some(&host_mismatch_prompt(&shown, &url)),
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
}

/// The question asked before opening a link whose text names another host.
pub(crate) fn host_mismatch_prompt(shown: &str, url: &Url) -> String {
    format!(
        "Its text reads “{}”, but it opens {url}. Open it anyway?",
        reveal_hidden_characters(shown)
    )
}

pub(crate) fn issue_page(key: &DetailKey) -> String {
    format!(
        "https://github.com/{}/issues/{}",
        key.repository, key.number
    )
}

/// One body or comment, drawn safely. `#N` references point into
/// `repository`.
pub struct GithubMarkdown {
    markdown: Entity<Markdown>,
    links: LinkOpener,
}

impl GithubMarkdown {
    pub fn new(
        source: &str,
        repository: &RepositoryId,
        backend: Arc<dyn GithubBackend>,
        session: SessionStamp,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = link_issue_references(source, repository);
        let link_texts = Arc::new(link_texts(&source));
        let markdown = cx.new(|cx| {
            Markdown::new_with_options(source.into(), None, None, github_markdown_options(), cx)
        });
        Self {
            markdown,
            links: LinkOpener {
                link_texts,
                backend,
                session,
            },
        }
    }

    pub fn source<'a>(&self, cx: &'a App) -> &'a str {
        self.markdown.read(cx).source()
    }

    /// What a click on a link to `destination` does.
    pub fn open_link(&self, destination: &str, window: &mut Window, cx: &mut App) {
        self.links.open(destination, window, cx);
    }
}

impl Render for GithubMarkdown {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let links = self.links.clone();
        MarkdownElement::new(
            self.markdown.clone(),
            MarkdownStyle::themed(MarkdownFont::Editor, window, cx),
        )
        .on_url_click(move |destination, window, cx| links.open(&destination, window, cx))
    }
}
