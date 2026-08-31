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
    /// The statement that reads a whole table, with `{schema}` and `{table}`
    /// standing for the names already quoted by
    /// [`quote_identifier`](Self::quote_identifier).
    ///
    /// Absent means `SELECT * FROM {schema}.{table}`, which is what every SQL
    /// engine here wants and what clicking a table used to produce
    /// unconditionally. That was the last place Zode still assumed an engine
    /// speaks SQL: a document store was handed a `SELECT` it could only refuse,
    /// so every click on a collection was a failed query.
    ///
    /// An optional field with a cautious default, so it costs no protocol
    /// version -- see `PROTOCOL.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_table_template: Option<String>,
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

/// Takes the password out of a URL, returning the URL without it and the
/// password on its own.
///
/// The mirror of [`ConnectionForm::build_url`], and it exists for the same
/// reason: a URL is written into a settings file people share, sync and back
/// up, and a password does not belong there. `build_url` keeps a secret out of
/// a URL it *builds*; this takes one out of a URL that arrived already built --
/// which is what a URL pasted into the import field is, and there the password
/// was travelling straight into settings in the clear.
///
/// The password is percent-decoded on the way out, because the driver is handed
/// it as a plain string rather than parsing it out of a URL itself.
///
/// A string with no `://` is left alone: that is a file path, and SQLite's
/// connection string has no userinfo to find.
pub fn split_password(url: &str) -> (String, Option<String>) {
    let Some((scheme, rest)) = url.split_once("://") else {
        return (url.to_string(), None);
    };
    // The authority runs to the first `/`, `?` or `#`. A password may contain
    // any of them once encoded, so the search has to stop at the authority
    // rather than at the first one anywhere in the URL.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    // The *last* `@`: an unencoded `@` inside a password is what makes a URL
    // ambiguous, and the one that separates userinfo from host is the last one.
    let Some((userinfo, host)) = authority.rsplit_once('@') else {
        return (url.to_string(), None);
    };
    let Some((user, password)) = userinfo.split_once(':') else {
        return (url.to_string(), None);
    };
    if password.is_empty() {
        return (url.to_string(), None);
    }
    (
        format!("{scheme}://{user}@{host}{tail}"),
        Some(percent_decode(password)),
    )
}

/// Undoes [`percent_encode`] for the one value that leaves a URL again.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = bytes[index] == b'%'
            && index + 3 <= bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit();
        if escape {
            let hex = [bytes[index + 1], bytes[index + 2]];
            // Both digits were just checked, so this cannot fail -- but it is
            // read rather than unwrapped, because a panic here would be one
            // inside a password.
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&hex).unwrap_or("zz"), 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
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

impl Capabilities {
    /// The statement that reads a whole table, with both names quoted this
    /// engine's way.
    ///
    /// Built here rather than at the panel so that adding an engine whose
    /// "read everything" is not a `SELECT` stays a driver-side change -- the
    /// same bargain [`quote_identifier`](Self::quote_identifier) already makes
    /// for the quote character.
    pub fn read_table_statement(&self, schema: &str, table: &str) -> String {
        let schema = self.quote_identifier(schema);
        let table = self.quote_identifier(table);
        match &self.read_table_template {
            Some(template) => template
                .replace("{schema}", &schema)
                .replace("{table}", &table),
            None => format!("SELECT * FROM {schema}.{table}"),
        }
    }
}

#[cfg(test)]
mod read_table_tests {
    use super::*;

    /// What every SQL engine here gets, and what the panel used to build for
    /// all of them regardless of what the driver could read.
    #[test]
    fn a_driver_that_says_nothing_gets_a_select() {
        let capabilities = Capabilities::default();
        assert_eq!(
            capabilities.read_table_statement("public", "users"),
            r#"SELECT * FROM "public"."users""#
        );
    }

    /// The defect this closes: a document store was handed a `SELECT` it could
    /// only refuse, so every click on a collection failed.
    #[test]
    fn a_driver_that_reads_no_sql_gets_its_own_statement() {
        let capabilities = Capabilities {
            read_table_template: Some(r#"{"find": {table}, "$db": {schema}}"#.to_string()),
            ..Default::default()
        };
        assert_eq!(
            capabilities.read_table_statement("app", "users"),
            r#"{"find": "users", "$db": "app"}"#
        );
    }

    /// A name from the tree becomes a statement here, and a table really can be
    /// called `"; drop table users; --`.
    #[test]
    fn a_name_that_would_escape_its_quotes_is_still_quoted() {
        let capabilities = Capabilities::default();
        assert_eq!(
            capabilities.read_table_statement("public", r#"a"b"#),
            r#"SELECT * FROM "public"."a""b""#
        );
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

    /// The defect this closes. A URL pasted into the import field is one
    /// plain field with no `secret` on it, so `build_url` had nothing to blank
    /// and the whole string -- password included -- was written into the
    /// settings file, which people share, sync and back up. The keychain got
    /// nothing, because nothing was marked as a secret to put there.
    #[test]
    fn a_pasted_url_gives_up_its_password() {
        let (url, password) =
            split_password("mongodb://tgiap:s3cret@db.example:27017/app?authSource=admin");
        assert_eq!(url, "mongodb://tgiap@db.example:27017/app?authSource=admin");
        assert_eq!(password.as_deref(), Some("s3cret"));
        assert!(!url.contains("s3cret"), "{url}");
    }

    /// The user has to stay: every driver connects as whoever the URL names,
    /// and only the password travels separately.
    #[test]
    fn the_user_stays_in_the_url() {
        let (url, _) = split_password("postgres://someone:pw@host:5432/app");
        assert_eq!(url, "postgres://someone@host:5432/app");
    }

    /// The driver is handed a plain string rather than parsing it out of a URL,
    /// so what comes out here has to be the password itself.
    #[test]
    fn an_encoded_password_is_decoded_on_its_way_out() {
        let (_, password) = split_password("postgres://someone:p%40ss%20word@host/app");
        assert_eq!(password.as_deref(), Some("p@ss word"));
    }

    /// An unencoded `@` inside a password is what makes a URL ambiguous. The
    /// `@` that separates userinfo from host is the last one.
    #[test]
    fn the_host_is_found_past_an_at_sign_in_the_password() {
        let (url, password) = split_password("mysql://someone:a@b@host:3306/app");
        assert_eq!(url, "mysql://someone@host:3306/app");
        assert_eq!(password.as_deref(), Some("a@b"));
    }

    /// Nothing to take, nothing changed -- including a SQLite path, which has
    /// no userinfo to look for and must survive untouched.
    #[test]
    fn a_url_without_a_password_is_left_exactly_as_it_was() {
        for url in [
            "postgres://someone@host/app",
            "postgres://host/app",
            "/home/someone/my app.sqlite",
            "mongodb://someone:@host/app",
        ] {
            let (out, password) = split_password(url);
            assert_eq!(out, url, "{url} was rewritten");
            assert_eq!(password, None, "{url} gave up a password it does not have");
        }
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
