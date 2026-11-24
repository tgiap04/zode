//! Checks for every value that becomes a tool argument here or, later, in a
//! shell command line for Run app.

use std::fmt;
use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidInput {
    pub what: &'static str,
    pub value: String,
    pub reason: String,
}

/// What an error repeats of a value.
const MAX_SHOWN_VALUE_CHARS: usize = 200;

impl InvalidInput {
    fn new(what: &'static str, value: &str, reason: impl Into<String>) -> Self {
        // The value is untrusted and the error reaches the screen.
        let shown: String = value.chars().take(MAX_SHOWN_VALUE_CHARS).collect();
        Self {
            what,
            value: shown,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for InvalidInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.value.is_empty() {
            write!(formatter, "{} is not accepted: {}", self.what, self.reason)
        } else {
            write!(
                formatter,
                "{} \"{}\" is not accepted: {}",
                self.what, self.value, self.reason
            )
        }
    }
}

impl std::error::Error for InvalidInput {}

/// The rule every value obeys: a leading `-` would be read as an option, and
/// shells rewrite `$` and `%` before the tool sees them.
pub fn validate_argument(what: &'static str, value: &str) -> Result<(), InvalidInput> {
    if value.is_empty() {
        return Err(InvalidInput::new(what, value, "it is empty"));
    }
    if value.starts_with('-') {
        return Err(InvalidInput::new(
            what,
            value,
            "it starts with \"-\" and would be read as an option",
        ));
    }
    if let Some(character) = value
        .chars()
        .find(|character| matches!(character, '$' | '%'))
    {
        return Err(InvalidInput::new(
            what,
            value,
            format!("it contains \"{character}\", which a shell would expand"),
        ));
    }
    if let Some(character) = value
        .chars()
        .find(|character| character.is_whitespace() || character.is_control())
    {
        return Err(InvalidInput::new(
            what,
            value,
            format!("it contains the character {character:?}"),
        ));
    }
    if let Some(character) = value
        .chars()
        .find(|character| matches!(character, ';' | '|' | '&' | '`' | '<' | '>' | '(' | ')'))
    {
        return Err(InvalidInput::new(
            what,
            value,
            format!("it contains \"{character}\", which a shell would interpret"),
        ));
    }
    Ok(())
}

fn validate_shape(
    what: &'static str,
    value: &str,
    first_allowed: impl Fn(char) -> bool,
    rest_allowed: impl Fn(char) -> bool,
    expected: &str,
) -> Result<(), InvalidInput> {
    validate_argument(what, value)?;
    let mut characters = value.chars();
    let first_ok = characters.next().is_some_and(&first_allowed);
    if first_ok && characters.all(rest_allowed) {
        Ok(())
    } else {
        Err(InvalidInput::new(
            what,
            value,
            format!("expected {expected}"),
        ))
    }
}

pub fn validate_udid(value: &str) -> Result<(), InvalidInput> {
    validate_shape(
        "UDID",
        value,
        |character| character.is_ascii_hexdigit(),
        |character| character.is_ascii_hexdigit() || character == '-',
        "25 to 40 hexadecimal digits and dashes",
    )?;
    if (25..=40).contains(&value.len()) {
        Ok(())
    } else {
        Err(InvalidInput::new(
            "UDID",
            value,
            "expected 25 to 40 hexadecimal digits and dashes",
        ))
    }
}

pub fn validate_serial(value: &str) -> Result<(), InvalidInput> {
    validate_shape(
        "serial",
        value,
        |character| character.is_ascii_alphanumeric(),
        |character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-'),
        "letters, digits, \".\", \"_\", \":\" and \"-\", starting with a letter or digit",
    )
}

/// Longer than any AVD name `avdmanager` or Android Studio makes.
const MAX_AVD_NAME_CHARS: usize = 100;

pub fn validate_avd_name(value: &str) -> Result<(), InvalidInput> {
    if value.chars().count() > MAX_AVD_NAME_CHARS {
        return Err(InvalidInput::new(
            "AVD name",
            value,
            format!("it is longer than {MAX_AVD_NAME_CHARS} characters"),
        ));
    }
    validate_shape(
        "AVD name",
        value,
        |character| character.is_ascii_alphanumeric(),
        |character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'),
        "letters, digits, \".\", \"_\" and \"-\", starting with a letter or digit",
    )
}

/// A Gradle module, an Android application id or an iOS bundle id.
pub fn validate_identifier(what: &'static str, value: &str) -> Result<(), InvalidInput> {
    validate_shape(
        what,
        value,
        |character| character.is_ascii_alphanumeric(),
        |character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'),
        "letters, digits, \".\", \"_\" and \"-\", starting with a letter or digit",
    )
}

/// An Xcode scheme or a workspace or project file name, which may hold
/// spaces; the spawner quotes them.
pub fn validate_xcode_name(what: &'static str, value: &str) -> Result<(), InvalidInput> {
    if value.is_empty() {
        return Err(InvalidInput::new(what, value, "it is empty"));
    }
    if value.starts_with('-') {
        return Err(InvalidInput::new(
            what,
            value,
            "it starts with \"-\" and would be read as an option",
        ));
    }
    match value.chars().find(|character| {
        !(character.is_ascii_alphanumeric() || matches!(character, ' ' | '.' | '_' | '-' | '+'))
    }) {
        Some(character) => Err(InvalidInput::new(
            what,
            value,
            format!(
                "it contains {character:?}; only letters, digits, spaces, \".\", \"_\", \"-\" \
                 and \"+\" are accepted"
            ),
        )),
        None => Ok(()),
    }
}

/// A built `.app` from Xcode's build settings. Spaces are fine, the spawner
/// quotes them; what a shell would still rewrite inside quotes is not.
pub fn validate_app_path(value: &str) -> Result<(), InvalidInput> {
    const WHAT: &str = "app path";
    if !value.starts_with('/') {
        return Err(InvalidInput::new(WHAT, value, "it is not an absolute path"));
    }
    match value.chars().find(|character| {
        matches!(character, '$' | '%' | '`' | '\'' | '"' | '\\') || character.is_control()
    }) {
        Some(character) => Err(InvalidInput::new(
            WHAT,
            value,
            format!("it contains {character:?}, which a shell would interpret"),
        )),
        None => Ok(()),
    }
}

/// An `ip:port` on a private or link-local network, the only places wireless
/// debugging is offered to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WirelessAddress(SocketAddr);

impl fmt::Display for WirelessAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

pub fn parse_wireless_address(value: &str) -> Result<WirelessAddress, InvalidInput> {
    const WHAT: &str = "address";
    validate_argument(WHAT, value)?;
    let address: SocketAddr = value.parse().map_err(|_| {
        InvalidInput::new(
            WHAT,
            value,
            "expected an IP address and port, such as 192.168.1.5:37099",
        )
    })?;
    if address.port() == 0 {
        return Err(InvalidInput::new(
            WHAT,
            value,
            "the port must be 1 to 65535",
        ));
    }
    let is_local_network = match address.ip() {
        IpAddr::V4(ip) => ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            let [first_segment, ..] = ip.segments();
            first_segment & 0xffc0 == 0xfe80
        }
    };
    if !is_local_network {
        return Err(InvalidInput::new(
            WHAT,
            value,
            "only private (192.168.x.x, 10.x.x.x, 172.16-31.x.x) and link-local addresses are allowed",
        ));
    }
    Ok(WirelessAddress(address))
}

#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode(String);

impl PairingCode {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PairingCode(<redacted>)")
    }
}

/// The error never carries the value: a code that is wrong by one digit is
/// still most of a secret.
pub fn parse_pairing_code(value: &str) -> Result<PairingCode, InvalidInput> {
    if value.len() == 6 && value.chars().all(|character| character.is_ascii_digit()) {
        Ok(PairingCode(value.to_string()))
    } else {
        Err(InvalidInput::new(
            "pairing code",
            "",
            "expected exactly 6 digits",
        ))
    }
}
