//! What a terminal needs in order to be mirrored to another device: a tap on
//! the raw bytes the pty produces, and a way to describe the current screen as
//! bytes a fresh emulator can replay.
//!
//! The two are only useful together. The tap runs on the pty reader thread
//! while the emulator lock is held, and a snapshot is taken under that same
//! lock, so the sequence number a snapshot carries splits the byte stream
//! exactly: everything at or below it is already drawn in the snapshot,
//! everything above it still has to be sent.
//!
//! The tap sees bytes before the emulator parses them, so a snapshot taken
//! while the emulator holds some of them back -- inside a synchronized update
//! (DEC mode 2026) or halfway through an escape sequence -- has a sequence that
//! covers bytes the screen does not show yet, and the reader of the mirror sees
//! the stale or partial screen until the program next redraws. Consumers that
//! snapshot while output is flowing should follow up with another snapshot
//! once it settles.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use alacritty_terminal::{
    Term,
    event::EventListener,
    event_loop::OutputTap,
    grid::Dimensions,
    index::{Column, Line},
    term::{
        TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, CursorShape, NamedColor},
};
use parking_lot::Mutex;
use smol::channel::{self, Receiver, RecvError, Sender, TryRecvError, TrySendError};

/// How many raw chunks may wait for the foreground before the tap starts
/// dropping them. Dropping is safe: the gap is visible in the sequence numbers
/// and the consumer answers it with a fresh snapshot.
pub const REMOTE_TAP_QUEUE_CAPACITY: usize = 256;

/// How many bytes may wait for the foreground before the tap starts dropping
/// chunks. Bounds memory where the chunk count alone cannot: one read can be a
/// megabyte, so a count of 256 would allow a quarter of a gigabyte.
pub const REMOTE_TAP_QUEUE_BYTES: usize = 4 * 1024 * 1024;

/// How much scrollback a snapshot carries. The reader of a mirrored terminal
/// wants the recent past, not ten thousand lines pushed through a relay.
pub const REMOTE_SNAPSHOT_HISTORY_LINES: usize = 1000;

/// One read from the pty, in the order it arrived.
///
/// `sequence` counts reads since the tap was installed, starting at 1. A read
/// may hold several writes of the child and may end anywhere inside an escape
/// sequence, so consumers must treat the chunks as one byte stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteOutputChunk {
    pub sequence: u64,
    pub bytes: Vec<u8>,
}

/// The consuming half of an installed tap.
pub struct RemoteTap {
    receiver: Receiver<RemoteOutputChunk>,
    overflowed: Arc<AtomicBool>,
    queued_bytes: Arc<AtomicUsize>,
}

impl RemoteTap {
    /// The next chunk, waiting for one if none has arrived.
    pub async fn recv(&self) -> Result<RemoteOutputChunk, RecvError> {
        let chunk = self.receiver.recv().await?;
        self.queued_bytes
            .fetch_sub(chunk.bytes.len(), Ordering::AcqRel);
        Ok(chunk)
    }

    /// The next chunk if one is waiting.
    pub fn try_recv(&self) -> Result<RemoteOutputChunk, TryRecvError> {
        let chunk = self.receiver.try_recv()?;
        self.queued_bytes
            .fetch_sub(chunk.bytes.len(), Ordering::AcqRel);
        Ok(chunk)
    }

    /// Chunks waiting to be taken.
    pub fn len(&self) -> usize {
        self.receiver.len()
    }

    pub fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }

    /// Whether the tap had to drop a chunk since this was last asked. Reading
    /// clears it.
    pub fn take_overflowed(&self) -> bool {
        self.overflowed.swap(false, Ordering::AcqRel)
    }
}

struct TapSink {
    sender: Sender<RemoteOutputChunk>,
    overflowed: Arc<AtomicBool>,
    queued_bytes: Arc<AtomicUsize>,
    sequence: u64,
}

/// Shared between the pty reader thread, which writes to it, and the terminal
/// entity, which installs and removes the sink.
///
/// `active` exists so a terminal nobody mirrors pays one relaxed atomic load
/// per read instead of a lock.
#[derive(Default)]
pub(crate) struct RemoteTapSlot {
    active: AtomicBool,
    sink: Mutex<Option<TapSink>>,
}

impl RemoteTapSlot {
    pub(crate) fn output_tap(self: &Arc<Self>) -> OutputTap {
        let slot = self.clone();
        Box::new(move |bytes| slot.observe(bytes))
    }

    /// Runs on the pty reader thread with the emulator lock held, so it must
    /// not block and must not touch the emulator.
    fn observe(&self, bytes: &[u8]) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let mut guard = self.sink.lock();
        let Some(sink) = guard.as_mut() else {
            return;
        };
        // Numbered even when dropped, which is how the consumer sees the gap.
        sink.sequence += 1;
        if sink.queued_bytes.load(Ordering::Acquire) + bytes.len() > REMOTE_TAP_QUEUE_BYTES {
            sink.overflowed.store(true, Ordering::Release);
            return;
        }
        let chunk = RemoteOutputChunk {
            sequence: sink.sequence,
            bytes: bytes.to_vec(),
        };
        // Counted before it is sent: the consumer may take it, and subtract it,
        // the instant it is in the queue.
        sink.queued_bytes.fetch_add(bytes.len(), Ordering::AcqRel);
        match sink.sender.try_send(chunk) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                sink.queued_bytes.fetch_sub(bytes.len(), Ordering::AcqRel);
                sink.overflowed.store(true, Ordering::Release);
            }
            // The consumer is gone; the terminal entity removes the sink next.
            Err(TrySendError::Closed(_)) => {
                sink.queued_bytes.fetch_sub(bytes.len(), Ordering::AcqRel);
            }
        }
    }

    pub(crate) fn install(&self) -> RemoteTap {
        let (sender, receiver) = channel::bounded(REMOTE_TAP_QUEUE_CAPACITY);
        let overflowed = Arc::new(AtomicBool::new(false));
        let queued_bytes = Arc::new(AtomicUsize::new(0));
        *self.sink.lock() = Some(TapSink {
            sender,
            overflowed: overflowed.clone(),
            queued_bytes: queued_bytes.clone(),
            sequence: 0,
        });
        self.active.store(true, Ordering::Release);
        RemoteTap {
            receiver,
            overflowed,
            queued_bytes,
        }
    }

    pub(crate) fn remove(&self) {
        self.active.store(false, Ordering::Release);
        *self.sink.lock() = None;
    }

    pub(crate) fn is_installed(&self) -> bool {
        self.sink.lock().is_some()
    }

    /// Chunks observed so far. Must be read under the emulator lock to mean
    /// "chunks already reflected in the grid".
    pub(crate) fn sequence(&self) -> u64 {
        self.sink.lock().as_ref().map_or(0, |sink| sink.sequence)
    }
}

/// The screen as bytes, and where in the output stream it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSnapshot {
    pub sequence: u64,
    pub columns: u16,
    pub rows: u16,
    pub bytes: Vec<u8>,
}

const RENDERING_FLAGS: Flags = Flags::INVERSE
    .union(Flags::BOLD)
    .union(Flags::ITALIC)
    .union(Flags::DIM)
    .union(Flags::HIDDEN)
    .union(Flags::STRIKEOUT)
    .union(Flags::ALL_UNDERLINES);

/// DEC private modes a snapshot restores, beside the ones handled by name
/// below. Anything not listed is not mirrored: a program that sets it will have
/// set it again by the time it next redraws.
const PRIVATE_MODES: [(TermMode, u16); 11] = [
    (TermMode::APP_CURSOR, 1),
    (TermMode::LINE_WRAP, 7),
    (TermMode::MOUSE_REPORT_CLICK, 1000),
    (TermMode::MOUSE_DRAG, 1002),
    (TermMode::MOUSE_MOTION, 1003),
    (TermMode::FOCUS_IN_OUT, 1004),
    (TermMode::UTF8_MOUSE, 1005),
    (TermMode::SGR_MOUSE, 1006),
    (TermMode::ALTERNATE_SCROLL, 1007),
    (TermMode::BRACKETED_PASTE, 2004),
    (TermMode::SHOW_CURSOR, 25),
];

/// Describes `term` as bytes that rebuild it on any emulator, whatever state
/// that emulator was in: they open with a full reset.
///
/// Limits, so nobody is surprised later: a screen in the alternate buffer is
/// sent without the primary buffer behind it, scroll regions, underline colour
/// and hyperlinks are not carried, and a snapshot cut while the child was
/// halfway through an escape sequence cannot say so.
pub(crate) fn render_snapshot<T: EventListener>(term: &Term<T>, history_limit: usize) -> Vec<u8> {
    let grid = term.grid();
    let columns = grid.columns();
    let rows = grid.screen_lines();
    let mode = *term.mode();
    let in_alternate_screen = mode.contains(TermMode::ALT_SCREEN);

    let history = if in_alternate_screen {
        0
    } else {
        grid.history_size().min(history_limit)
    };
    let mut out = Vec::with_capacity(columns * (rows + history));
    out.extend_from_slice(b"\x1bc");
    if in_alternate_screen {
        out.extend_from_slice(b"\x1b[?1049h");
    }
    let first_line = -(history as i32);
    let last_line = rows as i32 - 1;

    let mut pen = Pen::default();
    for line in first_line..=last_line {
        let row = &grid[Line(line)];
        let wrapped = columns > 0 && row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        let mut end = columns;
        if !wrapped {
            while end > 0 && is_trailing_blank(&row[Column(end - 1)]) {
                end -= 1;
            }
        }
        for column in 0..end {
            let cell = &row[Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            pen.move_to(cell, &mut out);
            push_character(&mut out, cell.c);
            for zero_width in cell.zerowidth().unwrap_or_default() {
                push_character(&mut out, *zero_width);
            }
        }
        if line < last_line && !wrapped {
            pen.reset(&mut out);
            out.extend_from_slice(b"\r\n");
        }
    }
    pen.reset(&mut out);

    // Origin mode homes the cursor when it is set, so it goes first and the
    // cursor is placed after it. Scroll regions are not carried, which makes
    // the origin the top left of the screen either way.
    if mode.contains(TermMode::ORIGIN) {
        push_private_mode(&mut out, 6, true);
    }
    let cursor = grid.cursor.point;
    out.extend_from_slice(
        format!("\x1b[{};{}H", cursor.line.0 + 1, cursor.column.0 + 1).as_bytes(),
    );
    if grid.cursor.input_needs_wrap && mode.contains(TermMode::LINE_WRAP) {
        // Positioning the cursor clears "the next character wraps first". The
        // last cell is written again, which leaves the cursor in that state.
        let cell = &grid[cursor.line][cursor.column];
        if !cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            let mut pen = Pen::default();
            pen.move_to(cell, &mut out);
            push_character(&mut out, cell.c);
            pen.reset(&mut out);
        }
    }

    for (flag, code) in PRIVATE_MODES {
        // Cursor visibility goes last so the cursor is not drawn mid-restore.
        if code != 25 {
            push_private_mode(&mut out, code, mode.contains(flag));
        }
    }
    out.extend_from_slice(if mode.contains(TermMode::APP_KEYPAD) {
        b"\x1b="
    } else {
        b"\x1b>"
    });
    out.extend_from_slice(if mode.contains(TermMode::LINE_FEED_NEW_LINE) {
        b"\x1b[20h"
    } else {
        b"\x1b[20l"
    });
    out.extend_from_slice(if mode.contains(TermMode::INSERT) {
        b"\x1b[4h"
    } else {
        b"\x1b[4l"
    });
    push_keyboard_mode(&mut out, mode);
    push_cursor_style(&mut out, term);
    push_private_mode(&mut out, 25, mode.contains(TermMode::SHOW_CURSOR));
    out
}

/// The kitty keyboard protocol flags the program asked for, which decide how
/// the other end must encode keys it sends back.
fn push_keyboard_mode(out: &mut Vec<u8>, mode: TermMode) {
    let mut flags = 0u32;
    for (flag, bit) in [
        (TermMode::DISAMBIGUATE_ESC_CODES, 1),
        (TermMode::REPORT_EVENT_TYPES, 2),
        (TermMode::REPORT_ALTERNATE_KEYS, 4),
        (TermMode::REPORT_ALL_KEYS_AS_ESC, 8),
        (TermMode::REPORT_ASSOCIATED_TEXT, 16),
    ] {
        if mode.contains(flag) {
            flags |= bit;
        }
    }
    if flags != 0 {
        out.extend_from_slice(b"\x1b[>");
        push_number(out, flags);
        out.push(b'u');
    }
}

fn push_private_mode(out: &mut Vec<u8>, code: u16, enabled: bool) {
    out.extend_from_slice(format!("\x1b[?{code}{}", if enabled { 'h' } else { 'l' }).as_bytes());
}

fn push_cursor_style<T: EventListener>(out: &mut Vec<u8>, term: &Term<T>) {
    let style = term.cursor_style();
    let base = match style.shape {
        CursorShape::Block | CursorShape::HollowBlock => 1,
        CursorShape::Underline => 3,
        CursorShape::Beam => 5,
        CursorShape::Hidden => return,
    };
    let code = if style.blinking { base } else { base + 1 };
    out.extend_from_slice(format!("\x1b[{code} q").as_bytes());
}

fn push_character(out: &mut Vec<u8>, character: char) {
    // A cell holding NUL or another control character would be read back as
    // a command, so it is drawn as a blank.
    let printable = if character.is_control() {
        ' '
    } else {
        character
    };
    let mut buffer = [0u8; 4];
    out.extend_from_slice(printable.encode_utf8(&mut buffer).as_bytes());
}

fn is_trailing_blank(cell: &Cell) -> bool {
    cell.c == ' '
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell
            .flags
            .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
}

/// The attributes currently in force on the emulator being written to.
#[derive(Clone, Copy, PartialEq)]
struct Pen {
    foreground: Color,
    background: Color,
    flags: Flags,
}

impl Default for Pen {
    fn default() -> Self {
        Self {
            foreground: Color::Named(NamedColor::Foreground),
            background: Color::Named(NamedColor::Background),
            flags: Flags::empty(),
        }
    }
}

impl Pen {
    fn move_to(&mut self, cell: &Cell, out: &mut Vec<u8>) {
        let wanted = Pen {
            foreground: cell.fg,
            background: cell.bg,
            flags: cell.flags & RENDERING_FLAGS,
        };
        if wanted == *self {
            return;
        }
        *self = wanted;
        out.extend_from_slice(b"\x1b[0");
        for (flag, code) in [
            (Flags::BOLD, "1"),
            (Flags::DIM, "2"),
            (Flags::ITALIC, "3"),
            (Flags::UNDERLINE, "4"),
            (Flags::UNDERCURL, "4:3"),
            (Flags::DOTTED_UNDERLINE, "4:4"),
            (Flags::DASHED_UNDERLINE, "4:5"),
            (Flags::DOUBLE_UNDERLINE, "21"),
            (Flags::INVERSE, "7"),
            (Flags::HIDDEN, "8"),
            (Flags::STRIKEOUT, "9"),
        ] {
            if wanted.flags.contains(flag) {
                out.push(b';');
                out.extend_from_slice(code.as_bytes());
            }
        }
        for (color, foreground) in [(wanted.foreground, true), (wanted.background, false)] {
            push_color_parameters(out, color, foreground);
        }
        out.push(b'm');
    }

    fn reset(&mut self, out: &mut Vec<u8>) {
        if *self != Pen::default() {
            out.extend_from_slice(b"\x1b[0m");
            *self = Pen::default();
        }
    }
}

/// Appends `number` in decimal, without building a string for it.
fn push_number(out: &mut Vec<u8>, number: u32) {
    let mut digits = [0u8; 10];
    let mut start = digits.len();
    let mut remaining = number;
    loop {
        start -= 1;
        digits[start] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[start..]);
}

/// Appends `;parameters` selecting `color`, or nothing for the default colour,
/// which the leading reset already selects.
fn push_color_parameters(out: &mut Vec<u8>, color: Color, foreground: bool) {
    let (normal, bright, extended) = if foreground {
        (30u32, 90u32, 38u32)
    } else {
        (40, 100, 48)
    };
    match color {
        Color::Indexed(index) => {
            out.push(b';');
            push_number(out, extended);
            out.extend_from_slice(b";5;");
            push_number(out, u32::from(index));
        }
        Color::Spec(rgb) => {
            out.push(b';');
            push_number(out, extended);
            out.extend_from_slice(b";2");
            for component in [rgb.r, rgb.g, rgb.b] {
                out.push(b';');
                push_number(out, u32::from(component));
            }
        }
        Color::Named(named) => {
            let index = named as u32;
            let code = match index {
                0..=7 => Some(normal + index),
                8..=15 => Some(bright + index - 8),
                _ => dim_index(named).map(|dim| normal + dim),
            };
            if let Some(code) = code {
                out.push(b';');
                push_number(out, code);
            }
        }
    }
}

fn dim_index(named: NamedColor) -> Option<u32> {
    Some(match named {
        NamedColor::DimBlack => 0,
        NamedColor::DimRed => 1,
        NamedColor::DimGreen => 2,
        NamedColor::DimYellow => 3,
        NamedColor::DimBlue => 4,
        NamedColor::DimMagenta => 5,
        NamedColor::DimCyan => 6,
        NamedColor::DimWhite => 7,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::{
        event::VoidListener,
        grid::Dimensions,
        index::Point,
        term::Config,
        vte::ansi::{Processor, StdSyncHandler},
    };

    struct Size {
        columns: usize,
        lines: usize,
    }

    impl Dimensions for Size {
        fn total_lines(&self) -> usize {
            self.lines
        }
        fn screen_lines(&self) -> usize {
            self.lines
        }
        fn columns(&self) -> usize {
            self.columns
        }
    }

    fn term(columns: usize, lines: usize) -> Term<VoidListener> {
        Term::new(Config::default(), &Size { columns, lines }, VoidListener)
    }

    fn feed(term: &mut Term<VoidListener>, bytes: &[u8]) {
        Processor::<StdSyncHandler>::new().advance(term, bytes);
    }

    /// Everything a person looking at the screen could tell apart.
    fn visible(cell: &Cell) -> (char, Color, Color, Flags, Vec<char>) {
        let blank = is_trailing_blank(cell);
        (
            if blank { ' ' } else { cell.c },
            if blank {
                Color::Named(NamedColor::Foreground)
            } else {
                cell.fg
            },
            cell.bg,
            cell.flags & (RENDERING_FLAGS | Flags::WIDE_CHAR | Flags::WRAPLINE),
            cell.zerowidth().unwrap_or_default().to_vec(),
        )
    }

    fn assert_same_screen(original: &Term<VoidListener>, replayed: &Term<VoidListener>) {
        let original_grid = original.grid();
        let replayed_grid = replayed.grid();
        let history = original_grid
            .history_size()
            .min(REMOTE_SNAPSHOT_HISTORY_LINES);
        assert_eq!(replayed_grid.history_size(), history, "scrollback length");
        for line in -(history as i32)..original_grid.screen_lines() as i32 {
            for column in 0..original_grid.columns() {
                let expected = &original_grid[Line(line)][Column(column)];
                let actual = &replayed_grid[Line(line)][Column(column)];
                if expected.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                assert_eq!(
                    visible(actual),
                    visible(expected),
                    "cell at line {line}, column {column}"
                );
            }
        }
        assert_eq!(
            replayed_grid.cursor.point, original_grid.cursor.point,
            "cursor"
        );
        let relevant = PRIVATE_MODES.iter().fold(
            TermMode::ALT_SCREEN | TermMode::APP_KEYPAD,
            |all, (flag, _)| all | *flag,
        );
        assert_eq!(
            *replayed.mode() & relevant,
            *original.mode() & relevant,
            "modes"
        );
    }

    fn round_trip(original: &Term<VoidListener>) -> Term<VoidListener> {
        let mut replayed = term(original.columns(), original.screen_lines());
        // Dirty the target first: a snapshot has to work on a used emulator.
        feed(&mut replayed, b"stale\x1b[31m\x1b[?1h\x1b[3;3Hjunk");
        feed(
            &mut replayed,
            &render_snapshot(original, REMOTE_SNAPSHOT_HISTORY_LINES),
        );
        replayed
    }

    #[test]
    fn colours_attributes_wide_characters_and_cursor_survive_a_round_trip() {
        let mut original = term(20, 6);
        feed(
            &mut original,
            "plain \x1b[1;31mred bold\x1b[0m \x1b[48;5;200mindexed\x1b[0m\r\n\
             \x1b[38;2;10;20;30;4mtruecolor underline\x1b[0m\r\n\
             \x1b[7minverse\x1b[0m \x1b[3;9mitalic strike\x1b[0m\r\n\
             日本語 e\u{301}\r\n\
             \x1b[44m        \x1b[0m tail\x1b[5;4H"
                .as_bytes(),
        );
        assert_same_screen(&original, &round_trip(&original));
    }

    #[test]
    fn scrollback_and_wrapped_lines_survive_a_round_trip() {
        let mut original = term(10, 4);
        for number in 0..30 {
            feed(&mut original, format!("line {number}\r\n").as_bytes());
        }
        feed(&mut original, b"this one is longer than a row and wraps");
        assert_same_screen(&original, &round_trip(&original));
    }

    #[test]
    fn scrollback_is_capped() {
        let mut original = term(10, 4);
        for number in 0..(REMOTE_SNAPSHOT_HISTORY_LINES + 200) {
            feed(&mut original, format!("{number}\r\n").as_bytes());
        }
        let replayed = round_trip(&original);
        assert_eq!(
            replayed.grid().history_size(),
            REMOTE_SNAPSHOT_HISTORY_LINES
        );
        assert_same_screen(&original, &replayed);
    }

    #[test]
    fn terminal_modes_survive_a_round_trip() {
        let mut original = term(20, 5);
        feed(
            &mut original,
            b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1004h\x1b=\x1b[?25l\x1b[?7l\x1b[2 q hi",
        );
        let replayed = round_trip(&original);
        assert_same_screen(&original, &replayed);
        let mode = *replayed.mode();
        assert!(mode.contains(TermMode::APP_CURSOR | TermMode::BRACKETED_PASTE));
        assert!(mode.contains(TermMode::SGR_MOUSE | TermMode::MOUSE_REPORT_CLICK));
        assert!(!mode.contains(TermMode::SHOW_CURSOR));
        assert!(!mode.contains(TermMode::LINE_WRAP));
        assert_eq!(replayed.cursor_style(), original.cursor_style());
    }

    #[test]
    fn the_alternate_screen_is_restored_as_the_alternate_screen() {
        let mut original = term(20, 5);
        feed(
            &mut original,
            b"shell output\r\n\x1b[?1049h\x1b[2;5Hfull screen app",
        );
        let replayed = round_trip(&original);
        assert!(replayed.mode().contains(TermMode::ALT_SCREEN));
        assert_same_screen(&original, &replayed);
        assert_eq!(
            replayed.grid().cursor.point,
            Point::new(Line(1), Column(4 + "full screen app".len()))
        );
    }

    #[test]
    fn a_blank_terminal_round_trips() {
        let original = term(12, 3);
        assert_same_screen(&original, &round_trip(&original));
    }

    #[test]
    fn control_characters_in_cells_are_not_replayed_as_commands() {
        let mut original = term(10, 2);
        feed(&mut original, b"ab");
        original.grid_mut()[Line(0)][Column(1)].c = '\x1b';
        let bytes = render_snapshot(&original, REMOTE_SNAPSHOT_HISTORY_LINES);
        let body_start = 2;
        assert!(
            !bytes[body_start..]
                .windows(2)
                .any(|pair| pair == b"\x1b\x1b"),
            "a stray escape would swallow the next sequence"
        );
    }

    #[test]
    fn an_idle_slot_records_nothing_and_an_installed_one_numbers_chunks_in_order() {
        let slot = Arc::new(RemoteTapSlot::default());
        let mut tap = slot.output_tap();
        tap(b"dropped: nobody is listening");
        assert_eq!(slot.sequence(), 0);

        let installed = slot.install();
        tap(b"one");
        tap(b"two");
        assert_eq!(slot.sequence(), 2);
        let chunks: Vec<_> = std::iter::from_fn(|| installed.try_recv().ok()).collect();
        assert_eq!(
            chunks,
            vec![
                RemoteOutputChunk {
                    sequence: 1,
                    bytes: b"one".to_vec()
                },
                RemoteOutputChunk {
                    sequence: 2,
                    bytes: b"two".to_vec()
                },
            ]
        );

        slot.remove();
        tap(b"after removal");
        assert!(!slot.is_installed());
        assert_eq!(slot.sequence(), 0);
    }

    #[cfg(unix)]
    mod with_a_process {
        use crate::{
            Terminal, TerminalBuilder,
            terminal_settings::{AlternateScroll, CursorShape as ShapeSetting},
        };
        use collections::HashMap;
        use gpui::{AppContext as _, TestAppContext};
        use std::time::Duration;
        use task::Shell;
        use util::paths::PathStyle;

        async fn shell_terminal(script: &str, cx: &mut TestAppContext) -> gpui::Entity<Terminal> {
            cx.update(|cx| {
                let settings_store = settings::SettingsStore::test(cx);
                cx.set_global(settings_store);
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            });
            let builder = cx
                .update(|cx| {
                    TerminalBuilder::new(
                        None,
                        None,
                        Shell::WithArguments {
                            program: "/bin/sh".to_string(),
                            args: vec!["-c".to_string(), script.to_string()],
                            title_override: None,
                        },
                        HashMap::default(),
                        ShapeSetting::default(),
                        AlternateScroll::On,
                        None,
                        vec![],
                        0,
                        false,
                        0,
                        None,
                        cx,
                        vec![],
                        PathStyle::local(),
                    )
                })
                .await
                .expect("a shell must spawn");
            cx.new(|cx| builder.subscribe(cx))
        }

        #[gpui::test]
        async fn bytes_typed_into_the_pty_come_out_of_the_tap_in_order(cx: &mut TestAppContext) {
            cx.executor().allow_parking();
            let terminal =
                shell_terminal("read line; printf 'got:%s\\n' \"$line\"; sleep 5", cx).await;
            let tap = terminal
                .update(cx, |terminal, _| terminal.attach_remote_tap())
                .expect("a pty terminal can be tapped");
            assert!(terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));

            terminal.update(cx, |terminal, _| terminal.input(b"hi\r".to_vec()));

            let mut stream = Vec::new();
            let mut last_sequence = 0;
            for _ in 0..500 {
                while let Ok(chunk) = tap.try_recv() {
                    assert_eq!(chunk.sequence, last_sequence + 1, "no chunk may be skipped");
                    last_sequence = chunk.sequence;
                    stream.extend_from_slice(&chunk.bytes);
                }
                if String::from_utf8_lossy(&stream).contains("got:hi") {
                    break;
                }
                cx.background_executor
                    .timer(Duration::from_millis(10))
                    .await;
            }
            let text = String::from_utf8_lossy(&stream).into_owned();
            let echoed = text.find("hi").expect("the tty echoes the input");
            let answered = text.find("got:hi").expect("the script answers");
            assert!(echoed < answered, "output must keep its order: {text:?}");

            let snapshot = terminal.read_with(cx, |terminal, _| terminal.remote_snapshot());
            assert!(snapshot.sequence >= last_sequence);
            assert!(String::from_utf8_lossy(&snapshot.bytes).contains("got:hi"));

            terminal.update(cx, |terminal, _| terminal.detach_remote_tap());
            assert!(!terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));
        }

        #[gpui::test]
        async fn a_terminal_nobody_mirrors_has_no_tap(cx: &mut TestAppContext) {
            cx.executor().allow_parking();
            let terminal = shell_terminal("sleep 5", cx).await;
            assert!(!terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));
        }
    }

    #[test]
    fn a_wrap_pending_cursor_still_wraps_after_a_round_trip() {
        let mut original = term(10, 3);
        feed(&mut original, b"0123456789");
        assert!(original.grid().cursor.input_needs_wrap);
        let mut replayed = round_trip(&original);
        assert!(
            replayed.grid().cursor.input_needs_wrap,
            "the next character must wrap first, as it would have"
        );
        assert_same_screen(&original, &replayed);

        feed(&mut original, b"K");
        feed(&mut replayed, b"K");
        assert_same_screen(&original, &replayed);
        assert_eq!(replayed.grid()[Line(1)][Column(0)].c, 'K');
    }

    #[test]
    fn a_cursor_that_is_not_waiting_to_wrap_is_left_alone() {
        let mut original = term(10, 3);
        feed(&mut original, b"012345678");
        let replayed = round_trip(&original);
        assert!(!replayed.grid().cursor.input_needs_wrap);
        assert_same_screen(&original, &replayed);
    }

    #[test]
    fn the_keyboard_protocol_and_origin_mode_are_carried() {
        let kitty = || {
            Term::new(
                Config {
                    kitty_keyboard: true,
                    ..Config::default()
                },
                &Size {
                    columns: 20,
                    lines: 6,
                },
                VoidListener,
            )
        };
        let mut original = kitty();
        feed(&mut original, b"\x1b[>9u\x1b[?6h\x1b[3;4Hhi");
        let mut replayed = kitty();
        feed(&mut replayed, b"stale");
        feed(
            &mut replayed,
            &render_snapshot(&original, REMOTE_SNAPSHOT_HISTORY_LINES),
        );
        assert_eq!(
            *replayed.mode() & TermMode::KITTY_KEYBOARD_PROTOCOL,
            *original.mode() & TermMode::KITTY_KEYBOARD_PROTOCOL,
            "key encoding must match what the program asked for"
        );
        assert!(replayed.mode().contains(TermMode::DISAMBIGUATE_ESC_CODES));
        assert!(replayed.mode().contains(TermMode::REPORT_ALL_KEYS_AS_ESC));
        assert!(replayed.mode().contains(TermMode::ORIGIN));
        assert_eq!(replayed.grid().cursor.point, original.grid().cursor.point);
    }

    #[test]
    fn numbers_are_written_in_decimal_without_allocating_a_string() {
        for number in [0u32, 7, 10, 255, 4_294_967_295] {
            let mut out = Vec::new();
            push_number(&mut out, number);
            assert_eq!(String::from_utf8(out).unwrap(), number.to_string());
        }
    }

    #[test]
    fn colour_parameters_match_the_standard_codes() {
        let mut out = Vec::new();
        push_color_parameters(&mut out, Color::Named(NamedColor::Red), true);
        push_color_parameters(&mut out, Color::Named(NamedColor::BrightBlue), false);
        push_color_parameters(&mut out, Color::Named(NamedColor::DimGreen), true);
        push_color_parameters(&mut out, Color::Indexed(200), false);
        push_color_parameters(&mut out, Color::Named(NamedColor::Foreground), true);
        assert_eq!(String::from_utf8(out).unwrap(), ";31;104;32;48;5;200");
    }

    #[test]
    fn the_tap_is_bounded_by_bytes_not_only_by_chunks() {
        let slot = Arc::new(RemoteTapSlot::default());
        let mut tap = slot.output_tap();
        let installed = slot.install();
        let megabyte = vec![0u8; 1024 * 1024];
        for _ in 0..8 {
            tap(&megabyte);
        }
        assert!(installed.take_overflowed());
        assert_eq!(
            installed.len(),
            REMOTE_TAP_QUEUE_BYTES / megabyte.len(),
            "only what fits in the byte bound is kept"
        );
        assert_eq!(
            slot.sequence(),
            8,
            "dropped chunks still advance the sequence"
        );

        // Draining frees the room, so a consumer that catches up is not
        // starved for good.
        while installed.try_recv().is_ok() {}
        tap(b"after");
        assert_eq!(installed.len(), 1);
        assert!(!installed.take_overflowed());
        assert_eq!(installed.try_recv().map(|chunk| chunk.sequence), Ok(9));
    }

    #[test]
    fn a_full_queue_drops_chunks_and_says_so_without_blocking() {
        let slot = Arc::new(RemoteTapSlot::default());
        let mut tap = slot.output_tap();
        let installed = slot.install();
        for _ in 0..REMOTE_TAP_QUEUE_CAPACITY + 10 {
            tap(b"x");
        }
        assert!(installed.take_overflowed());
        assert!(!installed.take_overflowed(), "reading clears the flag");
        assert_eq!(installed.len(), REMOTE_TAP_QUEUE_CAPACITY);
        // The sequence kept counting, which is how a consumer sees the gap.
        assert_eq!(slot.sequence(), REMOTE_TAP_QUEUE_CAPACITY as u64 + 10);
    }
}
