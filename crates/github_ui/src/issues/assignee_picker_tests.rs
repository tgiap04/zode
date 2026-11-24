use std::sync::Arc;

use github::{FakeGithubBackend, GithubBackend, GithubError, GithubUser};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use crate::github_view_session::test_session;
use crate::issues::assignee_picker::{
    ASSIGNEE_SEARCH_DEBOUNCE, AssigneePicker, AssigneePickerEvent, AssigneeResults, AssigneeSource,
};
use crate::issues::issues_test_support::{init, repository, workspace_window};

pub(crate) fn user(login: &str, name: Option<&str>) -> GithubUser {
    GithubUser {
        login: login.to_string(),
        name: name.map(str::to_string),
        avatar_url: Some(format!("https://avatars.githubusercontent.com/{login}")),
    }
}

/// `octocat` (The Octocat), `hubot` and `monalisa` (Mona Lisa).
pub(crate) fn people() -> FakeGithubBackend {
    FakeGithubBackend::new()
        .with_assignable_user(&repository(), user("octocat", Some("The Octocat")))
        .with_assignable_user(&repository(), user("hubot", None))
        .with_assignable_user(&repository(), user("monalisa", Some("Mona Lisa")))
}

fn picker_over(
    backend: Arc<dyn GithubBackend>,
    cx: &mut VisualTestContext,
) -> Entity<AssigneePicker> {
    let source = AssigneeSource {
        backend,
        repository: repository(),
        session: test_session(),
    };
    cx.update(|window, cx| cx.new(|cx| AssigneePicker::new(source, window, cx)))
}

fn logins(picker: &Entity<AssigneePicker>, cx: &mut VisualTestContext) -> Vec<String> {
    picker.read_with(cx, |picker, _| {
        picker
            .found()
            .iter()
            .map(|user| user.login.clone())
            .collect()
    })
}

fn type_query(picker: &Entity<AssigneePicker>, text: &str, cx: &mut VisualTestContext) {
    picker.update(cx, |picker, cx| picker.set_query(text.to_string(), cx));
}

fn settle(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(ASSIGNEE_SEARCH_DEBOUNCE);
    cx.run_until_parked();
}

#[gpui::test]
async fn quick_typing_sends_one_query_after_the_pause(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(people());
    let picker = picker_over(fake.clone(), cx);

    for text in ["m", "mo", "mon"] {
        type_query(&picker, text, cx);
        cx.executor().advance_clock(ASSIGNEE_SEARCH_DEBOUNCE / 5);
        cx.run_until_parked();
    }
    assert!(
        fake.assignable_user_queries().is_empty(),
        "nothing is sent while typing: {:?}",
        fake.assignable_user_queries()
    );
    settle(cx);

    assert_eq!(fake.assignable_user_queries(), vec!["mon".to_string()]);
    assert_eq!(logins(&picker, cx), vec!["monalisa"]);
}

#[gpui::test]
async fn an_answer_to_an_older_query_is_dropped(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(people());
    let picker = picker_over(fake.clone(), cx);

    let release_first = fake.hold_next_assignable_users();
    type_query(&picker, "o", cx);
    settle(cx);
    type_query(&picker, "hu", cx);
    settle(cx);
    assert_eq!(logins(&picker, cx), vec!["hubot"]);

    release_first.send(()).ok();
    cx.run_until_parked();
    assert_eq!(
        fake.assignable_user_queries(),
        vec!["o".to_string(), "hu".to_string()]
    );
    assert_eq!(
        logins(&picker, cx),
        vec!["hubot"],
        "the late answer is dropped"
    );
}

#[gpui::test]
async fn an_empty_query_lists_the_first_assignable_people_and_no_match_is_said(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(people());
    let picker = picker_over(fake.clone(), cx);

    picker.update(cx, |picker, cx| picker.open(cx));
    settle(cx);
    assert_eq!(logins(&picker, cx), vec!["octocat", "hubot", "monalisa"]);
    assert_eq!(fake.assignable_user_queries(), vec![String::new()]);

    type_query(&picker, "nobody", cx);
    settle(cx);
    assert_eq!(
        picker.read_with(cx, |picker, _| picker.results().clone()),
        AssigneeResults::Found(Vec::new())
    );
}

#[gpui::test]
async fn a_failed_search_is_reported_in_the_picker(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let fake = Arc::new(people().failing_with(GithubError::NotFound));
    let picker = picker_over(fake, cx);
    type_query(&picker, "oct", cx);
    settle(cx);
    assert!(matches!(
        picker.read_with(cx, |picker, _| picker.results().clone()),
        AssigneeResults::Failed(_)
    ));
}

#[gpui::test]
async fn the_keyboard_moves_through_the_results_and_enter_picks(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let picker = picker_over(Arc::new(people()), cx);
    let picked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let _subscription = cx.update(|_, cx| {
        let picked = picked.clone();
        cx.subscribe(&picker, move |_, event: &AssigneePickerEvent, _| {
            let AssigneePickerEvent::Picked(user) = event;
            picked
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(user.login.clone());
        })
    });
    picker.update(cx, |picker, cx| picker.open(cx));
    settle(cx);

    picker.update_in(cx, |picker, window, cx| {
        picker.select_next(cx);
        picker.select_next(cx);
        picker.select_next(cx);
        picker.select_previous(cx);
        picker.confirm(window, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        *picked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec!["monalisa".to_string()]
    );
    assert_eq!(
        picker.read_with(cx, |picker, _| picker.query().to_string()),
        ""
    );
}
