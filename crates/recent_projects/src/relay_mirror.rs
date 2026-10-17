//! A tab that shows, and types into, a terminal running on another Zode.
//!
//! The other Zode streams the terminal's screen and output over an attached
//! stream, and what is typed goes back the same way. Nothing runs here: the
//! terminal is a surface the stream draws on.

use anyhow::{Context as _, Result};
use futures::{Stream, StreamExt as _, channel::mpsc};
use gpui::{AppContext as _, Entity, WeakEntity, Window};
use remote_relay_client::{RelaySession, RelaySessionEvent, StreamReceiver, send_data_waiting};
use remote_relay_protocol::{Control, TerminalSummary};
use settings::Settings as _;
use terminal::{
    RemoteBackedOptions, RemoteTerminalCommand, Terminal, TerminalBuilder,
    terminal_settings::TerminalSettings,
};
use terminal_view::TerminalView;
use util::{ResultExt as _, paths::PathStyle};
use workspace::Workspace;

/// Opens `terminal` of the host `session` is connected to as a tab in the
/// workspace's active pane.
pub(crate) fn open_mirror_tab(
    session: &Entity<RelaySession>,
    terminal: &TerminalSummary,
    workspace: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Result<()> {
    let (commands_sender, commands) = mpsc::unbounded();
    let (stream_sender, output) = RelaySession::stream_channel();
    let host_name = session.read(cx).host_name().to_string();
    let stream_id = session.update(cx, |session, _| {
        let stream_id = session.allocate_stream_id();
        session
            .register_stream(stream_id, stream_sender)
            .map(|()| stream_id)
    })?;

    let surface =
        match attach_terminal(session, terminal, host_name, stream_id, commands_sender, cx) {
            Ok(surface) => surface,
            Err(error) => {
                session.update(cx, |session, _| session.unregister_stream(stream_id));
                return Err(error);
            }
        };

    let project = workspace.read(cx).project().downgrade();
    let view = cx.new(|cx| {
        TerminalView::new(
            surface.clone(),
            workspace.downgrade(),
            None,
            project,
            window,
            cx,
        )
    });
    workspace.update(cx, |workspace, cx| {
        workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
    });

    pump(
        session.downgrade(),
        surface.downgrade(),
        terminal.id.clone(),
        stream_id,
        output,
        commands,
        cx,
    );
    Ok(())
}

/// Builds the terminal surface and asks the other Zode to stream into it.
fn attach_terminal(
    session: &Entity<RelaySession>,
    terminal: &TerminalSummary,
    host_name: String,
    stream_id: u32,
    commands_sender: mpsc::UnboundedSender<RemoteTerminalCommand>,
    cx: &mut gpui::App,
) -> Result<Entity<Terminal>> {
    let settings = TerminalSettings::get_global(cx);
    let (cursor_shape, alternate_scroll, scroll_history) = (
        settings.cursor_shape,
        settings.alternate_scroll,
        settings.max_scroll_history_lines,
    );
    let title = format!("{host_name}: {}", terminal.title);
    let surface = TerminalBuilder::new_remote_backed(
        commands_sender,
        RemoteBackedOptions {
            title_override: Some(title),
            ..Default::default()
        },
        cursor_shape,
        alternate_scroll,
        scroll_history,
        0,
        cx.background_executor(),
        PathStyle::local(),
    )
    .context("could not create a terminal for the tab")?;
    let surface = cx.new(|cx| surface.subscribe(cx));

    session
        .update(cx, |session, cx| {
            session.send_control(
                &Control::TerminalAttach {
                    terminal_id: terminal.id.clone(),
                    stream_id,
                },
                cx,
            )
        })
        .context("could not ask the other Zode for the terminal")?;
    Ok(surface)
}

fn pump(
    session: WeakEntity<RelaySession>,
    surface: WeakEntity<Terminal>,
    terminal_id: String,
    stream_id: u32,
    output: StreamReceiver,
    mut commands: mpsc::UnboundedReceiver<RemoteTerminalCommand>,
    cx: &mut gpui::App,
) {
    // What the other Zode prints.
    cx.spawn({
        let session = session.clone();
        let surface = surface.clone();
        let terminal_id = terminal_id.clone();
        async move |cx| {
            // Closing of the terminal is announced as a control message that
            // names it, not on the stream.
            let closed = cx.update(|cx| {
                let (sender, receiver) = mpsc::unbounded();
                let subscription = session.upgrade().map(|session| {
                    cx.subscribe(&session, move |_, event: &RelaySessionEvent, _| {
                        if let RelaySessionEvent::Control(Control::TerminalClosed {
                            terminal_id,
                            exit_code,
                        }) = event
                            && sender
                                .unbounded_send((terminal_id.clone(), *exit_code))
                                .is_err()
                        {
                            log::debug!("the mirror tab stopped listening for its terminal");
                        }
                    })
                });
                (receiver, subscription)
            });
            let (closed, _subscription) = closed;
            forward_terminal_events(output, closed, &terminal_id, |event| match event {
                TerminalEvent::Output(bytes) => surface
                    .update(cx, |terminal, cx| terminal.feed_remote_output(bytes, cx))
                    .is_ok(),
                TerminalEvent::Exited(exit_code) => {
                    surface
                        .update(cx, |terminal, cx| {
                            terminal.remote_process_exited(exit_code, cx)
                        })
                        .ok();
                    true
                }
            })
            .await;
        }
    })
    .detach();

    // What is typed here. Ends when the tab is closed, which is when the
    // terminal and its command channel are dropped.
    cx.spawn(async move |cx| {
        while let Some(command) = commands.next().await {
            match command {
                RemoteTerminalCommand::Input(bytes) => {
                    if let Err(error) = send_data_waiting(&session, stream_id, &bytes, cx).await {
                        log::warn!("could not send keystrokes to the other Zode: {error}");
                        surface
                            .update(cx, |terminal, cx| terminal.remote_process_exited(None, cx))
                            .ok();
                        return;
                    }
                }
                // The other Zode decides the size of what it draws.
                RemoteTerminalCommand::Resize { .. } => {}
                RemoteTerminalCommand::Close => break,
            }
        }
        session
            .update(cx, |session, cx| {
                session.send_control(
                    &Control::TerminalDetach {
                        terminal_id: terminal_id.clone(),
                    },
                    cx,
                )?;
                session.send_end_of_stream(stream_id, cx)
            })
            .log_err()
            .and_then(|result| result.log_err());
    })
    .detach();
}

enum TerminalEvent<'a> {
    Output(&'a [u8]),
    Exited(Option<i32>),
}

/// Reports what the other Zode prints, and the terminal's end, to `handle`
/// until the stream ends, the other Zode reports the terminal closed, or
/// `handle` returns false for output because the tab is gone.
async fn forward_terminal_events(
    output: impl Stream<Item = Vec<u8>> + Unpin,
    mut closed: mpsc::UnboundedReceiver<(String, Option<i32>)>,
    terminal_id: &str,
    mut handle: impl FnMut(TerminalEvent<'_>) -> bool,
) {
    let mut output = output.fuse();
    loop {
        futures::select_biased! {
            bytes = output.next() => {
                let Some(bytes) = bytes else {
                    handle(TerminalEvent::Exited(None));
                    return;
                };
                if !handle(TerminalEvent::Output(&bytes)) {
                    return;
                }
            }
            closing = closed.next() => {
                // A closed channel stays ready forever, so it must end the
                // loop rather than be polled again.
                let Some((id, exit_code)) = closing else {
                    handle(TerminalEvent::Exited(None));
                    return;
                };
                if id == terminal_id {
                    handle(TerminalEvent::Exited(exit_code));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc as std_mpsc, thread, time::Duration};

    #[test]
    fn the_loop_ends_and_marks_the_terminal_exited_when_the_close_channel_is_gone() {
        let (_output_sender, output) = mpsc::unbounded();
        let (closed_sender, closed) = mpsc::unbounded();
        drop(closed_sender);

        let (done_sender, done) = std_mpsc::channel();
        thread::spawn(move || {
            let mut exits = Vec::new();
            futures::executor::block_on(forward_terminal_events(
                output,
                closed,
                "terminal-1",
                |event| {
                    if let TerminalEvent::Exited(exit_code) = event {
                        exits.push(exit_code);
                    }
                    true
                },
            ));
            done_sender.send(exits).ok();
        });

        let exits = done
            .recv_timeout(Duration::from_secs(5))
            .expect("the loop kept polling a closed channel");
        assert_eq!(exits, vec![None]);
    }

    #[test]
    fn only_the_close_of_this_terminal_marks_it_exited() {
        let (_output_sender, output) = mpsc::unbounded();
        let (closed_sender, closed) = mpsc::unbounded();
        closed_sender
            .unbounded_send(("other".to_string(), Some(1)))
            .unwrap();
        closed_sender
            .unbounded_send(("terminal-1".to_string(), Some(7)))
            .unwrap();
        drop(closed_sender);

        let mut exits = Vec::new();
        futures::executor::block_on(forward_terminal_events(
            output,
            closed,
            "terminal-1",
            |event| {
                if let TerminalEvent::Exited(exit_code) = event {
                    exits.push(exit_code);
                }
                true
            },
        ));
        assert_eq!(exits, vec![Some(7), None]);
    }
}
