//! Reading a connection URL back into a driver's form.
//!
//! The mirror of [`ConnectionForm::build_url`], and like it blind to which
//! engine the form is for: the template and the pasted URL are taken apart the
//! same way, and wherever the template has `{field}` the URL's part in that
//! place is the field's value. That is what lets one paste fill a Postgres form
//! and a Mongo form alike without this crate knowing what either is.

use super::types::{ConnectionForm, percent_decode};

/// What [`ConnectionForm::import_url`] read out of a URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportedUrl {
    /// `(field key, value)` for every field the URL had a value for. A field
    /// the URL says nothing about is absent, so whatever it held is kept.
    pub values: Vec<(String, String)>,
    /// What the URL carried that the form has no field for -- a query option,
    /// say -- so it can be named rather than dropped without a word.
    pub left_out: Vec<String>,
}

impl ConnectionForm {
    /// Reads `url` into this form's fields. A password in the URL goes to the
    /// form's secret field, never into an address field.
    ///
    /// Refuses text that is not a URL, and a URL whose scheme names another
    /// engine: filling a Postgres form from a MySQL URL would save a connection
    /// that cannot work. A scheme that only extends this one (`postgresql` for
    /// `postgres`, `mongodb+srv` for `mongodb`) is accepted.
    pub fn import_url(&self, url: &str) -> Result<ImportedUrl, String> {
        let url = url.trim();
        if url.is_empty() {
            return Err("Paste a URL first.".into());
        }
        let template = self.url_template.trim();
        // A form that is one field -- SQLite's path -- takes the text whole.
        if let Some(key) = placeholder(template) {
            return Ok(ImportedUrl {
                values: vec![(key.to_string(), url.to_string())],
                left_out: Vec::new(),
            });
        }

        let Some(shape) = UrlParts::parse(template, true) else {
            return Err("This engine's form cannot be filled from a URL.".into());
        };
        // Split the port off the host only when the form has a field for it;
        // otherwise `host:port` -- or a list of them -- is the host's value.
        let has_port_field = shape.port.is_some();
        let Some(pasted) = UrlParts::parse(url, has_port_field) else {
            return Err("That is not a URL: it has no `scheme://` at the front.".into());
        };
        if !same_engine(shape.scheme, pasted.scheme) {
            return Err(format!(
                "That is a {} URL; this form takes {}:// URLs.",
                pasted.scheme, shape.scheme
            ));
        }

        let mut imported = ImportedUrl::default();
        // `postgresql` saved as `postgres` means the same server; `mongodb+srv`
        // saved as `mongodb` does not -- the `+srv` asks for a DNS lookup the
        // form's URL will not make -- so a scheme that adds to this one is said.
        if pasted.scheme.contains('+') && !shape.scheme.contains('+') {
            imported.left_out.push(format!(
                "the {} scheme (saved as {}://)",
                pasted.scheme, shape.scheme
            ));
        }
        let mut take = |template_part: Option<&str>, pasted_part: Option<&str>| {
            let (Some(key), Some(value)) = (template_part.and_then(placeholder), pasted_part)
            else {
                return;
            };
            if !value.is_empty() {
                imported
                    .values
                    .push((key.to_string(), percent_decode(value)));
            }
        };
        take(shape.user, pasted.user);
        take(shape.host, pasted.host);
        take(shape.port, pasted.port);
        take(shape.path, pasted.path);

        if let Some(password) = pasted.password.filter(|password| !password.is_empty()) {
            match self.fields.iter().find(|field| field.secret) {
                Some(secret) => imported
                    .values
                    .push((secret.key.clone(), percent_decode(password))),
                None => imported.left_out.push("the password".into()),
            }
        }

        for (name, value) in pasted.query {
            let key = shape
                .query
                .iter()
                .find(|(template_name, _)| *template_name == name)
                .and_then(|(_, template_value)| placeholder(template_value));
            match key {
                Some(key) => imported
                    .values
                    .push((key.to_string(), percent_decode(value))),
                None => imported.left_out.push(format!("{name}={value}")),
            }
        }
        Ok(imported)
    }
}

/// `key` when `text` is exactly `{key}`.
fn placeholder(text: &str) -> Option<&str> {
    text.strip_prefix('{')?
        .strip_suffix('}')
        .filter(|key| !key.is_empty() && !key.contains(['{', '}']))
}

/// The same engine, allowing a scheme that only extends the other.
fn same_engine(expected: &str, pasted: &str) -> bool {
    let expected = expected.to_ascii_lowercase();
    let pasted = pasted.to_ascii_lowercase();
    pasted.starts_with(&expected) || expected.starts_with(&pasted)
}

/// A URL taken apart, without decoding: `scheme://user:password@host:port/path?query`.
struct UrlParts<'a> {
    scheme: &'a str,
    user: Option<&'a str>,
    password: Option<&'a str>,
    host: Option<&'a str>,
    port: Option<&'a str>,
    path: Option<&'a str>,
    query: Vec<(&'a str, &'a str)>,
}

impl<'a> UrlParts<'a> {
    fn parse(url: &'a str, split_port: bool) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        if scheme.is_empty() || scheme.contains(['/', '@', ' ']) {
            return None;
        }
        let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
        let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_end);

        // The last `@`: an unencoded one inside a password comes before it.
        let (userinfo, host_port) = match authority.rsplit_once('@') {
            Some((userinfo, host_port)) => (Some(userinfo), host_port),
            None => (None, authority),
        };
        let (user, password) = match userinfo {
            Some(userinfo) => match userinfo.split_once(':') {
                Some((user, password)) => (Some(user), Some(password)),
                None => (Some(userinfo), None),
            },
            None => (None, None),
        };
        let (host, port) = if split_port {
            split_host_port(host_port)
        } else {
            (host_port, None)
        };

        let (path, query) = match tail.split_once('?') {
            Some((path, query)) => (path, query),
            None => (tail, ""),
        };
        let path = path.strip_prefix('/').unwrap_or(path);
        let query = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
            .collect();

        Some(Self {
            scheme,
            user: user.filter(|user| !user.is_empty()),
            password,
            host: Some(host).filter(|host| !host.is_empty()),
            port,
            path: Some(path).filter(|path| !path.is_empty()),
            query,
        })
    }
}

/// `host:port`, minding an IPv6 address's own colons inside `[...]`.
fn split_host_port(host_port: &str) -> (&str, Option<&str>) {
    let colon = if host_port.starts_with('[') {
        host_port.find("]:").map(|bracket| bracket + 1)
    } else {
        host_port.rfind(':')
    };
    match colon {
        Some(colon) => {
            let port = &host_port[colon + 1..];
            (
                &host_port[..colon],
                Some(port).filter(|port| !port.is_empty()),
            )
        }
        None => (host_port, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ConnectionField;

    fn field(key: &str) -> ConnectionField {
        ConnectionField {
            key: key.into(),
            label: key.into(),
            url_encoded: true,
            ..Default::default()
        }
    }

    fn password() -> ConnectionField {
        ConnectionField {
            key: "password".into(),
            label: "Password".into(),
            secret: true,
            ..Default::default()
        }
    }

    /// The shape of the Postgres and MySQL drivers' forms.
    fn postgres_form() -> ConnectionForm {
        ConnectionForm {
            fields: vec![
                field("host"),
                field("port"),
                field("database"),
                field("user"),
                password(),
            ],
            url_template: "postgres://{user}@{host}:{port}/{database}".into(),
        }
    }

    fn value<'a>(imported: &'a ImportedUrl, key: &str) -> Option<&'a str> {
        imported
            .values
            .iter()
            .find(|(field, _)| field == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn every_part_of_the_url_lands_in_its_field_and_the_password_in_the_secret() {
        let imported = postgres_form()
            .import_url("postgres://ada:s3cr%40t@db.example.com:6543/shop")
            .expect("a postgres URL");
        assert_eq!(value(&imported, "user"), Some("ada"));
        assert_eq!(value(&imported, "password"), Some("s3cr@t"));
        assert_eq!(value(&imported, "host"), Some("db.example.com"));
        assert_eq!(value(&imported, "port"), Some("6543"));
        assert_eq!(value(&imported, "database"), Some("shop"));
        assert!(imported.left_out.is_empty(), "{:?}", imported.left_out);
    }

    /// A URL without a port leaves the port field as it was -- the driver's
    /// default -- rather than blanking it.
    #[test]
    fn a_part_the_url_leaves_out_fills_nothing() {
        let imported = postgres_form()
            .import_url("postgresql://ada@localhost/shop")
            .expect("postgresql is postgres");
        assert_eq!(value(&imported, "port"), None);
        assert_eq!(value(&imported, "password"), None);
        assert_eq!(value(&imported, "host"), Some("localhost"));
    }

    /// Nothing the form cannot hold is dropped without a word.
    #[test]
    fn what_the_form_has_no_field_for_is_named_as_left_out() {
        let imported = postgres_form()
            .import_url("postgres://ada@localhost:5432/shop?sslmode=require")
            .expect("a postgres URL");
        assert_eq!(imported.left_out, vec!["sslmode=require".to_string()]);
    }

    #[test]
    fn another_engines_url_is_refused() {
        let error = postgres_form()
            .import_url("mysql://ada@localhost:3306/shop")
            .expect_err("a mysql URL is not a postgres one");
        assert!(error.contains("mysql"), "{error}");
    }

    #[test]
    fn text_that_is_not_a_url_is_refused() {
        assert!(postgres_form().import_url("localhost:5432").is_err());
        assert!(postgres_form().import_url("   ").is_err());
    }

    /// A form with no port field keeps the port with the host, which is what a
    /// Mongo host list -- several `host:port`s -- needs.
    #[test]
    fn without_a_port_field_the_host_keeps_its_ports_and_query_fields_are_read() {
        let form = ConnectionForm {
            fields: vec![
                field("host"),
                field("database"),
                field("user"),
                field("auth_source"),
                password(),
            ],
            url_template: "mongodb://{user}@{host}/{database}?authSource={auth_source}".into(),
        };
        let imported = form
            .import_url("mongodb://ada:pw@a.example:27017,b.example:27017/shop?authSource=admin&retryWrites=true")
            .expect("a mongodb URL");
        assert_eq!(
            value(&imported, "host"),
            Some("a.example:27017,b.example:27017")
        );
        assert_eq!(value(&imported, "auth_source"), Some("admin"));
        assert_eq!(value(&imported, "password"), Some("pw"));
        assert_eq!(imported.left_out, vec!["retryWrites=true".to_string()]);
    }

    #[test]
    fn a_scheme_the_form_cannot_write_is_named() {
        let form = ConnectionForm {
            fields: vec![field("host"), field("database"), field("user"), password()],
            url_template: "mongodb://{user}@{host}/{database}".into(),
        };
        let imported = form
            .import_url("mongodb+srv://ada@cluster.example/shop")
            .expect("still a mongodb URL");
        assert_eq!(value(&imported, "host"), Some("cluster.example"));
        assert_eq!(
            imported.left_out,
            vec!["the mongodb+srv scheme (saved as mongodb://)".to_string()]
        );
    }

    /// SQLite's "URL" is a file path: the whole text is the one field.
    #[test]
    fn a_template_that_is_one_field_takes_the_whole_text() {
        let form = ConnectionForm {
            fields: vec![field("path")],
            url_template: "{path}".into(),
        };
        let imported = form.import_url(" /data/app.db ").expect("a path");
        assert_eq!(value(&imported, "path"), Some("/data/app.db"));
    }
}
