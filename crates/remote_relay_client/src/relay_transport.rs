//! The socket under a relay connection, behind a trait so everything above it
//! can be driven in-process.

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use async_tungstenite::{
    WebSocketStream,
    tokio::{ConnectStream, connect_async_with_tls_connector_and_config},
    tungstenite::{
        Message,
        client::IntoClientRequest as _,
        http::{HeaderValue, header::AUTHORIZATION},
        protocol::WebSocketConfig,
    },
};
use futures::StreamExt as _;
use gpui::{AppContext as _, AsyncApp, Task};
use gpui_tokio::Tokio;
use remote_relay_protocol::{MAX_RELAY_PAYLOAD_LEN, RELAY_HEADER_LEN};
use smol::channel::{self, Receiver, Sender};
use tokio_rustls::TlsConnector;

/// The subprotocol the relay speaks. Offered on connect so a relay that has
/// moved on can refuse the upgrade instead of answering in a dialect this
/// build would misread.
pub const RELAY_SUBPROTOCOL: &str = "zode-relay.v1";

/// Frames that may wait between the socket and the entity before the socket
/// is made to wait.
pub const INBOUND_QUEUE_CAPACITY: usize = 512;

/// Frames that may wait to be written. Full means the relay is not keeping up,
/// and the sender must hold back rather than queue without bound.
pub const OUTBOUND_QUEUE_CAPACITY: usize = 512;

/// The relay pings every 30 seconds and drops a peer that misses two. Hearing
/// nothing for longer than that means this end of the socket is dead even if
/// the OS has not noticed.
const SILENCE_LIMIT: Duration = Duration::from_secs(75);

/// A write that has not finished by now means the relay has stopped reading:
/// waiting longer would park the pump, and with it everything queued behind.
const WRITE_LIMIT: Duration = Duration::from_secs(15);

/// The most a refusal body is read for: it carries one short error code.
const MAX_REFUSAL_BODY_BYTES: usize = 1024;

/// How long the pump waits on a quiet or stalled socket before giving up.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PumpLimits {
    pub silence: Duration,
    pub write: Duration,
}

impl Default for PumpLimits {
    fn default() -> Self {
        Self {
            silence: SILENCE_LIMIT,
            write: WRITE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireInbound {
    Text(String),
    Binary(Vec<u8>),
    /// The socket ended. `code` is the WebSocket close code when the relay
    /// sent one; `None` for a socket that simply died.
    Closed {
        code: Option<u16>,
        /// The close reason text, empty when none was sent.
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireOutbound {
    Text(String),
    Binary(Vec<u8>),
    /// Close the socket cleanly.
    Close,
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The relay refused the credential or the device. Retrying with the same
    /// credential will not change the answer.
    #[error("the relay refused the connection with status {status}")]
    Refused {
        status: u16,
        /// The relay's short error code from the refusal body, when it sent
        /// one in the expected shape.
        reason: Option<String>,
    },
    #[error("could not reach the relay: {0}")]
    Unreachable(String),
}

/// A live connection: frames in, frames out, and the task that moves them.
/// Dropping it closes the socket.
pub struct RelayLink {
    pub inbound: Receiver<WireInbound>,
    pub outbound: Sender<WireOutbound>,
    pub _pump: Task<()>,
}

pub trait RelayTransport: 'static {
    fn connect(
        &self,
        url: String,
        bearer: String,
        cx: &AsyncApp,
    ) -> Task<Result<RelayLink, TransportError>>;
}

/// Opens real WebSockets, on the shared Tokio runtime.
pub struct WebSocketTransport;

impl RelayTransport for WebSocketTransport {
    fn connect(
        &self,
        url: String,
        bearer: String,
        cx: &AsyncApp,
    ) -> Task<Result<RelayLink, TransportError>> {
        let (inbound_sender, inbound) = channel::bounded(INBOUND_QUEUE_CAPACITY);
        let (outbound, outbound_receiver) = channel::bounded(OUTBOUND_QUEUE_CAPACITY);
        let (ready_sender, ready) = channel::bounded(1);

        let socket_task = Tokio::spawn(cx, async move {
            match connect_websocket(&url, &bearer).await {
                Ok(socket) => {
                    if ready_sender.send(Ok(())).await.is_err() {
                        return;
                    }
                    pump_socket(
                        socket,
                        inbound_sender,
                        outbound_receiver,
                        PumpLimits::default(),
                    )
                    .await;
                }
                Err(error) => {
                    if ready_sender.send(Err(error)).await.is_err() {
                        log::debug!("the relay connection attempt was abandoned");
                    }
                }
            }
        });
        // The tokio task's own result only says whether it was cancelled or
        // panicked; both end the link, which the inbound channel reports.
        let pump = cx.background_spawn(async move {
            if let Err(error) = socket_task.await {
                log::warn!("the relay socket task ended abnormally: {error}");
            }
        });

        cx.background_spawn(async move {
            match ready.recv().await {
                Ok(Ok(())) => Ok(RelayLink {
                    inbound,
                    outbound,
                    _pump: pump,
                }),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(TransportError::Unreachable(
                    "the connection task ended before it connected".into(),
                )),
            }
        })
    }
}

pub(crate) async fn connect_websocket(
    url: &str,
    bearer: &str,
) -> Result<WebSocketStream<ConnectStream>, TransportError> {
    let mut request = url
        .into_client_request()
        .context("the relay address is not a valid WebSocket request")
        .map_err(|error| TransportError::Unreachable(error.to_string()))?;
    let mut authorization = HeaderValue::from_str(&format!("Bearer {bearer}")).map_err(|_| {
        TransportError::Unreachable("the access token is not a valid header".into())
    })?;
    // Keeps the token out of any header dump the HTTP layer logs.
    authorization.set_sensitive(true);
    request.headers_mut().insert(AUTHORIZATION, authorization);
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static(RELAY_SUBPROTOCOL),
    );

    let connector = TlsConnector::from(Arc::new(http_client_tls::tls_config()));
    // The relay is untrusted, so what it may make this process buffer is
    // bounded by what a legitimate frame can be.
    let largest_message = MAX_RELAY_PAYLOAD_LEN + RELAY_HEADER_LEN;
    let config = WebSocketConfig::default()
        .max_message_size(Some(largest_message))
        .max_frame_size(Some(largest_message));
    match connect_async_with_tls_connector_and_config(request, Some(connector), Some(config)).await
    {
        Ok((socket, _response)) => Ok(socket),
        Err(async_tungstenite::tungstenite::Error::Http(response)) => {
            Err(TransportError::Refused {
                status: response.status().as_u16(),
                reason: refusal_reason(response.body().as_deref()),
            })
        }
        Err(error) => Err(TransportError::Unreachable(error.to_string())),
    }
}

/// The `error` code in a refusal body such as `{"error":"quota_exceeded"}`.
/// Only short lowercase codes are accepted, because the text comes from a
/// party that is not trusted and ends up in logs.
fn refusal_reason(body: Option<&[u8]>) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Refusal {
        error: String,
    }
    let body = body.filter(|body| body.len() <= MAX_REFUSAL_BODY_BYTES)?;
    let Refusal { error } = serde_json::from_slice(body).ok()?;
    let well_formed = !error.is_empty()
        && error.len() <= 64
        && error
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_');
    well_formed.then_some(error)
}

/// Moves frames between the socket and the two queues until either side ends,
/// then reports how it ended.
pub(crate) async fn pump_socket(
    mut socket: WebSocketStream<ConnectStream>,
    inbound: Sender<WireInbound>,
    outbound: Receiver<WireOutbound>,
    limits: PumpLimits,
) {
    // Only what the relay sends counts as hearing from it: a deadline re-armed
    // by this end's own writes would never fire on a relay that went quiet
    // while a terminal streams output.
    let mut last_heard = tokio::time::Instant::now();
    let (code, reason) = loop {
        tokio::select! {
            outgoing = outbound.recv() => {
                let message = match outgoing {
                    Ok(WireOutbound::Text(text)) => Message::text(text),
                    Ok(WireOutbound::Binary(bytes)) => Message::binary(bytes),
                    Ok(WireOutbound::Close) | Err(_) => {
                        match tokio::time::timeout(limits.write, socket.close(None)).await {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                log::debug!("closing the relay socket failed: {error}");
                            }
                            Err(_) => log::debug!("closing the relay socket timed out"),
                        }
                        break (None, String::new());
                    }
                };
                match tokio::time::timeout(limits.write, socket.send(message)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        log::warn!("writing to the relay failed: {error}");
                        break (None, String::new());
                    }
                    Err(_) => {
                        log::warn!("the relay stopped reading; giving up on the connection");
                        break (None, String::new());
                    }
                }
            }
            _ = tokio::time::sleep_until(last_heard + limits.silence) => {
                log::warn!("the relay has been silent too long");
                break (None, String::new());
            }
            incoming = socket.next() => {
                last_heard = tokio::time::Instant::now();
                let message = match incoming {
                    None => break (None, String::new()),
                    Some(Err(error)) => {
                        log::warn!("reading from the relay failed: {error}");
                        break (None, String::new());
                    }
                    Some(Ok(message)) => message,
                };
                let forwarded = match message {
                    Message::Text(text) => WireInbound::Text(text.as_str().to_owned()),
                    Message::Binary(bytes) => WireInbound::Binary(bytes.to_vec()),
                    Message::Close(frame) => {
                        break match frame {
                            Some(frame) => (
                                Some(u16::from(frame.code)),
                                frame.reason.as_str().to_owned(),
                            ),
                            None => (None, String::new()),
                        };
                    }
                    // Pongs are answered by the protocol layer, and every
                    // frame counted as hearing from the relay above.
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                };
                if inbound.send(forwarded).await.is_err() {
                    break (None, String::new());
                }
            }
        }
    };
    if inbound
        .send(WireInbound::Closed { code, reason })
        .await
        .is_err()
    {
        log::debug!("the relay connection ended with nobody listening");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_tungstenite::tokio::accept_hdr_async;
    use async_tungstenite::tungstenite::{
        handshake::server::{Request, Response},
        protocol::{CloseFrame, frame::coding::CloseCode},
    };
    use tokio::net::TcpListener;

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(future)
    }

    /// A relay stand-in: records the upgrade headers, echoes what it is sent,
    /// then closes with the code it was configured with.
    // The handshake callback's error type is tungstenite's and large; it is
    // never constructed here.
    #[allow(clippy::result_large_err)]
    async fn serve_once(
        listener: TcpListener,
        close_code: u16,
        close_reason: &'static str,
    ) -> (Option<String>, Option<String>) {
        let (stream, _) = listener.accept().await.expect("a connection");
        let mut seen = (None, None);
        let mut socket = accept_hdr_async(stream, |request: &Request, mut response: Response| {
            seen.0 = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            seen.1 = request
                .headers()
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            response.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                HeaderValue::from_static(RELAY_SUBPROTOCOL),
            );
            Ok(response)
        })
        .await
        .expect("the upgrade");
        while let Some(Ok(message)) = socket.next().await {
            match &message {
                Message::Text(text) if text.as_str() == "bye" => {
                    socket.send(message).await.expect("echo");
                    socket
                        .close(Some(CloseFrame {
                            code: CloseCode::from(close_code),
                            reason: close_reason.into(),
                        }))
                        .await
                        .expect("close");
                    return seen;
                }
                Message::Text(_) | Message::Binary(_) => {
                    socket.send(message).await.expect("echo");
                }
                Message::Close(_) => return seen,
                _ => {}
            }
        }
        seen
    }

    #[test]
    fn frames_flow_both_ways_with_the_token_and_subprotocol_on_the_upgrade() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            let server = tokio::spawn(serve_once(listener, 4403, "device_revoked"));

            let socket = connect_websocket(&format!("ws://{address}/api/relay"), "token-value")
                .await
                .expect("connect");
            let (inbound_sender, inbound) = channel::bounded(8);
            let (outbound, outbound_receiver) = channel::bounded(8);
            let pump = tokio::spawn(pump_socket(
                socket,
                inbound_sender,
                outbound_receiver,
                PumpLimits::default(),
            ));

            outbound
                .send(WireOutbound::Text("ping".into()))
                .await
                .unwrap();
            assert_eq!(
                inbound.recv().await.unwrap(),
                WireInbound::Text("ping".into())
            );
            outbound
                .send(WireOutbound::Binary(vec![0, 0, 0, 1, 9]))
                .await
                .unwrap();
            assert_eq!(
                inbound.recv().await.unwrap(),
                WireInbound::Binary(vec![0, 0, 0, 1, 9])
            );

            // The server answers "bye" by closing with its configured code, and
            // that code is what the owner of the link must be told.
            outbound
                .send(WireOutbound::Text("bye".into()))
                .await
                .unwrap();
            assert_eq!(
                inbound.recv().await.unwrap(),
                WireInbound::Text("bye".into())
            );
            assert_eq!(
                inbound.recv().await.unwrap(),
                WireInbound::Closed {
                    code: Some(4403),
                    reason: "device_revoked".into()
                }
            );
            pump.await.expect("pump ends");

            let (authorization, subprotocol) = server.await.expect("server");
            assert_eq!(authorization.as_deref(), Some("Bearer token-value"));
            assert_eq!(subprotocol.as_deref(), Some(RELAY_SUBPROTOCOL));
        });
    }

    #[test]
    fn closing_from_this_side_ends_the_pump() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            let server = tokio::spawn(serve_once(listener, 1000, ""));
            let socket = connect_websocket(&format!("ws://{address}/api/relay"), "t")
                .await
                .expect("connect");
            let (inbound_sender, inbound) = channel::bounded(8);
            let (outbound, outbound_receiver) = channel::bounded(8);
            let pump = tokio::spawn(pump_socket(
                socket,
                inbound_sender,
                outbound_receiver,
                PumpLimits::default(),
            ));

            outbound.send(WireOutbound::Close).await.unwrap();
            assert_eq!(
                inbound.recv().await.unwrap(),
                WireInbound::Closed {
                    code: None,
                    reason: String::new()
                }
            );
            pump.await.expect("pump ends");
            server.await.expect("server");
        });
    }

    #[test]
    fn an_unreachable_relay_is_reported_not_panicked_on() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            drop(listener);
            let error = connect_websocket(&format!("ws://{address}/api/relay"), "t")
                .await
                .err()
                .expect("nothing is listening");
            assert!(matches!(error, TransportError::Unreachable(_)), "{error}");
        });
    }

    #[test]
    fn a_refusal_carries_the_status() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt as _;
                let (mut stream, _) = listener.accept().await.expect("accept");
                stream
                    .write_all(
                        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .expect("respond");
            });
            let error = connect_websocket(&format!("ws://{address}/api/relay"), "t")
                .await
                .err()
                .expect("refused");
            assert!(
                matches!(
                    error,
                    TransportError::Refused {
                        status: 403,
                        reason: None
                    }
                ),
                "{error}"
            );
        });
    }

    #[test]
    fn a_refusal_carries_the_relays_error_code() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt as _;
                let (mut stream, _) = listener.accept().await.expect("accept");
                let body = br#"{"error":"quota_exceeded"}"#;
                let head = format!(
                    "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                // One write, as the relay does: the body is only seen when it
                // arrives with the headers.
                let mut response = head.into_bytes();
                response.extend_from_slice(body);
                stream.write_all(&response).await.expect("respond");
            });
            let error = connect_websocket(&format!("ws://{address}/api/relay"), "t")
                .await
                .err()
                .expect("refused");
            assert!(
                matches!(
                    &error,
                    TransportError::Refused { status: 429, reason: Some(reason) }
                        if reason == "quota_exceeded"
                ),
                "{error}"
            );
        });
    }

    #[test]
    fn only_short_lowercase_codes_are_taken_from_a_refusal_body() {
        assert_eq!(
            refusal_reason(Some(br#"{"error":"rate_limited"}"#)).as_deref(),
            Some("rate_limited")
        );
        assert_eq!(refusal_reason(Some(br#"{"error":"Bad Value!"}"#)), None);
        assert_eq!(refusal_reason(Some(b"<html>nope</html>")), None);
        assert_eq!(refusal_reason(Some(br#"{"error":""}"#)), None);
        assert_eq!(refusal_reason(None), None);
        let long = format!(r#"{{"error":"{}"}}"#, "a".repeat(100));
        assert_eq!(refusal_reason(Some(long.as_bytes())), None);
    }

    /// Accepts one upgrade and hands back the server end of the socket.
    #[allow(clippy::result_large_err)]
    async fn accept_socket(
        listener: TcpListener,
    ) -> WebSocketStream<async_tungstenite::tokio::TokioAdapter<tokio::net::TcpStream>> {
        let (stream, _) = listener.accept().await.expect("a connection");
        accept_hdr_async(stream, |_: &Request, mut response: Response| {
            response.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                HeaderValue::from_static(RELAY_SUBPROTOCOL),
            );
            Ok(response)
        })
        .await
        .expect("the upgrade")
    }

    #[test]
    fn a_relay_that_goes_quiet_is_noticed_even_while_this_end_keeps_writing() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            // Reads everything and says nothing.
            tokio::spawn(async move {
                let mut socket = accept_socket(listener).await;
                while let Some(Ok(_)) = socket.next().await {}
            });
            let socket = connect_websocket(&format!("ws://{address}/api/relay"), "t")
                .await
                .expect("connect");
            let (inbound_sender, inbound) = channel::bounded(8);
            let (outbound, outbound_receiver) = channel::bounded(8);
            let limits = PumpLimits {
                silence: Duration::from_millis(400),
                write: Duration::from_secs(5),
            };
            let pump = tokio::spawn(pump_socket(
                socket,
                inbound_sender,
                outbound_receiver,
                limits,
            ));
            let writer = tokio::spawn(async move {
                while outbound
                    .send(WireOutbound::Binary(vec![0, 0, 0, 1, 7]))
                    .await
                    .is_ok()
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            });

            let closed = tokio::time::timeout(Duration::from_secs(5), inbound.recv())
                .await
                .expect("the silence must be noticed despite the writes")
                .expect("a close report");
            assert_eq!(
                closed,
                WireInbound::Closed {
                    code: None,
                    reason: String::new()
                }
            );
            pump.await.expect("pump ends");
            writer.abort();
        });
    }

    #[test]
    fn a_relay_that_stops_reading_does_not_park_the_pump() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            // Holds the socket open and never reads from it.
            let (release, held) = channel::bounded::<()>(1);
            tokio::spawn(async move {
                let _socket = accept_socket(listener).await;
                held.recv().await.ok();
            });
            let socket = connect_websocket(&format!("ws://{address}/api/relay"), "t")
                .await
                .expect("connect");
            let (inbound_sender, inbound) = channel::bounded(8);
            let (outbound, outbound_receiver) = channel::bounded(8);
            let limits = PumpLimits {
                silence: Duration::from_secs(60),
                write: Duration::from_millis(400),
            };
            let pump = tokio::spawn(pump_socket(
                socket,
                inbound_sender,
                outbound_receiver,
                limits,
            ));
            let writer = tokio::spawn(async move {
                while outbound
                    .send(WireOutbound::Binary(vec![0u8; 60_000]))
                    .await
                    .is_ok()
                {}
            });

            let closed = tokio::time::timeout(Duration::from_secs(20), inbound.recv())
                .await
                .expect("a stalled write must end the connection")
                .expect("a close report");
            assert!(matches!(closed, WireInbound::Closed { code: None, .. }));
            pump.await.expect("pump ends");
            writer.abort();
            drop(release);
        });
    }

    #[test]
    fn a_message_larger_than_any_legitimate_frame_ends_the_connection() {
        run(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            tokio::spawn(async move {
                let mut socket = accept_socket(listener).await;
                let oversized = vec![0u8; MAX_RELAY_PAYLOAD_LEN + RELAY_HEADER_LEN + 1];
                if let Err(error) = socket.send(Message::binary(oversized)).await {
                    log::debug!("the test relay could not send: {error}");
                }
                while let Some(Ok(_)) = socket.next().await {}
            });
            let socket = connect_websocket(&format!("ws://{address}/api/relay"), "t")
                .await
                .expect("connect");
            let (inbound_sender, inbound) = channel::bounded(8);
            let (_outbound, outbound_receiver) = channel::bounded(8);
            let pump = tokio::spawn(pump_socket(
                socket,
                inbound_sender,
                outbound_receiver,
                PumpLimits::default(),
            ));

            let first = tokio::time::timeout(Duration::from_secs(5), inbound.recv())
                .await
                .expect("the oversize message is refused promptly")
                .expect("a close report");
            assert!(
                matches!(first, WireInbound::Closed { code: None, .. }),
                "{first:?}"
            );
            pump.await.expect("pump ends");
        });
    }
}
