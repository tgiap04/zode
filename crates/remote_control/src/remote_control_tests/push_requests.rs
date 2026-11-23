//! Push notifications end to end: a browser leaves its push keys over the
//! encrypted channel, an agent tab asks for attention, and the host posts an
//! encrypted push to the account service -- or, by the rules, does not.

use agent_ui::{AgentView, AgentViewEvent, TurnEvent};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gpui::VisualTestContext;
use zode_account::AccountStatusChanged;

use super::*;

const P256DH: &str = concat!(
    // Split so the spell checker does not read a word into the key.
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkN",
    "toIAiw4"
);
const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";

fn push_hello() -> Control {
    Control::Hello {
        relay_protocol: 1,
        app_version: "0.1.5".into(),
        rpc_protocol: 1,
        capabilities: vec!["terminal".into(), "push".into()],
    }
}

fn subscribe() -> Control {
    Control::PushSubscribe {
        p256dh: P256DH.into(),
        auth: AUTH.into(),
    }
}

fn push_statuses(controls: Vec<Control>) -> Vec<(bool, Option<String>)> {
    controls
        .into_iter()
        .filter_map(|control| match control {
            Control::PushStatus { enabled, reason } => Some((enabled, reason)),
            _ => None,
        })
        .collect()
}

async fn push_rig(cx: &mut TestAppContext) -> Rig {
    build_rig(
        cx,
        true,
        AccountStatus::SignedIn(AccountUser {
            id: USER_ID.into(),
            email: "ada@example.com".into(),
            name: None,
            avatar_url: None,
        }),
        false,
        false,
        true,
    )
    .await
}

/// A paired browser that said hello listing `push` and left its keys.
async fn subscribed(cx: &mut TestAppContext) -> (Rig, Browser, Connected) {
    let rig = push_rig(cx).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &push_hello());
    connected.send(&browser, &subscribe());
    cx.run_until_parked();
    (rig, browser, connected)
}

/// Opens a Claude tab in a window, and returns it with the id it is listed
/// under.
async fn agent_tab<'a>(
    browser: &Browser,
    connected: &mut Connected,
    cx: &'a mut TestAppContext,
) -> (Entity<AgentView>, String, &'a mut VisualTestContext) {
    connected.controls(browser);
    cx.update(|cx| {
        editor::init(cx);
        project::DisableAiSettings::register(cx);
    });
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi.read_with(cx, |multi, _| multi.workspace().clone());
    workspace.update_in(cx, |workspace, window, cx| {
        AgentView::open(workspace, project::CLAUDE_CODE_AGENT_ID, None, window, cx);
    });
    cx.run_until_parked();
    let view = workspace
        .read_with(cx, |workspace, cx| {
            workspace.items_of_type::<AgentView>(cx).next()
        })
        .expect("the tab opened");
    let agent_id = connected
        .controls(browser)
        .into_iter()
        .find_map(|control| match control {
            Control::AgentUpdate { agent } => Some(agent.id),
            _ => None,
        })
        .expect("the tab is listed");
    (view, agent_id, cx)
}

fn ask_for_approval(view: &Entity<AgentView>, cx: &mut VisualTestContext) {
    view.update(cx, |_, cx| {
        cx.emit(AgentViewEvent::Turn(TurnEvent::ApprovalCleared));
        cx.emit(AgentViewEvent::Turn(TurnEvent::ApprovalNeeded));
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn a_device_that_lists_push_is_told_whether_it_will_be_pushed_to_and_an_old_one_is_not(
    cx: &mut TestAppContext,
) {
    let rig = push_rig(cx).await;
    let modern = Browser::new(&rig, "browser-1");
    let old = Browser::new(&rig, "browser-2");
    pair(&rig, &modern, cx);
    pair(&rig, &old, cx);

    let mut modern_session = open_session(&rig, &modern, cx);
    modern_session.send(&modern, &push_hello());
    let mut old_session = open_session(&rig, &old, cx);
    old_session.send(&old, &hello(1));
    cx.run_until_parked();

    assert_eq!(
        push_statuses(modern_session.controls(&modern)),
        vec![(true, None)]
    );
    assert!(push_statuses(old_session.controls(&old)).is_empty());

    set_push(cx, false, false, false);
    cx.run_until_parked();
    assert_eq!(
        push_statuses(modern_session.controls(&modern)),
        vec![(false, Some("disabled".into()))]
    );
    assert!(
        push_statuses(old_session.controls(&old)).is_empty(),
        "an old browser fails on a message it does not know"
    );
}

fn set_push(cx: &mut TestAppContext, approval: bool, answered: bool, ended: bool) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                let push = content
                    .remote_control
                    .get_or_insert_default()
                    .push_notifications
                    .get_or_insert_default();
                push.approval = Some(approval);
                push.answered = Some(answered);
                push.ended = Some(ended);
            });
        });
    });
}

#[gpui::test]
async fn an_agent_waiting_for_approval_is_pushed_unless_the_device_is_looking_at_it(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.push_key_devices()),
        vec!["browser-1".to_string()]
    );
    let (view, agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;

    connected.send(
        &browser,
        &Control::PageVisibility {
            visible: true,
            agent_id: Some(agent_id.clone()),
        },
    );
    cx.run_until_parked();
    ask_for_approval(&view, cx);
    assert!(rig.pushes.lock().is_empty(), "the device is looking at it");

    connected.send(
        &browser,
        &Control::PageVisibility {
            visible: false,
            agent_id: Some(agent_id.clone()),
        },
    );
    cx.run_until_parked();
    ask_for_approval(&view, cx);

    let pushes = rig.pushes.lock().clone();
    assert_eq!(pushes.len(), 1, "{pushes:?}");
    assert_eq!(pushes[0]["deviceId"], "browser-1");
    assert_eq!(pushes[0]["urgency"], "high");
    assert_eq!(pushes[0]["topic"].as_str().map(str::len), Some(32));
    let body = URL_SAFE_NO_PAD
        .decode(pushes[0]["payload"].as_str().expect("a payload"))
        .expect("base64url");
    assert!(body.len() > 86 && body.len() <= 4096, "{}", body.len());
    let encrypted = String::from_utf8_lossy(&body);
    assert!(
        !encrypted.contains("Claude") && !encrypted.contains(&agent_id),
        "nothing readable reaches the account service"
    );
}

#[gpui::test]
async fn a_switched_off_event_is_not_pushed(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    set_push(cx, false, true, true);
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    ask_for_approval(&view, cx);
    assert!(rig.pushes.lock().is_empty());
}

#[gpui::test]
async fn a_plan_without_pushes_stops_them_says_so_and_a_plan_read_tries_again(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    *rig.push_answer.lock() = (402, r#"{"error":"push_not_in_plan"}"#);
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;

    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1);
    assert_eq!(
        push_statuses(connected.controls(&browser)),
        vec![(false, Some("not_in_plan".into()))]
    );
    assert!(rig.host.read_with(cx, |host, _| host.push_needs_upgrade()));

    ask_for_approval(&view, cx);
    assert_eq!(
        rig.pushes.lock().len(),
        1,
        "nothing is sent until the plan changes"
    );
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.push_key_devices()),
        vec!["browser-1".to_string()],
        "the keys are kept for when it does"
    );

    // Any account event that leaves the plan as it was changes nothing.
    rig.account
        .update(cx, |_, cx| cx.emit(AccountStatusChanged));
    cx.run_until_parked();
    assert!(
        rig.host.read_with(cx, |host, _| host.push_needs_upgrade()),
        "the plan did not change"
    );
    assert!(push_statuses(connected.controls(&browser)).is_empty());

    *rig.push_answer.lock() = (202, "");
    read_plan(&rig, "pro", cx);
    assert!(!rig.host.read_with(cx, |host, _| host.push_needs_upgrade()));
    assert_eq!(
        push_statuses(connected.controls(&browser)),
        vec![(true, None)]
    );
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 2);
}

/// The other answers that mean the same are the client's to tell apart, and
/// are, in `device_identity`.
#[gpui::test]
async fn a_subscription_the_push_service_dropped_is_forgotten(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    *rig.push_answer.lock() = (410, r#"{"error":"subscription_gone"}"#);
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1);
    assert!(
        rig.host
            .read_with(cx, |host, _| host.push_key_devices())
            .is_empty()
    );
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1, "nothing is left to push to");
}

#[gpui::test]
async fn a_failure_that_may_pass_keeps_the_keys_and_is_not_retried(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    *rig.push_answer.lock() = (503, "");
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    ask_for_approval(&view, cx);
    cx.executor().advance_clock(Duration::from_secs(600));
    cx.run_until_parked();
    assert_eq!(rig.pushes.lock().len(), 1);
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.push_key_devices()),
        vec!["browser-1".to_string()]
    );
}

#[gpui::test]
async fn unsubscribing_or_being_forgotten_drops_the_keys(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    connected.send(&browser, &Control::PushUnsubscribe);
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.push_key_devices())
            .is_empty()
    );

    connected.send(&browser, &subscribe());
    cx.run_until_parked();
    assert_eq!(
        rig.host
            .read_with(cx, |host, _| host.push_key_devices())
            .len(),
        1
    );
    rig.host
        .update(cx, |host, cx| host.forget_device("browser-1", cx));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.push_key_devices())
            .is_empty()
    );
}

#[gpui::test]
async fn keys_survive_a_restart_but_not_a_device_forgotten_while_off(cx: &mut TestAppContext) {
    let (rig, _browser, _connected) = subscribed(cx).await;
    set_remote_control(cx, false, None);
    cx.run_until_parked();
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.push_key_devices()),
        vec!["browser-1".to_string()]
    );

    set_remote_control(cx, false, None);
    cx.run_until_parked();
    rig.host
        .update(cx, |host, cx| host.load_trusted_devices(cx));
    cx.run_until_parked();
    rig.host
        .update(cx, |host, cx| host.forget_device("browser-1", cx));
    cx.run_until_parked();
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, _| host.push_key_devices())
            .is_empty()
    );
}

/// The account service states the plan as `tier`, and the account reads it.
fn read_plan(rig: &Rig, tier: &'static str, cx: &mut VisualTestContext) {
    *rig.plan_tier.lock() = tier;
    rig.account
        .update(cx, |account, cx| account.refresh_plan(cx));
    cx.run_until_parked();
}

#[gpui::test]
async fn a_refusal_for_a_push_sent_before_the_plan_changed_is_ignored(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    *rig.push_answer.lock() = (402, r#"{"error":"push_not_in_plan"}"#);
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    let release = rig.hold_pushes();
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1);

    read_plan(&rig, "pro", cx);
    release.send(()).ok();
    cx.run_until_parked();
    assert!(
        !rig.host.read_with(cx, |host, _| host.push_needs_upgrade()),
        "the refusal was for the plan before"
    );
}

/// What the account service says about pushes for a Free account, which is the
/// one thing that changes when pushes are opened to it: the tier, the sources
/// and every other limit stay as they were.
fn read_free_plan_with_pushes(rig: &Rig, pushes: Option<bool>, cx: &mut VisualTestContext) {
    *rig.plan_pushes.lock() = pushes;
    read_plan(rig, "free", cx);
}

#[gpui::test]
async fn a_free_plan_that_opens_pushes_lifts_a_refusal_without_a_restart(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    read_free_plan_with_pushes(&rig, Some(false), cx);

    *rig.push_answer.lock() = (402, r#"{"error":"push_not_in_plan"}"#);
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1);
    assert!(rig.host.read_with(cx, |host, _| host.push_needs_upgrade()));
    connected.controls(&browser);

    *rig.push_answer.lock() = (202, "");
    read_free_plan_with_pushes(&rig, Some(true), cx);
    assert!(
        !rig.host.read_with(cx, |host, _| host.push_needs_upgrade()),
        "the plan now says pushes are in it"
    );
    assert_eq!(
        push_statuses(connected.controls(&browser)),
        vec![(true, None)],
        "the paired device is told, with no new pairing"
    );
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 2, "exactly one new push request");
}

#[gpui::test]
async fn a_plan_that_still_has_no_pushes_keeps_a_refusal_whatever_else_changed(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    read_free_plan_with_pushes(&rig, Some(false), cx);
    *rig.push_answer.lock() = (402, r#"{"error":"push_not_in_plan"}"#);
    ask_for_approval(&view, cx);
    connected.controls(&browser);

    read_plan(&rig, "team", cx);
    assert!(
        rig.host.read_with(cx, |host, _| host.push_needs_upgrade()),
        "a different plan that says no is still no"
    );
    assert!(push_statuses(connected.controls(&browser)).is_empty());
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1, "nothing is sent");
}

#[gpui::test]
async fn a_plan_that_becomes_unknown_keeps_a_refusal(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    read_free_plan_with_pushes(&rig, Some(false), cx);
    *rig.push_answer.lock() = (402, r#"{"error":"push_not_in_plan"}"#);
    ask_for_approval(&view, cx);
    assert!(rig.host.read_with(cx, |host, _| host.push_needs_upgrade()));
    connected.controls(&browser);

    // Signing out clears the plan: not knowing it is not the plan saying yes. (The host stops
    // right after, so what is left to see is what its paired devices were told on the way.)
    rig.account
        .update(cx, |account, cx| account.sign_out(cx).detach());
    cx.run_until_parked();

    assert!(
        push_statuses(connected.controls(&browser))
            .iter()
            .all(|(will_push, _)| !*will_push),
        "no device is told that pushes will be sent"
    );
}

#[gpui::test]
async fn a_server_that_says_nothing_about_pushes_leaves_the_refusal_to_a_plan_change(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    read_free_plan_with_pushes(&rig, None, cx);
    *rig.push_answer.lock() = (402, r#"{"error":"push_not_in_plan"}"#);
    ask_for_approval(&view, cx);
    assert!(rig.host.read_with(cx, |host, _| host.push_needs_upgrade()));

    read_free_plan_with_pushes(&rig, None, cx);
    assert!(
        rig.host.read_with(cx, |host, _| host.push_needs_upgrade()),
        "the same plan, and the 402 is still the truth"
    );

    read_plan(&rig, "pro", cx);
    assert!(
        !rig.host.read_with(cx, |host, _| host.push_needs_upgrade()),
        "a different plan from a server that never says no lifts it"
    );
}

#[gpui::test]
async fn a_refusal_for_a_push_sent_before_pushes_were_opened_is_ignored(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    read_free_plan_with_pushes(&rig, Some(false), cx);
    *rig.push_answer.lock() = (402, r#"{"error":"push_not_in_plan"}"#);
    let release = rig.hold_pushes();
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1);

    read_free_plan_with_pushes(&rig, Some(true), cx);
    release.send(()).ok();
    cx.run_until_parked();
    assert!(
        !rig.host.read_with(cx, |host, _| host.push_needs_upgrade()),
        "the refusal was for the plan before"
    );
}

/// The sender's public key from RFC 8291: another real point.
const OTHER_P256DH: &str = concat!(
    "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIg",
    "Dll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
);

#[gpui::test]
async fn a_late_gone_for_old_keys_leaves_the_new_ones_alone(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    let release = rig.hold_pushes();
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 1);

    connected.send(
        &browser,
        &Control::PushSubscribe {
            p256dh: OTHER_P256DH.into(),
            auth: AUTH.into(),
        },
    );
    cx.run_until_parked();
    *rig.push_answer.lock() = (410, r#"{"error":"subscription_gone"}"#);
    release.send(()).ok();
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.push_key_devices()),
        vec!["browser-1".to_string()],
        "the keys the device just left are not the ones that were gone"
    );
}

#[gpui::test]
async fn a_push_that_never_returns_gives_up_and_is_not_doubled_meanwhile(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    let _release = rig.hold_pushes();
    ask_for_approval(&view, cx);
    ask_for_approval(&view, cx);
    assert_eq!(
        rig.pushes.lock().len(),
        1,
        "one approval push for this agent is already on its way"
    );
    assert_eq!(rig.host.read_with(cx, |host, _| host.pushes_in_flight()), 1);

    cx.executor().advance_clock(Duration::from_secs(15));
    cx.run_until_parked();
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.pushes_in_flight()),
        0,
        "a push gives up after fifteen seconds"
    );
    ask_for_approval(&view, cx);
    assert_eq!(rig.pushes.lock().len(), 2);
}

#[gpui::test]
async fn a_page_that_stopped_saying_it_is_visible_is_pushed_to(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    connected.send(
        &browser,
        &Control::PageVisibility {
            visible: true,
            agent_id: Some(agent_id),
        },
    );
    cx.run_until_parked();
    ask_for_approval(&view, cx);
    assert!(rig.pushes.lock().is_empty());

    cx.executor().advance_clock(Duration::from_secs(151));
    cx.run_until_parked();
    ask_for_approval(&view, cx);
    assert_eq!(
        rig.pushes.lock().len(),
        1,
        "a phone that locked without saying so is not looking"
    );
}

#[gpui::test]
async fn nothing_is_pushed_once_remote_control_is_off(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = subscribed(cx).await;
    let (view, _agent_id, cx) = agent_tab(&browser, &mut connected, cx).await;
    cx.update(|_, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content.remote_control.get_or_insert_default().enabled = Some(false);
            });
        });
    });
    cx.run_until_parked();
    ask_for_approval(&view, cx);
    assert!(rig.pushes.lock().is_empty());
}
