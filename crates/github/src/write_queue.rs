use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
use gpui::{App, BackgroundExecutor, Global};

use crate::error::GithubResult;
use crate::token::TokenIdentity;

/// GitHub asks for at least a second between writes from one account.
pub const MIN_WRITE_INTERVAL: Duration = Duration::from_secs(1);

/// The pause after a write was rate limited without a `retry-after`.
pub const RATE_LIMITED_PAUSE: Duration = Duration::from_secs(60);

/// The longest the queue holds writes back, whatever GitHub asks: a
/// `retry-after` of a day would otherwise freeze every write of the token
/// with nothing on screen. The write that was told to wait reports the full
/// wait in its error.
pub const MAX_WRITE_PAUSE: Duration = Duration::from_secs(300);

/// What a write produced, plus how long GitHub asked to wait before the next.
pub struct WriteOutcome<T> {
    pub result: GithubResult<T>,
    pub retry_after: Option<Duration>,
}

/// Runs one token's writes one at a time, in the order they were queued,
/// spaced out as GitHub asks. Shared by every window, so two workspaces on one
/// token cannot double the pace.
pub struct WriteQueue {
    executor: BackgroundExecutor,
    /// Settles when the latest queued write is done or dropped.
    last_write: Mutex<Option<Shared<oneshot::Receiver<()>>>>,
    next_write_at: Mutex<Option<Instant>>,
}

/// A place in the queue, taken when the write is asked for so writes run in
/// that order whatever order their futures are first polled in.
pub struct WriteTicket {
    previous: Option<Shared<oneshot::Receiver<()>>>,
    done: oneshot::Sender<()>,
}

impl WriteQueue {
    pub fn new(executor: BackgroundExecutor) -> Self {
        Self {
            executor,
            last_write: Mutex::new(None),
            next_write_at: Mutex::new(None),
        }
    }

    pub fn ticket(&self) -> WriteTicket {
        let (done, finished) = oneshot::channel();
        let previous = self
            .last_write
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(finished.shared());
        WriteTicket { previous, done }
    }

    /// Waits for the ticket's turn, runs the write, and holds the next one
    /// back for at least [`MIN_WRITE_INTERVAL`] or the `retry-after` GitHub
    /// sent, up to [`MAX_WRITE_PAUSE`]. Dropping the future gives the turn to
    /// the next write.
    pub async fn run<T>(
        &self,
        ticket: WriteTicket,
        write: impl Future<Output = WriteOutcome<T>>,
    ) -> GithubResult<T> {
        let WriteTicket { previous, done } = ticket;
        if let Some(previous) = previous {
            // Ok when the earlier write finished, Canceled when it was
            // dropped; either way the turn is ours.
            previous.await.ok();
        }
        let next_write_at = *self
            .next_write_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = next_write_at {
            let now = self.executor.now();
            if at > now {
                self.executor.timer(at - now).await;
            }
        }
        // Spaced from the start as well: if this future is dropped mid-write,
        // the request may still have reached GitHub.
        *self
            .next_write_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner) =
            Some(self.executor.now() + MIN_WRITE_INTERVAL);
        let outcome = write.await;
        let pause = outcome.retry_after.map_or(MIN_WRITE_INTERVAL, |wait| {
            wait.clamp(MIN_WRITE_INTERVAL, MAX_WRITE_PAUSE)
        });
        *self
            .next_write_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(self.executor.now() + pause);
        // The receiver may be gone when no write was queued after this one.
        done.send(()).ok();
        outcome.result
    }
}

/// The write queue of every token in use, held weakly so a token nobody uses
/// any more does not keep its queue alive.
#[derive(Default)]
pub struct WriteQueues {
    queues: HashMap<TokenIdentity, Weak<WriteQueue>>,
}

impl Global for WriteQueues {}

impl WriteQueues {
    pub fn queue_for(identity: &TokenIdentity, cx: &mut App) -> Arc<WriteQueue> {
        let executor = cx.background_executor().clone();
        let queues = &mut cx.default_global::<Self>().queues;
        if let Some(queue) = queues.get(identity).and_then(Weak::upgrade) {
            return queue;
        }
        queues.retain(|_, queue| queue.strong_count() > 0);
        let queue = Arc::new(WriteQueue::new(executor));
        queues.insert(identity.clone(), Arc::downgrade(&queue));
        queue
    }
}
