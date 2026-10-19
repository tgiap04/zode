//! The two project-appearance modals as a person meets them: opened by the
//! same functions the project menu calls, drawn by the modal layer, closed or
//! confirmed through actions rather than by reaching into their fields.

use crate::Sidebar;
use crate::colour_modal::ColourModal;
use crate::initials_modal::InitialsModal;
use crate::project_appearance_actions::{prompt_for_colour, prompt_for_initials};
use crate::sidebar_tests::init_test;
use fs::FakeFs;
use gpui::{
    AppContext as _, Entity, ManagedView, SharedString, TestAppContext, VisualTestContext, px, rems,
};
use project::{Project, ProjectGroupKey};
use serde_json::json;
use util::path;
use workspace::MultiWorkspace;

struct OpenProject {
    multi_workspace: Entity<MultiWorkspace>,
    sidebar: Entity<Sidebar>,
    key: ProjectGroupKey,
    label: SharedString,
}

async fn one_open_project(cx: &mut TestAppContext) -> (OpenProject, &mut VisualTestContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/root_a"), json!({ "a.txt": "" }))
        .await;
    let project = Project::test(fs, [path!("/root_a").as_ref()], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        sidebar
    });
    cx.run_until_parked();
    let key = multi_workspace.read_with(cx, |mw, cx| mw.project_groups(cx)[0].key.clone());
    (
        OpenProject {
            multi_workspace,
            sidebar,
            key,
            label: "root_a".into(),
        },
        cx,
    )
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn open_colour_modal(open: &OpenProject, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        prompt_for_colour(
            &open.multi_workspace.downgrade(),
            &open.sidebar.downgrade(),
            &open.key,
            &open.label,
            window,
            cx,
        );
    });
    redraw(cx);
}

fn open_initials_modal(open: &OpenProject, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        prompt_for_initials(
            &open.multi_workspace.downgrade(),
            &open.key,
            &open.label,
            window,
            cx,
        );
    });
    redraw(cx);
}

fn modal_is_open<V: ManagedView>(open: &OpenProject, cx: &mut VisualTestContext) -> bool {
    open.multi_workspace.read_with(cx, |mw, cx| {
        mw.workspace().read(cx).active_modal::<V>(cx).is_some()
    })
}

/// The small modal tier: a confirm-sized box, not the old `w_80`.
#[gpui::test]
async fn the_colour_modal_is_a_small_modal(cx: &mut TestAppContext) {
    let (open, cx) = one_open_project(cx).await;
    open_colour_modal(&open, cx);

    let modal = cx
        .debug_bounds("colour-modal")
        .expect("the colour modal must be drawn");
    let rem_size = cx.update(|window, _| window.rem_size());
    assert_eq!(modal.size.width, rems(28.).to_pixels(rem_size));
}

#[gpui::test]
async fn escape_dismisses_the_colour_modal_without_writing(cx: &mut TestAppContext) {
    let (open, cx) = one_open_project(cx).await;
    open_colour_modal(&open, cx);
    assert!(modal_is_open::<ColourModal>(&open, cx));

    cx.dispatch_action(menu::Cancel);
    redraw(cx);

    assert!(!modal_is_open::<ColourModal>(&open, cx));
    let colour = open
        .multi_workspace
        .read_with(cx, |mw, _| mw.project_presentation(&open.key).colour);
    assert_eq!(
        colour, None,
        "cancelling must leave the project's colour alone"
    );
}

#[gpui::test]
async fn confirm_writes_the_colour_and_closes_the_colour_modal(cx: &mut TestAppContext) {
    let (open, cx) = one_open_project(cx).await;
    open_colour_modal(&open, cx);

    cx.dispatch_action(menu::Confirm);
    redraw(cx);

    assert!(!modal_is_open::<ColourModal>(&open, cx));
    let colour = open
        .multi_workspace
        .read_with(cx, |mw, _| mw.project_presentation(&open.key).colour);
    assert!(colour.is_some(), "confirming must keep the picked colour");
}

#[gpui::test]
async fn the_initials_modal_is_a_small_modal(cx: &mut TestAppContext) {
    let (open, cx) = one_open_project(cx).await;
    open_initials_modal(&open, cx);

    let modal = cx
        .debug_bounds("initials-modal")
        .expect("the initials modal must be drawn");
    let rem_size = cx.update(|window, _| window.rem_size());
    assert_eq!(modal.size.width, rems(28.).to_pixels(rem_size));
}

/// The footer is drawn beside `Modal` rather than inside it, so it has to land
/// below the body and inside the box, not past its bottom edge.
#[gpui::test]
async fn the_initials_footer_stays_inside_the_modal(cx: &mut TestAppContext) {
    let (open, cx) = one_open_project(cx).await;
    open_initials_modal(&open, cx);

    let modal = cx
        .debug_bounds("initials-modal")
        .expect("the initials modal must be drawn");
    let footer = cx
        .debug_bounds("initials-modal-footer")
        .expect("and its footer");
    assert!(
        modal.contains(&footer.origin) && footer.bottom() <= modal.bottom(),
        "footer {footer:?} must sit inside modal {modal:?}"
    );
    assert!(
        footer.size.width <= modal.size.width && footer.size.width + px(4.) >= modal.size.width,
        "the footer spans the box inside its border: {footer:?} in {modal:?}"
    );
}

#[gpui::test]
async fn escape_dismisses_the_initials_modal_without_writing(cx: &mut TestAppContext) {
    let (open, cx) = one_open_project(cx).await;
    open_initials_modal(&open, cx);
    assert!(modal_is_open::<InitialsModal>(&open, cx));

    cx.dispatch_action(menu::Cancel);
    redraw(cx);

    assert!(!modal_is_open::<InitialsModal>(&open, cx));
    let initials = open
        .multi_workspace
        .read_with(cx, |mw, _| mw.project_presentation(&open.key).initials);
    assert_eq!(initials, None);
}

#[gpui::test]
async fn confirm_writes_the_typed_initials(cx: &mut TestAppContext) {
    let (open, cx) = one_open_project(cx).await;
    open_initials_modal(&open, cx);

    cx.simulate_input("qz");
    cx.dispatch_action(menu::Confirm);
    redraw(cx);

    assert!(!modal_is_open::<InitialsModal>(&open, cx));
    let initials = open
        .multi_workspace
        .read_with(cx, |mw, _| mw.project_presentation(&open.key).initials);
    assert_eq!(
        initials.map(|text| text.to_string()),
        Some("qz".to_string())
    );
}
