//! The ports a container publishes, read out of the engine's own text.
//!
//! The backend hands the `Ports` column over as the string the engine printed,
//! and the panel is not allowed to trust or interpret engine output beyond
//! showing it. This is the one place that reads it, and it only ever extracts
//! numbers: the result is a list of `u16`, never text that goes on to a URL.

use util::command::CommandHost;

/// Host ports published over TCP, in the order the engine listed them, each once.
///
/// `0.0.0.0:8080->80/tcp, :::8080->80/tcp, 5432/tcp` is one port: the `:::`
/// entry is the IPv6 twin of the first, and `5432/tcp` is exposed but not
/// published, so there is nothing on the host to open. UDP is skipped because a
/// browser cannot speak it, and a range (`8000-8010->8000-8010/tcp`) because it
/// does not say which of its ports is the web one.
pub(crate) fn published_ports(detail: &[(&'static str, String)]) -> Vec<u16> {
    let Some((_, ports)) = detail.iter().find(|(label, _)| *label == "Ports") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in ports.split(',') {
        let Some((published, container_side)) = entry.trim().split_once("->") else {
            continue;
        };
        let protocol = container_side
            .rsplit_once('/')
            .map(|(_, protocol)| protocol);
        if protocol.is_some_and(|protocol| protocol != "tcp") {
            continue;
        }
        let Some(port) = published
            .rsplit(':')
            .next()
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port != 0)
        else {
            continue;
        };
        if !found.contains(&port) {
            found.push(port);
        }
    }
    found
}

/// The address a browser on this machine reaches a port published on `host` at,
/// or `None` when that is not known.
///
/// Never taken from `remote_name`: that is a name for messages -- an SSH
/// nickname, a WSL distro -- and a URL built from it opens the wrong place or
/// nothing at all. This machine is localhost; any other host says for itself.
pub(crate) fn address_of(host: &dyn CommandHost) -> Option<String> {
    match host.remote_name() {
        None => Some("localhost".to_string()),
        Some(_) => host.reachable_address(),
    }
}

/// Where a published port answers, or `None` when there is no honest address.
///
/// Only when `address` reads as a host: an address that is not one would put
/// something else in the URL, which is worse than offering nothing.
pub(crate) fn port_url(address: Option<&str>, port: u16) -> Option<String> {
    let host = address.filter(|address| is_plain_host(address))?;
    Some(format!("http://{host}:{port}"))
}

/// `host:port` for each of `ports`, or the bare port when there is no address
/// to put in front of it. Text to copy, never a URL to open.
pub(crate) fn copyable_addresses(address: Option<&str>, ports: &[u16]) -> Vec<String> {
    let host = address.filter(|address| is_plain_host(address));
    ports
        .iter()
        .map(|port| match host {
            Some(host) => format!("{host}:{port}"),
            None => port.to_string(),
        })
        .collect()
}

fn is_plain_host(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
        && !name.starts_with(['.', '-'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ports(text: &str) -> Vec<u16> {
        published_ports(&[("Image", "nginx".into()), ("Ports", text.into())])
    }

    #[test]
    fn a_port_published_on_both_stacks_is_one_port() {
        assert_eq!(ports("0.0.0.0:8080->80/tcp, :::8080->80/tcp"), [8080]);
    }

    #[test]
    fn only_published_tcp_ports_count() {
        assert_eq!(
            ports("5432/tcp, 0.0.0.0:9000->9000/udp, 127.0.0.1:3000->3000/tcp"),
            [3000],
            "exposed-only and udp are not something a browser opens"
        );
    }

    #[test]
    fn several_ports_keep_the_engines_order() {
        assert_eq!(
            ports("0.0.0.0:443->443/tcp, 0.0.0.0:80->80/tcp, [::]:443->443/tcp"),
            [443, 80]
        );
    }

    #[test]
    fn a_range_and_nonsense_are_skipped_rather_than_guessed() {
        assert_eq!(ports("0.0.0.0:8000-8010->8000-8010/tcp"), Vec::<u16>::new());
        assert_eq!(
            ports("->80/tcp, 0.0.0.0:99999->80/tcp, :0->80/tcp, ???"),
            Vec::<u16>::new()
        );
        assert_eq!(ports(""), Vec::<u16>::new());
    }

    #[test]
    fn a_row_without_a_ports_column_publishes_nothing() {
        assert!(published_ports(&[("Image", "nginx".into())]).is_empty());
        assert!(published_ports(&[]).is_empty());
    }

    #[test]
    fn a_known_address_becomes_the_url() {
        assert_eq!(
            port_url(Some("localhost"), 8080).as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            port_url(Some("build-box.example.com"), 3000).as_deref(),
            Some("http://build-box.example.com:3000")
        );
        assert_eq!(
            port_url(Some("10.0.0.7"), 80).as_deref(),
            Some("http://10.0.0.7:80")
        );
    }

    #[test]
    fn no_address_is_no_url_and_so_is_one_that_is_not_a_host() {
        assert_eq!(port_url(None, 80), None);
        for name in [
            "my box",
            "[podman] dev",
            "user@host",
            "host/path",
            "",
            "-x",
            "a:b",
        ] {
            assert_eq!(port_url(Some(name), 80), None, "{name:?}");
        }
    }

    #[test]
    fn what_is_copied_is_host_and_port_or_the_port_alone() {
        assert_eq!(
            copyable_addresses(Some("10.0.0.7"), &[8080, 443]),
            ["10.0.0.7:8080", "10.0.0.7:443"]
        );
        assert_eq!(copyable_addresses(None, &[8080, 443]), ["8080", "443"]);
        assert_eq!(
            copyable_addresses(Some("my box"), &[80]),
            ["80"],
            "a name that is not a host is not put in front of a port"
        );
    }
}
