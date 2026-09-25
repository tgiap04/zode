use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification, ToastNotificationManager,
        ToastTemplateType,
    },
    Win32::{
        Foundation::{LPARAM, WPARAM},
        System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize},
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::HSTRING,
};

use crate::{SafeHwnd, WM_GPUI_NOTIFICATION_EVENT};
use gpui::{Notification, SharedString};

/// Caps how many live toasts we hold for `Activated`/`Dismissed`/`Failed`
/// wiring. A dropped `ToastNotification` stops delivering events, so
/// something must own it -- but Action Center, not us, is the source of
/// truth for what's still visible, so an unbounded map would just leak COM
/// objects for toasts nobody can act on any more.
const MAX_LIVE_TOASTS: usize = 16;

type LiveToasts = RefCell<VecDeque<(SharedString, ToastNotification)>>;

/// What `handle_notification_event` (in `platform.rs`) unpacks on the main
/// thread. `activated` distinguishes a click (which must reach the caller's
/// callback) from a `Dismissed`/`Failed` event (which exists only to evict
/// `NotificationState::live_toasts`).
pub(crate) struct NotificationEvent {
    pub(crate) id: SharedString,
    pub(crate) activated: bool,
}

pub(crate) struct NotificationState {
    com_initialized: Cell<bool>,
    live_toasts: LiveToasts,
}

impl NotificationState {
    pub(crate) fn new() -> Self {
        Self {
            com_initialized: Cell::new(false),
            live_toasts: RefCell::new(VecDeque::new()),
        }
    }

    pub(crate) fn post(
        &self,
        notification: &Notification,
        platform_window: SafeHwnd,
        validation_number: usize,
    ) {
        post_toast(
            &self.com_initialized,
            &self.live_toasts,
            notification,
            platform_window,
            validation_number,
        );
    }

    /// Evicts `id` from the live-toast map. Called from the main thread once
    /// a `Dismissed`, `Failed`, or `Activated` event for it has been
    /// delivered -- see `handle_notification_event` in `platform.rs`.
    pub(crate) fn forget(&self, id: &str) {
        forget_toast(&self.live_toasts, id);
    }
}

/// Builds and shows a toast for `notification`, then keeps it alive in
/// `live_toasts` so `Activated`/`Dismissed`/`Failed` can still fire on it.
///
/// `ToastNotifier::Show` is a synchronous local call into the shell's
/// notification broker, not a network round trip, so running it inline does
/// not violate `Platform::post_notification`'s "must not block" contract.
/// Success from `Show` is not evidence anything became visible: an
/// AppUserModelID with no registered Start Menu shortcut makes both
/// `CreateToastNotifier` and `Show` return `Ok(())` while rendering nothing,
/// with no HRESULT to catch that in advance.
fn post_toast(
    com_initialized: &Cell<bool>,
    live_toasts: &LiveToasts,
    notification: &Notification,
    platform_window: SafeHwnd,
    validation_number: usize,
) {
    ensure_com_initialized(com_initialized);
    if !com_initialized.get() {
        return;
    }

    match build_and_show(notification, platform_window, validation_number) {
        Ok(toast) => remember_toast(live_toasts, notification.id.clone(), toast),
        Err(error) => {
            log::error!("post_notification: WinRT toast call failed, no toast is visible: {error}")
        }
    }
}

fn ensure_com_initialized(com_initialized: &Cell<bool>) {
    if com_initialized.get() {
        return;
    }
    // `OleInitialize` already ran in `WindowsPlatform::new`, establishing a
    // single-threaded apartment for classic OLE on this thread.
    // `RoInitialize(RO_INIT_SINGLETHREADED)` here returns `S_FALSE` ("already
    // initialized, same model"), which `windows-rs` treats as success -- not
    // redundant paranoia, this is what makes WinRT (as opposed to classic
    // OLE) activation work on this thread at all.
    match unsafe { RoInitialize(RO_INIT_SINGLETHREADED) } {
        Ok(()) => com_initialized.set(true),
        Err(error) => {
            log::error!("RoInitialize failed, toast notifications will not post: {error}")
        }
    }
}

fn remember_toast(live_toasts: &LiveToasts, id: SharedString, toast: ToastNotification) {
    let mut live = live_toasts.borrow_mut();
    if live.len() >= MAX_LIVE_TOASTS {
        live.pop_front();
    }
    live.push_back((id, toast));
}

fn forget_toast(live_toasts: &LiveToasts, id: &str) {
    live_toasts
        .borrow_mut()
        .retain(|(toast_id, _)| toast_id.as_ref() != id);
}

fn build_and_show(
    notification: &Notification,
    platform_window: SafeHwnd,
    validation_number: usize,
) -> windows::core::Result<ToastNotification> {
    let xml = ToastNotificationManager::GetTemplateContent(ToastTemplateType::ToastText02)?;
    set_text(&xml, 0, notification.title.as_ref())?;
    set_text(&xml, 1, notification.body.as_ref())?;

    let toast = ToastNotification::CreateToastNotification(&xml)?;
    // `Tag`/`Group` are what let a second post with the same
    // `Notification::id` replace the banner in place -- Windows has no
    // notion of our `id` on its own.
    toast.SetTag(&HSTRING::from(notification.id.as_ref()))?;
    toast.SetGroup(&HSTRING::from("zode-agent"))?;

    // `Activated` hands back `IInspectable` rather than the documented
    // `ToastActivatedEventArgs` -- a known windows-rs gap for this event.
    let id = notification.id.clone();
    toast.Activated(&TypedEventHandler::<
        ToastNotification,
        windows::core::IInspectable,
    >::new(move |_, _| {
        Ok(forward_to_foreground(
            platform_window,
            validation_number,
            &id,
            true,
        ))
    }))?;

    let id = notification.id.clone();
    toast.Dismissed(&TypedEventHandler::<
        ToastNotification,
        ToastDismissedEventArgs,
    >::new(move |_, _| {
        Ok(forward_to_foreground(
            platform_window,
            validation_number,
            &id,
            false,
        ))
    }))?;

    let id = notification.id.clone();
    toast.Failed(
        &TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(move |_, _| {
            Ok(forward_to_foreground(
                platform_window,
                validation_number,
                &id,
                false,
            ))
        }),
    )?;

    // No-argument `CreateToastNotifier` takes the process's implicit AUMID
    // from the shortcut it was launched from. The AUMID string itself lives
    // in exactly one place -- `zed.iss`, fed from `bundle-windows.ps1` --
    // and stays there; hardcoding it here would create a second definition
    // that drifts from the installer.
    let notifier = ToastNotificationManager::CreateToastNotifier()?;
    notifier.Show(&toast)?;

    Ok(toast)
}

fn set_text(xml: &XmlDocument, index: u32, text: &str) -> windows::core::Result<()> {
    let nodes = xml.GetElementsByTagName(&HSTRING::from("text"))?;
    nodes.Item(index)?.SetInnerText(&HSTRING::from(text))
}

/// Marshals a toast event from whatever thread WinRT delivered it on back to
/// the main thread, by posting a custom message to the platform window --
/// the same idiom `handle_gpu_device_lost` uses to reach the main thread
/// from its own background thread. Can't use `foreground_executor` instead:
/// `TypedEventHandler::new` requires `Send + 'static`, and `ForegroundExecutor`
/// is deliberately `!Send` (`PhantomData<Rc<()>>`), so it can't be captured.
fn forward_to_foreground(
    platform_window: SafeHwnd,
    validation_number: usize,
    id: &SharedString,
    activated: bool,
) {
    let payload = Box::new(NotificationEvent {
        id: id.clone(),
        activated,
    });
    let lparam = LPARAM(Box::into_raw(payload) as isize);
    let post_result = unsafe {
        PostMessageW(
            Some(platform_window.as_raw()),
            WM_GPUI_NOTIFICATION_EVENT,
            WPARAM(validation_number),
            lparam,
        )
    };
    if let Err(error) = post_result {
        log::error!("failed to forward toast event to the main thread, dropping it: {error}");
        // SAFETY: `lparam` was produced by `Box::into_raw` immediately above
        // and ownership never passed to the message queue because
        // `PostMessageW` failed, so reclaiming it here is the only way to
        // avoid leaking it.
        drop(unsafe { Box::from_raw(lparam.0 as *mut NotificationEvent) });
    }
}
