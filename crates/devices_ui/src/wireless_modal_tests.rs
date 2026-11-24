use std::cell::Cell;
use std::rc::Rc;

use devices::{DeviceKey, DeviceKind};
use gpui::{AppContext as _, DismissEvent, Entity, TestAppContext, VisualTestContext};
use ui_input::InputField;

use crate::devices_panel::DevicesPanel;
use crate::test_support::{ADB, Rig, draw, init_test, panel_in_window};
use crate::wireless_modal::{Step, WirelessModal};

const PAIR_ADDRESS: &str = "192.168.1.5:37099";
const CONNECT_ADDRESS: &str = "192.168.1.5:5555";
const CODE: &str = "482915";
const PAIRED: &str =
    "Enter pairing code: Successfully paired to 192.168.1.5:37099 [guid=adb-R58M-x]";
const WIRELESS_PHONE: &str = "192.168.1.5:5555 device product:p model:Pixel_8 transport_id:5\n";

struct Opened {
    panel: Entity<DevicesPanel>,
    modal: Entity<WirelessModal>,
    dismissals: Rc<Cell<usize>>,
}

async fn open<'a>(cx: &'a mut TestAppContext, rig: &Rig) -> (Opened, &'a mut VisualTestContext) {
    let (panel, cx) = panel_in_window(cx, rig, false).await;
    let modal = cx.update(|window, cx| {
        let panel = panel.downgrade();
        cx.new(|cx| WirelessModal::new(panel, window, cx))
    });
    let dismissals = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let dismissals = dismissals.clone();
        cx.subscribe(&modal, move |_, _: &DismissEvent, _| {
            dismissals.set(dismissals.get() + 1)
        })
        .detach();
    });
    (
        Opened {
            panel,
            modal,
            dismissals,
        },
        cx,
    )
}

fn type_into(field: &Entity<InputField>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let editor = field.read(cx).editor().clone();
        editor.set_text(text, window, cx);
    });
}

fn text_of(field: &Entity<InputField>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| field.read(cx).text(cx))
}

fn fields(
    modal: &Entity<WirelessModal>,
    cx: &mut VisualTestContext,
) -> (Entity<InputField>, Entity<InputField>, Entity<InputField>) {
    modal.read_with(cx, |modal, _| {
        (
            modal.pair_address.clone(),
            modal.pair_code.clone(),
            modal.connect_address.clone(),
        )
    })
}

fn pair(opened: &Opened, address: &str, code: &str, cx: &mut VisualTestContext) {
    let (pair_address, pair_code, _) = fields(&opened.modal, cx);
    type_into(&pair_address, address, cx);
    type_into(&pair_code, code, cx);
    opened
        .modal
        .update_in(cx, |modal, window, cx| modal.pair(window, cx));
    cx.run_until_parked();
}

fn connect(opened: &Opened, address: &str, cx: &mut VisualTestContext) {
    let (_, _, connect_address) = fields(&opened.modal, cx);
    opened
        .modal
        .update_in(cx, |modal, window, cx| modal.skip_pairing(window, cx));
    type_into(&connect_address, address, cx);
    opened
        .modal
        .update_in(cx, |modal, window, cx| modal.connect(window, cx));
    cx.run_until_parked();
}

fn error_of(opened: &Opened, cx: &mut VisualTestContext) -> Option<String> {
    opened.modal.read_with(cx, |modal, _| {
        modal.error.as_ref().map(|error| error.to_string())
    })
}

fn step_of(opened: &Opened, cx: &mut VisualTestContext) -> Step {
    opened.modal.read_with(cx, |modal, _| modal.step)
}

#[gpui::test]
async fn a_malformed_address_or_code_never_reaches_adb(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    let (opened, cx) = open(cx, &rig).await;

    for (address, code) in [
        ("", CODE),
        ("not an address", CODE),
        ("192.168.1.5", CODE),
        ("192.168.1.5:0", CODE),
        ("-h:5555", CODE),
        (PAIR_ADDRESS, ""),
        (PAIR_ADDRESS, "12345"),
        (PAIR_ADDRESS, "1234567"),
        (PAIR_ADDRESS, "12345a"),
    ] {
        pair(&opened, address, code, cx);
        let error = error_of(&opened, cx);
        assert!(error.is_some(), "({address:?}, {code:?}) must say why");
        assert!(
            !error.unwrap_or_default().contains(code) || code.is_empty(),
            "the message must not repeat the code"
        );
        assert_eq!(step_of(&opened, cx), Step::Pair);
    }

    assert_eq!(rig.host.invocations().len(), 0);
    assert_eq!(opened.dismissals.get(), 0);
}

#[gpui::test]
async fn addresses_off_the_local_network_are_refused_with_a_reason(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    let (opened, cx) = open(cx, &rig).await;

    pair(&opened, "8.8.8.8:5555", CODE, cx);
    let error = error_of(&opened, cx).unwrap_or_default();
    assert!(error.contains("private"), "{error}");

    pair(&opened, "example.com:5555", CODE, cx);
    assert!(error_of(&opened, cx).is_some());

    connect(&opened, "8.8.8.8:5555", cx);
    let error = error_of(&opened, cx).unwrap_or_default();
    assert!(error.contains("private"), "{error}");

    connect(&opened, "example.com:5555", cx);
    assert!(error_of(&opened, cx).is_some());

    assert_eq!(rig.host.invocations().len(), 0, "adb was never started");
}

#[gpui::test]
async fn a_failed_pairing_shows_in_the_modal_which_stays_open_and_forgets_the_code(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = Rig::linux();
    rig.host.script_output(
        ADB,
        ["pair", PAIR_ADDRESS],
        0,
        "Enter pairing code: Failed: Wrong password or connection was dropped.",
        "",
    );
    let (opened, cx) = open(cx, &rig).await;

    pair(&opened, PAIR_ADDRESS, CODE, cx);

    let error = error_of(&opened, cx).unwrap_or_default();
    assert!(error.contains("Wrong password"), "{error}");
    assert!(!error.contains(CODE));
    assert_eq!(step_of(&opened, cx), Step::Pair);
    assert_eq!(opened.dismissals.get(), 0);
    let (_, pair_code, _) = fields(&opened.modal, cx);
    assert_eq!(
        text_of(&pair_code, cx),
        "",
        "the code is gone after sending"
    );
}

#[gpui::test]
async fn the_code_goes_to_adb_on_stdin_and_nowhere_else(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.host
        .script_output(ADB, ["pair", PAIR_ADDRESS], 0, PAIRED, "");
    let (opened, cx) = open(cx, &rig).await;

    pair(&opened, PAIR_ADDRESS, CODE, cx);

    let invocations = rig.host.invocations();
    assert_eq!(invocations.len(), 1);
    let invocation = &invocations[0];
    assert_eq!(invocation.args, ["pair", PAIR_ADDRESS]);
    assert_eq!(invocation.stdin.as_deref(), Some("482915\n"));
    assert!(!format!("{invocations:?}").contains(CODE));
    assert_eq!(step_of(&opened, cx), Step::Connect);
    assert_eq!(error_of(&opened, cx), None);
    let (_, pair_code, _) = fields(&opened.modal, cx);
    assert_eq!(text_of(&pair_code, cx), "");
    assert_eq!(opened.dismissals.get(), 0, "connecting is still to do");
}

#[gpui::test]
async fn pairing_says_so_when_there_is_no_android_sdk(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::mac();
    let (opened, cx) = open(cx, &rig).await;

    pair(&opened, PAIR_ADDRESS, CODE, cx);

    assert!(error_of(&opened, cx).is_some_and(|error| error.contains("Android SDK")));
    assert_eq!(rig.host.invocations().len(), 0);
}

#[gpui::test]
async fn a_successful_connect_closes_the_modal_and_reads_the_list_once(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    rig.script_adb_devices(WIRELESS_PHONE);
    rig.host.script_output(
        ADB,
        ["connect", CONNECT_ADDRESS],
        0,
        "connected to 192.168.1.5:5555\n",
        "",
    );
    let (opened, cx) = open(cx, &rig).await;
    opened.panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();
    assert_eq!(rig.command_count(ADB, &["devices", "-l"]), 1);

    connect(&opened, CONNECT_ADDRESS, cx);

    assert_eq!(opened.dismissals.get(), 1);
    assert_eq!(
        rig.command_count(ADB, &["devices", "-l"]),
        2,
        "exactly one read after the connect"
    );
    let phone = opened.panel.read_with(cx, |panel, _| {
        panel
            .find_device(&DeviceKey::AndroidSerial(CONNECT_ADDRESS.to_string()))
            .cloned()
    });
    assert!(phone.is_some_and(|phone| phone.kind == DeviceKind::AndroidWireless));
}

#[gpui::test]
async fn a_connect_that_lands_while_a_read_is_in_flight_still_shows_the_phone(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let rig = Rig::linux();
    rig.script_adb_devices("");
    rig.script_adb_devices(WIRELESS_PHONE);
    rig.host.script_output(
        ADB,
        ["connect", CONNECT_ADDRESS],
        0,
        "connected to 192.168.1.5:5555\n",
        "",
    );
    let release = rig.runner.hold(&["devices", "-l"]);
    let (opened, cx) = open(cx, &rig).await;
    opened.panel.update(cx, |panel, cx| panel.reload(cx));
    cx.run_until_parked();

    connect(&opened, CONNECT_ADDRESS, cx);
    release.send(()).ok();
    cx.run_until_parked();

    let listed = opened.panel.read_with(cx, |panel, _| {
        panel
            .find_device(&DeviceKey::AndroidSerial(CONNECT_ADDRESS.to_string()))
            .is_some()
    });
    assert!(
        listed,
        "the read that began before the connect was replaced"
    );
    assert!(!opened.panel.read_with(cx, |panel, _| panel.is_reading()));
}

#[gpui::test]
async fn a_failed_connect_shows_adbs_words_and_reads_nothing(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.host.script_output(
        ADB,
        ["connect", CONNECT_ADDRESS],
        0,
        "failed to connect to '192.168.1.5:5555': Connection refused\n",
        "",
    );
    let (opened, cx) = open(cx, &rig).await;

    connect(&opened, CONNECT_ADDRESS, cx);

    let error = error_of(&opened, cx).unwrap_or_default();
    assert!(error.contains("Connection refused"), "{error}");
    assert_eq!(opened.dismissals.get(), 0);
    assert_eq!(rig.command_count(ADB, &["devices", "-l"]), 0);
}

#[gpui::test]
async fn closing_the_modal_forgets_a_typed_code(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    let (opened, cx) = open(cx, &rig).await;
    let (_, pair_code, _) = fields(&opened.modal, cx);
    type_into(&pair_code, CODE, cx);

    opened
        .modal
        .update_in(cx, |modal, window, cx| modal.cancel(window, cx));

    assert_eq!(text_of(&pair_code, cx), "");
    assert_eq!(opened.dismissals.get(), 1);
    assert_eq!(rig.host.invocations().len(), 0);
}

#[gpui::test]
async fn the_modal_draws_both_steps(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    let (opened, cx) = open(cx, &rig).await;
    draw(cx);
    opened
        .modal
        .update_in(cx, |modal, window, cx| modal.skip_pairing(window, cx));
    draw(cx);
    assert_eq!(step_of(&opened, cx), Step::Connect);
}

#[gpui::test]
async fn undo_cannot_bring_the_code_back_after_pairing(cx: &mut TestAppContext) {
    init_test(cx);
    let rig = Rig::linux();
    rig.host
        .script_output(ADB, ["pair", PAIR_ADDRESS], 0, PAIRED, "");
    let (opened, cx) = open(cx, &rig).await;

    pair(&opened, PAIR_ADDRESS, CODE, cx);
    let (_, pair_code, _) = fields(&opened.modal, cx);
    cx.update(|window, cx| {
        let erased = pair_code.read(cx).editor().clone();
        let editor = erased
            .as_any()
            .downcast_ref::<Entity<editor::Editor>>()
            .expect("an editor")
            .clone();
        editor.update(cx, |editor, cx| {
            editor.undo(&editor::actions::Undo, window, cx);
        });
    });

    assert_eq!(text_of(&pair_code, cx), "");
}
