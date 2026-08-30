use crate::protocol::{
    ErrorCode, InitializeResult, Method, PROTOCOL_VERSION, Request, RequestId, Response,
    ResponseError, version_is_compatible,
};
use crate::transport::Transport;
use anyhow::{Context as _, Result};
use collections::HashMap;
use futures::channel::oneshot;
use futures::{FutureExt as _, StreamExt as _, select_biased};
use gpui::{AppContext as _, AsyncApp, BackgroundExecutor, Task};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// How long a single call may take before the driver is treated as gone.
///
/// Generous, because the caller's own answer to a slow query is `cancel` rather
/// than this: a timeout that fires before a legitimate query finishes would
/// make large tables unbrowsable. This is the backstop for a driver that has
/// stopped answering at all.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A driver's own refusal, carried whole so callers can tell a read-only
/// rejection from a syntax error without reading English.
#[derive(Clone, Debug)]
pub struct DriverError(pub ResponseError);

impl DriverError {
    pub fn code(&self) -> ErrorCode {
        self.0.code
    }
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.message)?;
        // A detail that only repeats the message is not a second fact, and
        // printing it made a connection failure read as
        // `error connecting to server (error connecting to server)` -- which
        // looks like the driver stuttering rather than like an error saying
        // everything it knows. Drivers fall back to the engine's own words for
        // `message` whenever there is no shorter line to give, so the two being
        // equal is ordinary rather than a driver bug to fix at each call site.
        if let Some(detail) = self.0.detail.as_deref().filter(|detail| *detail != self.0.message) {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for DriverError {}

/// Talks to one driver.
///
/// Every call is typed by its `Method`, so a driver and this client cannot
/// disagree about a payload without the compiler saying so on our side.
pub struct DriverClient {
    transport: Arc<dyn Transport>,
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Response>>>>,
    next_id: AtomicU64,
    timeout: Duration,
    executor: BackgroundExecutor,
    _pumps: Vec<Task<()>>,
}

impl DriverClient {
    pub fn new(transport: Arc<dyn Transport>, timeout: Duration, cx: &AsyncApp) -> Self {
        let pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Response>>>> =
            Arc::new(Mutex::new(HashMap::default()));

        let responses = cx.background_spawn({
            let pending = pending.clone();
            let mut incoming = transport.receive();
            async move {
                while let Some(line) = incoming.next().await {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Response>(line) {
                        Ok(response) => {
                            if let Some(waiting) = pending.lock().remove(&response.id) {
                                // The receiver is gone when the caller timed
                                // out or was dropped; that is not an error, the
                                // answer simply has nowhere left to go.
                                waiting.send(response).ok();
                            } else {
                                log::warn!("database driver answered an unknown request id");
                            }
                        }
                        // Not fatal: the next line may well parse. A driver
                        // that writes rubbish to stdout is diagnosed by the
                        // caller timing out, with this in the log to say why.
                        Err(error) => log::error!("undecodable line from database driver: {error}"),
                    }
                }
                // The pipe closed, so nothing still waiting will ever be
                // answered. Dropping the senders wakes every caller at once
                // with a broken channel rather than leaving them to time out
                // one by one.
                pending.lock().clear();
            }
        });

        let errors = cx.background_spawn({
            let mut incoming = transport.receive_err();
            async move {
                while let Some(line) = incoming.next().await {
                    let line = line.trim();
                    if !line.is_empty() {
                        log::warn!("database driver: {line}");
                    }
                }
            }
        });

        Self {
            transport,
            pending,
            next_id: AtomicU64::new(1),
            timeout,
            executor: cx.background_executor().clone(),
            _pumps: vec![responses, errors],
        }
    }

    pub async fn request<M: Method>(&self, params: M::Params) -> Result<M::Result> {
        let id = RequestId::Number(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().insert(id.clone(), sender);

        let request = Request::new(id.clone(), M::NAME, serde_json::to_value(params)?);
        let encoded = serde_json::to_string(&request)?;
        if let Err(error) = self.transport.send(encoded).await {
            self.pending.lock().remove(&id);
            return Err(error).with_context(|| format!("sending `{}` to the driver", M::NAME));
        }

        let response = select_biased! {
            response = receiver.fuse() => response,
            _ = self.executor.timer(self.timeout).fuse() => {
                self.pending.lock().remove(&id);
                anyhow::bail!(
                    "the database driver did not answer `{}` within {:?}",
                    M::NAME,
                    self.timeout
                );
            }
        };
        let response = response.with_context(|| {
            format!("the database driver stopped before answering `{}`", M::NAME)
        })?;

        if let Some(error) = response.error {
            return Err(DriverError(error).into());
        }
        let result = response
            .result
            .with_context(|| format!("`{}` answered with neither result nor error", M::NAME))?;
        serde_json::from_value(result)
            .with_context(|| format!("decoding the answer to `{}`", M::NAME))
    }

    /// The handshake, and the only place a version is checked.
    ///
    /// Refused outright on a mismatch rather than worked around: a driver
    /// speaking a different shape will fail later anyway, and it will fail
    /// somewhere far less obvious than here.
    pub async fn initialize(&self) -> Result<InitializeResult> {
        let result = self
            .request::<crate::protocol::Initialize>(crate::protocol::NoParams {})
            .await?;
        anyhow::ensure!(
            version_is_compatible(PROTOCOL_VERSION, result.protocol_version),
            "driver `{}` speaks database protocol version {}, but this build of Zode speaks {}",
            result.driver_name,
            result.protocol_version,
            PROTOCOL_VERSION,
        );
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ErrorCode, ResponseError};

    /// What a connection failure actually looked like: the driver has no
    /// shorter line to offer than the engine's own words, so `message` and
    /// `detail` are the same sentence -- and printing both read as the driver
    /// stuttering rather than as an error saying everything it knows.
    #[test]
    fn a_detail_that_only_repeats_the_message_is_not_printed_twice() {
        let error = DriverError(
            ResponseError::new(ErrorCode::Connection, "error connecting to server")
                .with_detail("error connecting to server"),
        );
        assert_eq!(error.to_string(), "error connecting to server");
    }

    #[test]
    fn a_detail_that_adds_something_is_still_printed() {
        let error = DriverError(
            ResponseError::new(ErrorCode::Connection, "could not reach db.example:5432")
                .with_detail("error connecting to server: Network is unreachable (os error 51)"),
        );
        assert_eq!(
            error.to_string(),
            "could not reach db.example:5432 \
             (error connecting to server: Network is unreachable (os error 51))"
        );
    }
}
