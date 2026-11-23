use std::sync::Arc;

use github::{FakeGithubBackend, RepositoryId};
use gpui::{AppContext as _, TestAppContext};
use markdown::DataImages;
use url::Url;

use crate::github_markdown::{
    GithubMarkdown, LinkTarget, classify_link, github_markdown_options, link_issue_references,
    link_texts, shown_host_mismatch,
};
use crate::github_view::DetailKey;
use crate::github_view_session::test_session;
use crate::issues::issues_test_support::{detail_tabs, init, repository, workspace_window};

fn key(owner: &str, name: &str, number: u64) -> DetailKey {
    DetailKey {
        repository: RepositoryId::new(owner, name).expect("valid"),
        number,
    }
}

#[test]
fn references_outside_code_become_links() {
    assert_eq!(
        link_issue_references("Fixes #12 and octo/other#3.", &repository()),
        "Fixes [#12](https://github.com/octo/repo/issues/12) and \
         [octo/other#3](https://github.com/octo/other/issues/3)."
    );
    assert_eq!(
        link_issue_references("(#7)\n\n- #8", &repository()),
        "([#7](https://github.com/octo/repo/issues/7))\n\n- [#8](https://github.com/octo/repo/issues/8)"
    );
}

#[test]
fn references_in_code_links_and_words_stay_as_written() {
    let untouched = [
        "```\nlet x = #123;\n```",
        "    indented #123 code",
        "inline `#123` code",
        "[see #9](https://example.com)",
        "abc#3 #12abc C#4 &#39; \\#5",
        "# Heading",
        "<div>#77</div>",
    ];
    for source in untouched {
        assert_eq!(
            link_issue_references(source, &repository()),
            source,
            "{source:?} is left as written"
        );
    }
}

#[test]
fn only_http_links_are_followed() {
    for refused in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,<script>alert(1)</script>",
        "vbscript:msgbox",
        "mailto:someone@example.com",
        "/relative/path",
        "zode://open",
    ] {
        assert_eq!(classify_link(refused), LinkTarget::Refused, "{refused}");
    }
    assert!(matches!(
        classify_link("https://docs.rs/gpui"),
        LinkTarget::Web(_)
    ));
}

#[test]
fn only_an_exact_github_issue_address_opens_in_zode() {
    assert_eq!(
        classify_link("https://github.com/octo/repo/issues/12"),
        LinkTarget::Issue(key("octo", "repo", 12))
    );
    assert_eq!(
        classify_link("https://GitHub.com/octo/repo/issues/12#issuecomment-1"),
        LinkTarget::Issue(key("octo", "repo", 12))
    );
    assert_eq!(
        classify_link("https://github.com/octo/repo/pull/3"),
        LinkTarget::PullRequest(key("octo", "repo", 3))
    );
    for lookalike in [
        "https://github.com.evil.com/octo/repo/issues/1",
        "https://evil.com/github.com/octo/repo/issues/1",
        "https://github.com@evil.com/octo/repo/issues/1",
        "https://user@github.com/octo/repo/issues/1",
        "https://github.com:8443/octo/repo/issues/1",
        "https://github.com/octo/repo/issues/1/extra",
        "https://github.com/octo/repo/issues/+1",
        "https://github.com/octo/repo/issues/0",
        "https://github.com/octo/repo/discussions/1",
        "https://gist.github.com/octo/repo/issues/1",
    ] {
        assert!(
            matches!(classify_link(lookalike), LinkTarget::Web(_)),
            "{lookalike} stays on the web"
        );
    }
}

#[test]
fn link_text_showing_another_host_is_caught() {
    let evil = Url::parse("https://evil.example/login").expect("valid");
    assert_eq!(
        shown_host_mismatch("https://github.com/octo/repo", &evil).as_deref(),
        Some("evil.example")
    );
    assert_eq!(
        shown_host_mismatch("github.com/octo/repo", &evil).as_deref(),
        Some("evil.example")
    );
    assert_eq!(shown_host_mismatch("click here", &evil), None);
    assert_eq!(shown_host_mismatch("#12", &evil), None);
    assert_eq!(shown_host_mismatch("evil.example/login", &evil), None);
    assert_eq!(
        link_texts("[https://github.com/a/b](https://evil.example/x) and [`code`](https://c.d)")
            .get("https://evil.example/x"),
        Some(&vec!["https://github.com/a/b".to_string()])
    );
}

#[test]
fn link_text_with_spaces_still_names_the_host() {
    let evil = Url::parse("https://evil.example/login").expect("valid");
    for shown in [
        "https://github.com/login/device verify your account",
        "verify your account at github.com/login/device",
        "github.com/login/device\u{a0}verify",
        "github.com/login/device\u{3000}verify",
        "see (https://github.com/octo/repo), then sign in",
        "visit \"github.com\".",
        "go to\u{200B} https://github.com/octo/repo now",
        "https://github.com/a and https://evil.example/b",
        "https://evil.example/b and https://github.com/a",
        "8.8.8.8 verify",
        "git\u{200B}hub.com is down",
        "github.com.evil.com",
        "www.paypal.com",
        "www.example.rs",
        "main.rs/login",
        "files.sh:8080",
    ] {
        assert!(
            shown_host_mismatch(shown, &evil).is_some(),
            "{shown:?} shows another host"
        );
    }
}

#[test]
fn spaced_link_text_is_left_alone_unless_it_shows_another_host() {
    let evil = Url::parse("https://evil.example/login").expect("valid");
    for shown in [
        "click here",
        "See #12 and octo/other#3",
        "read the docs, e.g. now.",
        "version 1.2 notes",
        "Download for Windows (x64)",
        "visit evil.example/login for details",
        "https://evil.example/a and https://evil.example/b",
        "evil.example\u{a0}login",
        "main.rs",
        "Node.js",
        "README.md",
        "open main.rs, then Cargo.toml and package.json (Node.js)",
        "see Cargo.lock.",
        "",
        "   ",
    ] {
        assert_eq!(
            shown_host_mismatch(shown, &evil),
            None,
            "{shown:?} says nothing false"
        );
    }
}

#[test]
fn a_line_break_in_link_text_separates_words() {
    assert_eq!(
        link_texts("[go to\ngithub.com](https://x.example/)").get("https://x.example/"),
        Some(&vec!["go to github.com".to_string()])
    );
}

#[test]
fn github_markdown_draws_no_html_and_decodes_no_images() {
    let options = github_markdown_options();
    assert!(!options.parse_html);
    assert!(!options.render_mermaid_diagrams);
    assert_eq!(options.data_images, DataImages::Off);
}

#[gpui::test]
async fn script_and_file_links_are_never_opened(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let body = cx.new(|cx| {
        GithubMarkdown::new(
            "[x](javascript:alert(1)) [y](file:///etc/passwd)",
            &repository(),
            Arc::new(FakeGithubBackend::new()),
            test_session(),
            cx,
        )
    });
    for link in ["javascript:alert(1)", "file:///etc/passwd"] {
        body.update_in(cx, |body, window, cx| body.open_link(link, window, cx));
        cx.run_until_parked();
    }
    assert_eq!(cx.opened_url(), None);
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
async fn a_link_whose_text_shows_another_host_asks_first(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let body = cx.new(|cx| {
        GithubMarkdown::new(
            "[https://github.com/octo/repo](https://evil.example/x) [docs](https://docs.rs/gpui)",
            &repository(),
            Arc::new(FakeGithubBackend::new()),
            test_session(),
            cx,
        )
    });
    body.update_in(cx, |body, window, cx| {
        body.open_link("https://evil.example/x", window, cx)
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asked before opening");
    let (message, detail) = cx.pending_prompt().unwrap_or_default();
    assert!(message.contains("evil.example"), "{message}");
    assert!(detail.contains("https://github.com/octo/repo"), "{detail}");
    assert_eq!(cx.opened_url(), None, "nothing opens before the answer");
    cx.simulate_prompt_answer("Open");
    cx.run_until_parked();
    assert_eq!(cx.opened_url().as_deref(), Some("https://evil.example/x"));

    body.update_in(cx, |body, window, cx| {
        body.open_link("https://docs.rs/gpui", window, cx)
    });
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "honest link text asks nothing");
    assert_eq!(cx.opened_url().as_deref(), Some("https://docs.rs/gpui"));
}

#[gpui::test]
async fn a_link_with_spaced_text_showing_another_host_asks_first(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let body = cx.new(|cx| {
        GithubMarkdown::new(
            "[https://github.com/login/device verify your account](https://evil.example)",
            &repository(),
            Arc::new(FakeGithubBackend::new()),
            test_session(),
            cx,
        )
    });
    body.update_in(cx, |body, window, cx| {
        body.open_link("https://evil.example", window, cx)
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asked before opening");
    assert_eq!(cx.opened_url(), None);
}

#[gpui::test]
async fn an_issue_link_whose_text_shows_another_host_asks_first(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let body = cx.new(|cx| {
        GithubMarkdown::new(
            "[docs.rs/gpui](https://github.com/octo/repo/issues/4) \
             [Fix on https://crates.io now](https://github.com/octo/repo/pull/5)",
            &repository(),
            Arc::new(FakeGithubBackend::new()),
            test_session(),
            cx,
        )
    });
    for link in [
        "https://github.com/octo/repo/issues/4",
        "https://github.com/octo/repo/pull/5",
    ] {
        body.update_in(cx, |body, window, cx| body.open_link(link, window, cx));
        cx.run_until_parked();
        assert!(cx.has_pending_prompt(), "{link} asks before opening");
        let (message, _) = cx.pending_prompt().unwrap_or_default();
        assert!(message.contains("github.com"), "{message}");
        assert!(
            detail_tabs(&workspace, cx).is_empty(),
            "no tab before the answer"
        );
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(detail_tabs(&workspace, cx).is_empty());
        assert_eq!(cx.opened_url(), None);
    }
}

#[gpui::test]
async fn a_refused_link_says_why(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let body = cx.new(|cx| {
        GithubMarkdown::new(
            "[x](javascript:alert(1))",
            &repository(),
            Arc::new(FakeGithubBackend::new()),
            test_session(),
            cx,
        )
    });
    assert!(workspace.read_with(cx, |workspace, _| workspace.notification_ids().is_empty()));
    body.update_in(cx, |body, window, cx| {
        body.open_link("javascript:alert(1)", window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |workspace, _| workspace.notification_ids().len()),
        1,
        "the click is answered with a notice"
    );
    assert_eq!(cx.opened_url(), None);
}

#[gpui::test]
async fn a_plain_http_link_asks_before_opening(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let body = cx.new(|cx| {
        GithubMarkdown::new(
            "[docs](http://docs.example/page)",
            &repository(),
            Arc::new(FakeGithubBackend::new()),
            test_session(),
            cx,
        )
    });
    body.update_in(cx, |body, window, cx| {
        body.open_link("http://docs.example/page", window, cx)
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asked before opening");
    let (_, detail) = cx.pending_prompt().unwrap_or_default();
    assert!(detail.contains("http://docs.example/page"), "{detail}");
    assert_eq!(cx.opened_url(), None);
    cx.simulate_prompt_answer("Open");
    cx.run_until_parked();
    assert_eq!(cx.opened_url().as_deref(), Some("http://docs.example/page"));
}

#[gpui::test]
async fn an_issue_link_opens_its_tab_and_a_lookalike_host_does_not(cx: &mut TestAppContext) {
    init(cx);
    let (workspace, cx) = workspace_window(cx).await;
    let body = cx.new(|cx| {
        GithubMarkdown::new(
            "See #4",
            &repository(),
            Arc::new(FakeGithubBackend::new()),
            test_session(),
            cx,
        )
    });
    let source = body.read_with(cx, |body, cx| body.source(cx).to_string());
    assert_eq!(source, "See [#4](https://github.com/octo/repo/issues/4)");

    body.update_in(cx, |body, window, cx| {
        body.open_link("https://github.com/octo/repo/issues/4", window, cx)
    });
    cx.run_until_parked();
    let tabs = detail_tabs(&workspace, cx);
    assert_eq!(tabs.len(), 1);
    assert_eq!(
        tabs[0].read_with(cx, |tab, _| tab.key().clone()),
        key("octo", "repo", 4)
    );
    assert_eq!(cx.opened_url(), None, "opened inside Zode");

    body.update_in(cx, |body, window, cx| {
        body.open_link("https://github.com.evil.com/octo/repo/issues/1", window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        detail_tabs(&workspace, cx).len(),
        1,
        "no tab for a lookalike"
    );
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://github.com.evil.com/octo/repo/issues/1")
    );
}

#[test]
fn references_after_an_equals_sign_question_mark_or_colon_stay_as_written() {
    let repository = repository();
    for text in ["a=#5", "page?#5", "see:#5", "x?id=#7"] {
        assert_eq!(link_issue_references(text, &repository), text, "{text}");
    }
    assert_eq!(
        link_issue_references("fixes: #5", &repository),
        "fixes: [#5](https://github.com/octo/repo/issues/5)",
        "a space still lets it through"
    );
}

#[test]
fn the_host_mismatch_question_reveals_hidden_characters_in_the_link_text() {
    let url = Url::parse("https://evil.example/login").expect("valid");
    let prompt = crate::github_markdown::host_mismatch_prompt("github.com/\u{202E}nigol", &url);
    assert!(prompt.contains("github.com/<U+202E>nigol"), "{prompt}");
    assert!(!prompt.contains('\u{202E}'));
}
