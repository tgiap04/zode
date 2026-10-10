# Remote control — what can and cannot happen

Remote control lets another device operate this Zode through a relay at
zodekit.site. The relay is not open source, and it sits in the middle of every
session. This page exists because that is a reasonable thing to be uneasy
about. Everything here is either checkable by you or held by a test in the
repositories named below; where a claim is weaker than it sounds, it says so.

How to use the feature is in [Remote control](./remote-control.md). The wire
format is in [Remote control protocol](./remote-control-protocol.md).

## The short version

|                                                    |                                                                                                            |
| -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Does it run unless I turn it on?                   | No. Off by default; while off, no connection is opened.                                                    |
| Can the relay read what I do on the remote device? | No. Content is encrypted between the two devices; the relay has no key.                                    |
| Can the relay forge input to my machine?           | No. Each side only accepts keys pinned at pairing.                                                         |
| What does the relay learn?                         | Who connected to whom, when, and how many bytes.                                                           |
| Can a paired device do damage?                     | **Yes.** It has a shell on your machine. Revoke a lost device at once.                                     |
| What stops someone pairing without me?             | A six-digit code you compare yourself, and the fact that you must confirm on the machine being controlled. |

## The threat model, stated plainly

**Assumed hostile:** the relay, its database and operator, and anyone on the
network between your devices and it.

**Assumed trusted:** your two devices, their operating systems, the Zode build
you run, and the browser you pair. If a paired device is compromised, nothing
here helps: it is allowed to control your Zode.

**Out of scope:** someone who already has your unlocked machine.

## What the relay can and cannot do

The relay is zero-knowledge. It forwards frames between the two ends of a
session and does not parse what is inside them.

| It can see                              | It cannot do                                                                      |
| --------------------------------------- | --------------------------------------------------------------------------------- |
| Which account and device connected      | Read terminal output, files, paths or keystrokes                                  |
| Which host a client asked for           | Forge or alter a message: any tampering closes the session for good               |
| Frame lengths and when frames were sent | Replay a message into a live session: it is refused                               |
| That a device was revoked or is offline | Open a session to a device that does not hold the pinned key; the handshake fails |

The channel is Noise KK: both static keys are known to each side before the
handshake, because pairing pinned them. A relay that does not hold them cannot
complete it, and a handshake made for one account and pair of devices does not
open under another.

**Tests that hold this.**

| Claim                                                            | Test                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A stranger without the pinned keys cannot finish a handshake     | `a_stranger_who_knows_no_pinned_key_cannot_complete_the_handshake` (`crates/remote_relay_protocol`)                                                                                                                                  |
| A tampered message ends the session                              | `a_tampered_message_closes_the_session` (`crates/remote_relay_protocol`); `a_tampered_message_closes_the_channel_for_good` (`crates/remote_relay_client`)                                                                            |
| A replayed message is refused                                    | `a_replayed_message_is_refused` (`crates/remote_relay_protocol`)                                                                                                                                                                     |
| Weak (low-order) keys are refused                                | `a_low_order_key_is_refused_as_a_pinned_key_or_an_ephemeral_key` (`crates/remote_relay_protocol`)                                                                                                                                    |
| The host acts on nothing until the peer proves it has the keys   | `a_session_that_never_proves_the_keys_ends_even_when_the_idle_limit_is_off` (`crates/remote_control`)                                                                                                                                |
| The relay forwards bytes untouched and logs sizes, never content | `delivers 1 MiB of random bytes byte for byte, both ways, and logs sizes only` (`web/backend/test/remote-relay.e2e-spec.ts`); `delivers binary frames byte for byte in both directions` (`web/backend/src/remote/relay-hub.spec.ts`) |
| Pairing messages are forwarded untouched                         | `forwards pairing messages untouched` (`web/backend/test/remote-relay.e2e-spec.ts`)                                                                                                                                                  |
| One account cannot reach another account's host                  | `never lets one account reach another account's host` (`web/backend/test/remote-relay.e2e-spec.ts`)                                                                                                                                  |

## If something goes wrong

| Scenario                        | What happens                                                                                                                                                                                     |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The relay is malicious          | It sees metadata only. It can refuse service, drop or delay frames, or close sessions. It cannot read or forge content, and cannot pair a new device without a human confirming on your machine. |
| The relay is man-in-the-middled | The attacker is in the position of the relay, and gets the same nothing. Keys were pinned at pairing; a handshake without them fails.                                                            |
| A paired device is stolen       | The thief can use it as you. Revoke it ([below](#if-a-device-is-lost)). Until you do, assume a shell on your machine.                                                                            |
| Your account is compromised     | An attacker can list your devices and open the relay, but cannot control a Zode without pairing, and pairing needs you to confirm on that Zode.                                                  |

A controller Zode also checks that the key a host presents matches the key
your account lists for it, and refuses to pair if it does not.

## Pairing security

Pairing is where a new device becomes trusted, so it is built to resist an
attacker who wants to be paired.

- **Commit, then reveal.** The requester commits to a nonce before the
  acceptor's is known. A machine in the middle cannot grind keypairs until the
  six digits match, because the digits are fixed before it can choose. The
  details are in [Pairing](./remote-control-protocol.md#pairing).
- **Six digits is small.** The comparison is the security boundary. If you
  confirm on A without looking at both screens, you pair whoever asked. Never
  confirm a prompt you did not start.
- **Nothing is pinned until you say the digits match**, and a host that declines
  leaves nothing pinned.
- **A pins when you confirm on A.** Choosing **Digits differ** on the other
  device afterwards does not undo that: the relay could hold that message back,
  so A does not wait for it. If you confirmed on A by mistake, remove the device
  under **Manage trusted devices...**.
- **Lockouts.** A takes one pairing at a time, a request expires after 120
  seconds, and it takes at most three requests an hour. Three failed attempts
  lock pairing, and the lock does not expire: you unlock it explicitly under
  **Manage trusted devices...**.

| Claim                                                           | Test                                                                                                         |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| A reveal that does not match its commitment is refused          | `a_reveal_that_does_not_match_the_commitment_is_refused` (`crates/remote_relay_protocol`)                    |
| A swapped requester key fails the commitment                    | `a_swapped_requester_key_fails_the_commitment` (`crates/remote_relay_protocol`)                              |
| Repeated failures lock pairing until the user resets it         | `repeated_failures_lock_pairing_until_the_user_resets_it` (`crates/remote_relay_protocol`)                   |
| Three mismatches lock pairing on the host                       | `a_mismatched_commitment_ends_pairing_and_three_of_them_lock_it` (`crates/remote_control`)                   |
| A lockout does not expire; only unlocking ends it               | `a_lockout_does_not_expire_and_only_unlock_ends_it` (`crates/remote_control`)                                |
| A lockout survives the connection dropping                      | `a_lockout_survives_the_connection_dropping_and_coming_back` (`crates/remote_control`)                       |
| Nothing is pinned and no session opens without confirmation     | `nothing_is_pinned_and_no_session_opened_without_confirmation` (`crates/remote_relay_client`)                |
| A man in the middle that can finish the handshake is not pinned | `a_machine_in_the_middle_that_could_finish_the_handshake_is_still_not_pinned` (`crates/remote_relay_client`) |

## A paired device has a shell on A

Be clear about what you grant. A paired controller can type into your agents
and terminals, so it can run any command you can. A Zode that controls another
can also open its projects, and the host then runs a project server as your
user: whatever you can do on that machine, it can do. Neither the viewer limits
nor `private_files` change this: those restrict what the web file browser shows,
not what a shell can read.

### If a device is lost

1. On A, open **Manage trusted devices...** (status-bar indicator, or the
   `remote_control::ManageDevices` action) and choose **Forget** for that
   device. This ends its session at once; it cannot return without pairing again.
2. For a browser, also sign it out on the web devices page,
   `zodekit.site/account/devices`.
3. If in doubt, **Disconnect all devices** or **Turn off remote control**.

Revocation also wipes credentials on the revoked device itself: a device that
the relay tells has been revoked deletes its key and every pairing. An ordinary
network failure does not: a dropped connection is retried with backoff, and
only an explicit revocation deletes anything.

| Claim                                                            | Test                                                                                                                       |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Forgetting a device ends its session and it cannot come back     | `forgetting_a_device_ends_its_session_and_it_cannot_come_back` (`crates/remote_control`)                                   |
| A revocation removes the pin and the session                     | `the_relay_announcing_a_revoked_device_removes_its_pin_and_its_session` (`crates/remote_control`)                          |
| Only explicit revocation wipes pins and key                      | `only_an_explicit_device_revocation_wipes_the_pins_and_key` (`crates/remote_relay_client`)                                 |
| Revoking during an open project stops the project server         | `revoking_the_device_during_an_open_project_stops_the_project_server` (`crates/remote_control`)                            |
| The relay closes a revoked device's sockets                      | `closes an editor's live socket when the device is revoked, within a second` (`web/backend/test/remote-relay.e2e-spec.ts`) |
| Trusting a host to control says nothing about it controlling you | `hosts_pinned_for_control_never_become_devices_allowed_to_control` (`crates/remote_relay_client`)                          |

## Off means off

With `remote_control.enabled` false, Zode opens no connection for remote
control and taps no terminal. Turning it off while sessions are open takes down
every connection, tap and project server.

| Claim                                                    | Test                                                                                         |
| -------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| It ships off, with a 30-minute idle limit                | `remote_control_ships_off_with_a_thirty_minute_idle_limit` (`crates/remote_control`)         |
| While off, no socket is opened and no terminal is tapped | `while_disabled_no_socket_is_opened_and_no_terminal_is_tapped` (`crates/remote_control`)     |
| Switching off disconnects at once and stays off          | `turning_it_off_from_the_action_disconnects_at_once_and_stays_off` (`crates/remote_control`) |
| Turning it off stops the project server                  | `turning_remote_control_off_stops_the_project_server` (`crates/remote_control`)              |

## Where this is weaker than you might assume

- **Metadata is visible.** The relay knows when you connect, from which device,
  to which host, and how much you send.
- **A paired device is a shell**, as above. The pairing prompt says so; the
  point of this page is that it is not hidden.
- **A browser key can be used by script on the page.** It is stored
  non-extractable, so it cannot be copied away, but a script running in that
  origin can still use it while it runs. Pair only browsers you trust.
- **Availability is the relay's.** A hostile relay can refuse service.
- **Agents are separate programs.** Remote control does not sandbox them.
