//! Reading JSON values out of a child process as they arrive.
//!
//! Two engines stream in two shapes: `docker events --format '{{json .}}'`
//! prints one object per line, and `kubectl get --watch -o json` pretty-prints
//! each object over several. One reader handles both by parsing values out of a
//! growing buffer rather than by splitting on newlines.
//!
//! Not `serde_json::from_reader`: `clippy.toml` disallows it -- parsing straight
//! from a reader is much slower than reading into a buffer first. So the buffer
//! is explicit, and `StreamDeserializer::byte_offset` says how much of it has
//! been consumed.

use futures::AsyncReadExt as _;
use futures::channel::mpsc;
use futures::stream::{BoxStream, StreamExt as _};
use util::command::{Child, Command, Stdio};

/// A child process that dies with the stream reading it.
///
/// The whole reason this type exists rather than a bare `Child`: a `docker
/// events` process that outlives its panel is one leaked process per open, and
/// nothing later would notice. `Drop` is the only place that can be guaranteed
/// to run whether the stream ended, errored, or was simply dropped.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        // Best effort by nature: the process may already be gone, which is the
        // outcome being asked for anyway.
        //
        // Known limitation: this kills the single tracked PID (`libc::kill`),
        // not a process group. If the `docker`/`kubectl` CLI forks a helper
        // process, that grandchild is reparented to PID 1 and survives this
        // kill. Fixing that needs process-group semantics in `util::command`,
        // which is out of scope here.
        if let Err(error) = self.0.kill() {
            log::debug!("could not kill a finished event process: {error}");
        }
    }
}

/// Spawns `command` and streams the JSON values it prints.
///
/// Values that will not parse are skipped rather than ending the stream: one
/// malformed event must not stop the panel hearing about the next one. A value
/// cut off mid-way is not malformed -- it is incomplete, and waits for more.
pub(crate) fn json_values<T>(mut command: Command) -> Option<BoxStream<'static, T>>
where
    T: serde::de::DeserializeOwned + Send + 'static,
{
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
    // Belt-and-braces: `Reaped` below already kills the child on drop, but that
    // guard only exists once `spawn` returns. Setting this here means even a
    // future refactor that moves work between `spawn` and the guard's
    // construction cannot reintroduce a silent leak.
    command.kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            log::warn!("could not start an event stream: {error}");
            return None;
        }
    };
    let stdout = child.stdout.take()?;
    let reaped = Reaped(child);

    // A channel rather than `stream::unfold` so the reading future owns the
    // buffer and the `Reaped` guard outright; when the receiver is dropped the
    // send fails, the loop ends, and the guard runs.
    let (sender, receiver) = mpsc::unbounded::<T>();

    let pump = async move {
        // Held for the whole read so the process is killed when this future is
        // dropped, which is what happens when the receiver goes.
        let _reaped = reaped;
        let mut stdout = stdout;
        let mut buffer = String::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = match stdout.read(&mut chunk).await {
                Ok(0) => return,
                Ok(read) => read,
                Err(error) => {
                    log::debug!("event stream ended: {error}");
                    return;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk[..read]));

            let mut consumed = 0;
            {
                let mut values = serde_json::Deserializer::from_str(&buffer).into_iter::<T>();
                while let Some(result) = values.next() {
                    match result {
                        Ok(value) => {
                            consumed = values.byte_offset();
                            if sender.unbounded_send(value).is_err() {
                                return;
                            }
                        }
                        // Cut off mid-value: not an error, just not all here yet.
                        Err(error) if error.is_eof() => break,
                        Err(error) => {
                            log::warn!("skipping an unreadable event: {error}");
                            // Everything up to here is unusable. Dropping the
                            // whole buffer risks eating a good value, but keeping
                            // it means re-reading the same bad bytes forever.
                            consumed = buffer.len();
                            break;
                        }
                    }
                }
            }
            buffer.drain(..consumed);
        }
    };

    // Driven by whoever polls the stream: the pump future is folded into it, so
    // no executor handle is needed here and nothing runs until somebody reads.
    let stream = futures::stream::select(
        receiver,
        futures::stream::once(pump).filter_map(|()| async { None }),
    );
    Some(stream.boxed())
}
