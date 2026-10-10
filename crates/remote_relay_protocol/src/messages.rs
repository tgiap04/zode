use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    COMMITMENT_LEN, FrameError, KEY_LEN, MAX_INNER_PAYLOAD_LEN, NONCE_LEN, RELAY_PROTOCOL_VERSION,
};

/// Stable error codes carried by [`Control::Error`].
pub mod error_code {
    /// The peer speaks a protocol version this build does not.
    pub const VERSION: &str = "version";
    /// The bytes did not parse as the message they claimed to be.
    pub const MALFORMED: &str = "malformed";
    /// The request names a device or resource the sender may not use.
    pub const UNAUTHORIZED: &str = "unauthorized";
    /// The request names something that does not exist.
    pub const NOT_FOUND: &str = "not_found";
    /// The request is over a size limit.
    pub const TOO_LARGE: &str = "too_large";
    /// The file asked for is not UTF-8 text.
    pub const BINARY: &str = "binary";
    /// The sender is going faster than the receiver allows.
    pub const RATE_LIMITED: &str = "rate_limited";
    /// The receiver failed for a reason that is not the sender's fault.
    pub const INTERNAL: &str = "internal";
}

#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("message is not a valid control message: {0}")]
    Invalid(#[from] serde_json::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Working,
    WaitingForInput,
    Idle,
    Finished,
    Failed,
    /// A status a newer peer introduced. Kept distinct so an old client shows
    /// "unknown" rather than refusing the whole list.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSummary {
    pub id: String,
    pub name: String,
    pub status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSummary {
    pub id: String,
    pub title: String,
    pub columns: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub kind: FileKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Set only on the roots a `files_list` without a `worktree_id` returns:
    /// the id to name in later requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
}

/// A message inside the encrypted channel.
///
/// Fields a peer does not recognise are ignored, so a newer peer can add to a
/// message without breaking an older one. A `t` nobody recognises is an error:
/// guessing what an unknown message meant is how a request gets acted on twice
/// or not at all.
///
/// Requests carry a `request_id` chosen by the sender; the reply repeats it.
/// Bulk content (file bytes, diffs, terminal output) does not travel in a
/// control message but in data frames on the `stream_id` the reply names,
/// ended by an empty data frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Control {
    Hello {
        relay_protocol: u32,
        app_version: String,
        rpc_protocol: u32,
        capabilities: Vec<String>,
    },
    HelloAck {
        relay_protocol: u32,
        app_version: String,
        rpc_protocol: u32,
        capabilities: Vec<String>,
    },
    Error {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<u32>,
    },
    Ping,
    Pong,
    AgentList {
        agents: Vec<AgentSummary>,
    },
    AgentUpdate {
        agent: AgentSummary,
    },
    TerminalList {
        terminals: Vec<TerminalSummary>,
    },
    TerminalAttach {
        terminal_id: String,
        stream_id: u32,
    },
    TerminalAttached {
        terminal_id: String,
        stream_id: u32,
        columns: u16,
        rows: u16,
    },
    TerminalDetach {
        terminal_id: String,
    },
    TerminalResized {
        terminal_id: String,
        columns: u16,
        rows: u16,
    },
    TerminalClosed {
        terminal_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    /// Asks the host for its project server. With a `stream_id` the host
    /// starts `remote_server` and carries its framed messages on that stream,
    /// in both directions, until either end ends the stream; `app_version` and
    /// `proto_version` must then say what this build speaks, because the
    /// project server's messages are only compatible between identical builds.
    /// `path` is the folder the client means to open, or empty while it has not
    /// chosen yet; the host does not restrict the server to it.
    IdeOpen {
        request_id: u32,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proto_version: Option<u32>,
    },
    /// The project server is running and its stream is open. The facts are
    /// what the client cannot learn from the stream before it has built a
    /// connection on it: how the host writes paths and which shells it has.
    IdeOpened {
        request_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<u32>,
        /// `posix` or `windows`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path_style: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_shell: Option<String>,
    },
    /// With no `worktree_id` the reply lists the open folders; with one, the
    /// direct children of `path` inside it (empty `path` is the folder itself).
    FilesList {
        request_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_id: Option<String>,
        path: String,
    },
    /// A listing too long for one message arrives as several, in order, each
    /// repeating `request_id`; every one but the last has `more` set.
    FilesListReply {
        request_id: u32,
        entries: Vec<FileEntry>,
        /// The directory has more entries than the host lists.
        #[serde(default)]
        truncated: bool,
        #[serde(default)]
        more: bool,
    },
    FileRead {
        request_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_id: Option<String>,
        path: String,
    },
    FileReadReply {
        request_id: u32,
        size: u64,
        stream_id: u32,
    },
    DiffRequest {
        request_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_id: Option<String>,
    },
    DiffReply {
        request_id: u32,
        stream_id: u32,
        /// The diff is cut: what follows on the stream is only its start.
        #[serde(default)]
        truncated: bool,
    },
}

pub fn encode_control(control: &Control) -> Result<Vec<u8>, MessageError> {
    let bytes = serde_json::to_vec(control)?;
    if bytes.len() > MAX_INNER_PAYLOAD_LEN {
        return Err(FrameError::TooLarge {
            actual: bytes.len(),
            limit: MAX_INNER_PAYLOAD_LEN,
        }
        .into());
    }
    Ok(bytes)
}

pub fn decode_control(bytes: &[u8]) -> Result<Control, MessageError> {
    Ok(serde_json::from_slice(bytes)?)
}

/// The answer to a peer whose protocol version this build does not speak, or
/// `None` when the versions agree. The caller sends it and then closes.
pub fn reject_unknown_version(peer_relay_protocol: u32) -> Option<Control> {
    (peer_relay_protocol != RELAY_PROTOCOL_VERSION).then(|| Control::Error {
        code: error_code::VERSION.to_string(),
        message: format!(
            "this peer speaks relay protocol {RELAY_PROTOCOL_VERSION}, not {peer_relay_protocol}"
        ),
        request_id: None,
    })
}

/// Messages exchanged while two devices pair, before any shared key exists.
///
/// They travel in the clear and carry only public keys, a nonce and a hash —
/// nothing here is secret, and nothing here is trusted until the short
/// authentication string has been compared by a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum PairingMessage {
    PairRequest {
        #[serde(with = "base64_array")]
        public_key: [u8; KEY_LEN],
        #[serde(with = "base64_array")]
        commitment: [u8; COMMITMENT_LEN],
    },
    PairAccept {
        #[serde(with = "base64_array")]
        public_key: [u8; KEY_LEN],
        #[serde(with = "base64_array")]
        nonce: [u8; NONCE_LEN],
    },
    PairReveal {
        #[serde(with = "base64_array")]
        nonce: [u8; NONCE_LEN],
    },
}

mod base64_array {
    use super::*;

    pub fn serialize<S: Serializer, const LEN: usize>(
        bytes: &[u8; LEN],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const LEN: usize>(
        deserializer: D,
    ) -> Result<[u8; LEN], D::Error> {
        let encoded = String::deserialize(deserializer)?;
        let decoded = BASE64
            .decode(encoded.as_bytes())
            .map_err(serde::de::Error::custom)?;
        let length = decoded.len();
        decoded
            .try_into()
            .map_err(|_| serde::de::Error::custom(format!("expected {LEN} bytes, found {length}")))
    }
}
