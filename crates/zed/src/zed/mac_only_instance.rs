use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream},
    thread,
    time::Duration,
};

use sysinfo::System;

use release_channel::ReleaseChannel;

const LOCALHOST: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(10);
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(35);
const SEND_TIMEOUT: Duration = Duration::from_millis(20);
const USER_BLOCK: u16 = 100;

/// Base port for Zode's block, chosen to sit entirely below Zed's.
///
/// Zed derives its port as `43737 + channel * 100 + uid % (65535 - base)`, which
/// means its ports span `[43737, 65534]` -- the whole upper range, leaving no
/// room above it. So Zode's block goes below, and its user offset is capped
/// (rather than allowed to run to the top of the port space) so the two can
/// never meet: the highest port Zode can produce is
/// `39737 + 3 * 100 + 2999 = 43036`, below Zed's lowest of `43737`.
const BASE_PORT: u16 = 39737;

/// How many distinct user IDs get their own port within a channel.
///
/// Zed lets this run to the end of the port space (~21500 values). Capping it at
/// 3000 is what keeps Zode's block disjoint from Zed's, and it costs something:
/// two users whose IDs differ by exactly a multiple of 3000 land on the same
/// port. That is far outside the real range on either platform (macOS starts at
/// 501, Linux at 1000), and when it does happen the bind simply fails and the
/// instance runs without a handshake, which is the same fallback Zed has always
/// had for a port already claimed by something else.
const UID_SPAN: u32 = 3000;

fn address() -> SocketAddr {
    let mut sys = System::new_all();
    sys.refresh_all();
    let uid = sysinfo::get_current_pid().ok().and_then(|current_pid| {
        sys.process(current_pid)
            .and_then(|process| process.user_id())
            .map(get_uid_as_u32)
    });

    SocketAddr::V4(SocketAddrV4::new(
        LOCALHOST,
        port_for(*release_channel::RELEASE_CHANNEL, uid),
    ))
}

/// Split out from [`address`] so it can be tested: `address` reads the running
/// process's user ID, which a test cannot vary.
///
/// Ports are offset by the user ID so two users on one machine do not collide,
/// and the channels are spaced `USER_BLOCK` apart on top of that. Channel blocks
/// interleave across users, exactly as they do upstream -- for one user the four
/// channels always differ by the block size, so they never meet.
fn port_for(channel: ReleaseChannel, uid: Option<u32>) -> u16 {
    let base = BASE_PORT
        + match channel {
            ReleaseChannel::Dev => 0,
            ReleaseChannel::Preview => USER_BLOCK,
            ReleaseChannel::Stable => 2 * USER_BLOCK,
            ReleaseChannel::Nightly => 3 * USER_BLOCK,
        };
    match uid {
        Some(uid) => base + (uid % UID_SPAN) as u16,
        None => base,
    }
}

#[cfg(unix)]
fn get_uid_as_u32(uid: &sysinfo::Uid) -> u32 {
    *uid.clone()
}

#[cfg(windows)]
fn get_uid_as_u32(uid: &sysinfo::Uid) -> u32 {
    // Extract the RID which is an integer
    uid.to_string()
        .rsplit('-')
        .next()
        .and_then(|rid| rid.parse::<u32>().ok())
        .unwrap_or(0)
}

/// The second guard against mistaking a Zed instance for our own. The port block
/// alone would do it today, but a handshake that still said "Zed Editor" would
/// silently start matching again the moment anyone moved the ports back into
/// range.
fn instance_handshake() -> &'static str {
    handshake_for(*release_channel::RELEASE_CHANNEL)
}

/// Split from [`instance_handshake`] for the same reason as [`port_for`]: the
/// release channel is a process-wide global a test cannot vary.
fn handshake_for(channel: ReleaseChannel) -> &'static str {
    match channel {
        ReleaseChannel::Dev => "Zode Dev Instance Running",
        ReleaseChannel::Nightly => "Zode Nightly Instance Running",
        ReleaseChannel::Preview => "Zode Preview Instance Running",
        ReleaseChannel::Stable => "Zode Stable Instance Running",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsOnlyInstance {
    Yes,
    No,
}

pub fn ensure_only_instance() -> IsOnlyInstance {
    if check_got_handshake() {
        return IsOnlyInstance::No;
    }

    let listener = match TcpListener::bind(address()) {
        Ok(listener) => listener,

        Err(err) => {
            log::warn!("Error binding to single instance port: {err}");
            if check_got_handshake() {
                return IsOnlyInstance::No;
            }

            // Avoid failing to start when some other application by chance already has
            // a claim on the port. This is sub-par as any other instance that gets launched
            // will be unable to communicate with this instance and will duplicate
            log::warn!("Backup handshake request failed, continuing without handshake");
            return IsOnlyInstance::Yes;
        }
    };

    thread::Builder::new()
        .name("EnsureSingleton".to_string())
        .spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(stream) => stream,
                    Err(_) => return,
                };

                _ = stream.set_nodelay(true);
                _ = stream.set_read_timeout(Some(SEND_TIMEOUT));
                _ = stream.write_all(instance_handshake().as_bytes());
            }
        })
        .unwrap();

    IsOnlyInstance::Yes
}

fn check_got_handshake() -> bool {
    match TcpStream::connect_timeout(&address(), CONNECT_TIMEOUT) {
        Ok(mut stream) => {
            let mut buf = vec![0u8; instance_handshake().len()];

            stream.set_read_timeout(Some(RECEIVE_TIMEOUT)).unwrap();
            if let Err(err) = stream.read_exact(&mut buf) {
                log::warn!("Connected to single instance port but failed to read: {err}");
                return false;
            }

            if buf == instance_handshake().as_bytes() {
                log::info!("Got instance handshake");
                return true;
            }

            log::warn!("Got wrong instance handshake value");
            false
        }

        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lowest port Zed can ever produce. Its own offset runs to the top of
    /// the port space, so this bound -- not an upper one -- is what Zode has to
    /// stay clear of.
    const ZED_LOWEST_PORT: u16 = 43737;

    const ALL_CHANNELS: [ReleaseChannel; 4] = [
        ReleaseChannel::Dev,
        ReleaseChannel::Preview,
        ReleaseChannel::Stable,
        ReleaseChannel::Nightly,
    ];

    /// Installing Zed alongside Zode must not make one of them refuse to start.
    /// Sharing a port means the second launch reads the first one's handshake and
    /// exits with "already running".
    #[test]
    fn no_channel_can_reach_zeds_port_range() {
        // Sweep the whole wrap rather than one machine's user ID: the failure is
        // a range overlap, and a single sample would miss it.
        for channel in ALL_CHANNELS {
            for uid in [None, Some(0), Some(501), Some(1000), Some(UID_SPAN - 1)] {
                let port = port_for(channel, uid);
                assert!(
                    port < ZED_LOWEST_PORT,
                    "{channel:?} with uid {uid:?} resolved to {port}, inside Zed's range"
                );
            }
        }
    }

    /// A user running two channels at once needs them to stay separate, which is
    /// the reason the block spacing exists at all.
    #[test]
    fn one_user_gets_a_distinct_port_per_channel() {
        for uid in [None, Some(501), Some(1000)] {
            let mut ports = ALL_CHANNELS.map(|channel| port_for(channel, uid)).to_vec();
            ports.sort_unstable();
            ports.dedup();
            assert_eq!(ports.len(), 4, "channels collided for uid {uid:?}");
        }
    }

    /// The handshake is the guard that survives someone moving the ports back
    /// into Zed's range, so it has to be checked against the real function --
    /// asserting a copy of the strings here would pass no matter what the
    /// function returned.
    #[test]
    fn handshake_does_not_impersonate_zed() {
        for channel in ALL_CHANNELS {
            let handshake = handshake_for(channel);
            assert!(
                !handshake.contains("Zed"),
                "{channel:?} handshake {handshake:?} still answers as Zed"
            );
            assert!(
                handshake.contains("Zode"),
                "{channel:?} handshake {handshake:?} does not name Zode"
            );
        }
    }
}
