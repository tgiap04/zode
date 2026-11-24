use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gpui::{AppContext as _, TestAppContext};

use crate::error::GithubError;
use crate::token::{GithubToken, TokenSource};
use crate::write_queue::{MIN_WRITE_INTERVAL, WriteOutcome, WriteQueue, WriteQueues};

type Starts = Arc<Mutex<Vec<Instant>>>;

/// Queues a write that notes when it started and asks for `retry_after`.
fn queue_write(
    queue: &Arc<WriteQueue>,
    starts: &Starts,
    retry_after: Option<Duration>,
    cx: &mut TestAppContext,
) -> gpui::Task<Result<(), GithubError>> {
    let queue = queue.clone();
    let ticket = queue.ticket();
    let starts = starts.clone();
    let executor = cx.executor();
    cx.background_spawn(async move {
        queue
            .run(ticket, async {
                starts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(executor.now());
                WriteOutcome {
                    result: Ok(()),
                    retry_after,
                }
            })
            .await
    })
}

fn started(starts: &Starts) -> Vec<Instant> {
    starts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[gpui::test]
async fn writes_are_at_least_a_second_apart(cx: &mut TestAppContext) {
    let queue = Arc::new(WriteQueue::new(cx.executor()));
    let starts = Starts::default();
    let first = queue_write(&queue, &starts, None, cx);
    let second = queue_write(&queue, &starts, None, cx);
    let third = queue_write(&queue, &starts, None, cx);
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 1);

    cx.executor()
        .advance_clock(MIN_WRITE_INTERVAL - Duration::from_millis(1));
    cx.executor().run_until_parked();
    assert_eq!(
        started(&starts).len(),
        1,
        "the second write waits a full second"
    );

    cx.executor().advance_clock(Duration::from_millis(1));
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 2);

    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.executor().run_until_parked();
    let starts = started(&starts);
    assert_eq!(starts.len(), 3);
    assert_eq!(starts[1] - starts[0], MIN_WRITE_INTERVAL);
    assert_eq!(starts[2] - starts[1], MIN_WRITE_INTERVAL);
    for task in [first, second, third] {
        task.await.expect("each write succeeds");
    }
}

#[gpui::test]
async fn a_retry_after_holds_back_the_next_write(cx: &mut TestAppContext) {
    let queue = Arc::new(WriteQueue::new(cx.executor()));
    let starts = Starts::default();
    let _first = queue_write(&queue, &starts, Some(Duration::from_secs(30)), cx);
    let _second = queue_write(&queue, &starts, None, cx);
    cx.executor().run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(29));
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 1, "GitHub asked for 30 seconds");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.executor().run_until_parked();
    let starts = started(&starts);
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[1] - starts[0], Duration::from_secs(30));
}

#[gpui::test]
async fn a_write_after_a_quiet_spell_runs_at_once(cx: &mut TestAppContext) {
    let queue = Arc::new(WriteQueue::new(cx.executor()));
    let starts = Starts::default();
    let _first = queue_write(&queue, &starts, None, cx);
    cx.executor().run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(5));
    let _second = queue_write(&queue, &starts, None, cx);
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 2);
}

#[gpui::test]
async fn a_dropped_write_gives_its_turn_to_the_next(cx: &mut TestAppContext) {
    let queue = Arc::new(WriteQueue::new(cx.executor()));
    let abandoned = queue.ticket();
    drop(abandoned);
    let starts = Starts::default();
    let _write = queue_write(&queue, &starts, None, cx);
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 1);
}

#[gpui::test]
fn one_queue_is_shared_per_token(cx: &mut TestAppContext) {
    let token = GithubToken::new("first", TokenSource::Keychain);
    let same = GithubToken::new("first", TokenSource::Keychain);
    let other = GithubToken::new("second", TokenSource::GhCli);
    cx.update(|cx| {
        let queue = WriteQueues::queue_for(token.identity(), cx);
        let again = WriteQueues::queue_for(same.identity(), cx);
        let different = WriteQueues::queue_for(other.identity(), cx);
        assert!(Arc::ptr_eq(&queue, &again));
        assert!(!Arc::ptr_eq(&queue, &different));
    });
}

#[gpui::test]
async fn a_write_dropped_in_flight_still_spaces_the_next(cx: &mut TestAppContext) {
    let queue = Arc::new(WriteQueue::new(cx.executor()));
    let starts = Starts::default();
    let in_flight = {
        let queue = queue.clone();
        let ticket = queue.ticket();
        let starts = starts.clone();
        let executor = cx.executor();
        cx.background_spawn(async move {
            queue
                .run(ticket, async {
                    starts
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(executor.now());
                    futures::future::pending::<WriteOutcome<()>>().await
                })
                .await
        })
    };
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 1);
    drop(in_flight);
    let _next = queue_write(&queue, &starts, None, cx);
    cx.executor().run_until_parked();
    assert_eq!(
        started(&starts).len(),
        1,
        "the dropped write may have reached GitHub"
    );
    cx.executor().advance_clock(MIN_WRITE_INTERVAL);
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 2);
}

#[gpui::test]
async fn a_huge_retry_after_is_capped_at_five_minutes(cx: &mut TestAppContext) {
    let queue = Arc::new(WriteQueue::new(cx.executor()));
    let starts = Starts::default();
    let _first = queue_write(&queue, &starts, Some(Duration::from_secs(86_400)), cx);
    let _second = queue_write(&queue, &starts, None, cx);
    cx.executor().run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(299));
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 1);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.executor().run_until_parked();
    assert_eq!(started(&starts).len(), 2);
}
