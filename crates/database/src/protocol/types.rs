use serde::{Deserialize, Serialize};

/// A namespace of tables. SQLite has exactly one (`main`); Postgres has many;
/// MySQL calls its databases schemas. Drivers map their engine onto this rather
/// than the other way round -- nothing above this layer knows which engine it
/// is talking to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaRef {
    pub name: String,
    /// Set on the schema a fresh connection is already pointed at, so a tree can
    /// open it without guessing which of twenty is the interesting one.
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    Table,
    View,
    /// A view whose rows are stored. Worth keeping apart from `View` because
    /// reading one costs nothing while reading a view can cost a great deal.
    MaterializedView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRef {
    pub name: String,
    pub kind: TableKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    /// The engine's own name for the type (`int4`, `jsonb`, `VARCHAR(255)`),
    /// carried through for display only. Nothing above this layer parses it --
    /// that is what `Cell`'s own tag is for.
    pub type_name: String,
    pub nullable: bool,
    pub primary_key: bool,
}

/// One value, already rendered by the driver.
///
/// The driver formats, because it is the only layer that knows what its
/// engine's types mean; the tag says how to *present* the result, not what the
/// engine called it. So a grid can right-align numbers and grey out nulls
/// without a table of engine-specific type names.
///
/// `Null` is a variant rather than an `Option<String>` so it can never be
/// confused with the empty string -- a distinction that matters in a database
/// client more than almost anywhere else.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cell {
    Null,
    Text {
        value: String,
    },
    /// Kept as text, not as a JSON number: `numeric(38,10)` and `u64` both lose
    /// digits through an f64, and a database client that quietly rounds the
    /// values it is asked to display is worse than useless.
    Number {
        value: String,
    },
    Bool {
        value: bool,
    },
    /// Never the bytes themselves. Base64 of a large blob through a
    /// line-delimited JSON pipe is how a driver stalls the whole editor.
    Binary {
        byte_len: u64,
    },
    Json {
        value: String,
    },
    Timestamp {
        value: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultColumn {
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultSet {
    pub columns: Vec<ResultColumn>,
    pub rows: Vec<Vec<Cell>>,
    /// More rows exist past this page. Drivers answer it by asking for one row
    /// more than the page and dropping it, so it costs no `COUNT(*)`.
    pub truncated: bool,
    pub elapsed_ms: u64,
}

/// What a driver can do, answered once at `initialize`.
///
/// Read at startup so the UI can hide what an engine cannot do rather than
/// offering it and failing. Every field defaults to the cautious answer, so a
/// driver written against an older version of this struct stays correct.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Whether the engine has more than one schema worth showing. SQLite says
    /// no, and a tree that draws a lone `main` node for it wastes a level.
    #[serde(default)]
    pub multiple_schemas: bool,
    /// Whether `cancel` does anything. A driver that cannot cancel says so, and
    /// the UI offers no button that would lie.
    #[serde(default)]
    pub cancellation: bool,
    /// The character this engine wraps identifiers in.
    ///
    /// Absent means the SQL standard's double quote, which SQLite and
    /// PostgreSQL accept. MySQL is the reason this exists: it wants backticks
    /// unless `ANSI_QUOTES` is set, which is not the default and is not a
    /// driver's to change on a session the user also types into.
    ///
    /// Added as an optional field with a cautious default rather than as a new
    /// method, so it costs no protocol version -- see `PROTOCOL.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier_quote: Option<String>,
    /// What to ask for when someone adds a connection to this engine.
    ///
    /// Absent means Zode asks for a URL and nothing else, which is correct for
    /// any driver written before this existed and for any engine whose DSN does
    /// not decompose. A driver that fills this in gets a proper form instead,
    /// and Zode never learns what a host or a file path means to it.
    ///
    /// An optional field with a cautious default, so it costs no protocol
    /// version -- see `PROTOCOL.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_form: Option<ConnectionForm>,
}

/// How to ask for a connection to one engine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionForm {
    pub fields: Vec<ConnectionField>,
    /// The `url` a filled-in form becomes, with `{key}` standing for a field.
    ///
    /// A whole template rather than pieces Zode assembles: only the driver
    /// knows whether its engine wants a scheme, a socket or a bare file path,
    /// and a client that assembled DSNs would be a client that knows engines.
    pub url_template: String,
}

/// One thing to ask for.
///
/// Every field is required except a [`secret`](Self::secret) one -- a server
/// that wants no password is served by leaving it blank. A driver that needs an
/// optional non-secret field should give it a `default` instead, so what the
/// user sees is always a complete answer they can edit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionField {
    /// What `url_template` calls this field.
    pub key: String,
    /// What the person filling the form is shown.
    pub label: String,
    /// The heading this field sits under -- "Connection", "Authentication".
    ///
    /// Absent puts it in an unheaded group at the top, which is right for a
    /// form with too few fields to be worth dividing. Only the driver can
    /// group these: which of its fields are about *reaching* the server and
    /// which are about *being let in* is a fact about the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Filled in before anyone types. The way a driver says "5432" without Zode
    /// knowing what a port is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// A password: masked while typed, put in the OS keychain, and **never**
    /// substituted into the URL or written to a settings file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
    /// Percent-encode this value before it goes into the URL.
    ///
    /// Off by default, which is what a bare file path needs. A driver whose
    /// template is a real URL turns it on for the parts that sit inside the
    /// authority, where a `@` or a `/` in a name would otherwise change what
    /// the URL means.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub url_encoded: bool,
}

impl ConnectionForm {
    /// Substitutes filled-in values into [`Self::url_template`].
    ///
    /// Lives here so every driver's URL is built the same way, and so the UI
    /// stays unable to tell one engine's template from another's.
    pub fn build_url(&self, value_for: impl Fn(&str) -> String) -> String {
        let mut url = self.url_template.clone();
        for field in &self.fields {
            // A secret is not part of the address. It goes to the keychain, and
            // a URL carrying it would be written into a settings file.
            let value = if field.secret {
                String::new()
            } else if field.url_encoded {
                percent_encode(&value_for(&field.key))
            } else {
                value_for(&field.key)
            };
            url = url.replace(&format!("{{{}}}", field.key), &value);
        }
        url
    }
}

/// Percent-encodes everything outside the unreserved set.
///
/// Deliberately strict rather than clever: this runs on a user name or a
/// database name that is about to sit inside a URL's authority, and the cost of
/// encoding one character too many is nothing next to the cost of letting an
/// `@` decide where the host begins.
fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

impl Capabilities {
    /// Wraps an identifier the way this engine expects, doubling any quote
    /// character it already contains.
    ///
    /// Lives here rather than in the UI so that adding an engine with different
    /// quoting stays a driver-side change.
    pub fn quote_identifier(&self, identifier: &str) -> String {
        let quote = self.identifier_quote.as_deref().unwrap_or("\"");
        format!(
            "{quote}{}{quote}",
            identifier.replace(quote, &quote.repeat(2))
        )
    }
}

#[cfg(test)]
mod connection_form_tests {
    use super::*;

    fn server_form() -> ConnectionForm {
        let field = |key: &str| ConnectionField {
            key: key.into(),
            label: key.into(),
            url_encoded: true,
            ..Default::default()
        };
        ConnectionForm {
            fields: vec![
                field("host"),
                field("user"),
                field("database"),
                ConnectionField {
                    key: "password".into(),
                    label: "Password".into(),
                    secret: true,
                    ..Default::default()
                },
            ],
            url_template: "engine://{user}@{host}/{database}?p={password}".into(),
        }
    }

    /// The whole reason a secret is a separate kind of field: the URL it would
    /// land in is written into a settings file people share and back up.
    #[test]
    fn a_secret_never_reaches_the_url() {
        let url = server_form().build_url(|key| match key {
            "password" => "hunter2".into(),
            other => other.into(),
        });
        assert!(!url.contains("hunter2"), "{url}");
        assert_eq!(url, "engine://user@host/database?p=");
    }

    /// An `@` in a user name decides where the host begins. Left raw, a
    /// connection quietly points somewhere nobody asked for.
    #[test]
    fn a_value_that_would_change_the_url_is_encoded() {
        let url = server_form().build_url(|key| match key {
            "user" => "a@b".into(),
            "host" => "real.example".into(),
            other => other.into(),
        });
        assert_eq!(url, "engine://a%40b@real.example/database?p=");
    }

    /// A file path is not a URL, and encoding one turns every separator into
    /// `%2F`. This is why encoding is per field and off by default.
    #[test]
    fn a_path_field_is_left_exactly_as_typed() {
        let form = ConnectionForm {
            fields: vec![ConnectionField {
                key: "path".into(),
                label: "File".into(),
                ..Default::default()
            }],
            url_template: "{path}".into(),
        };
        assert_eq!(
            form.build_url(|_| "/home/someone/my app.sqlite".into()),
            "/home/someone/my app.sqlite"
        );
    }
}
