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

**One thing a hostile relay can force.** A device that is told it was revoked
deletes its pairings and its key, and what tells it is the relay or the account
service. A hostile one can say so falsely. Zode believes two exact answers only:
a `403` from the account service whose JSON `error` is `device_revoked`, or the
relay closing the connection with code 4403 and the reason `device_revoked`.
Anything else wipes nothing, including a different error text and a 4403 with
another reason. The cost is a re-pair: the device is left with no trusted
devices and no key. It gives no one access.

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
| Only the exact revoked answer wipes a device                     | `only_the_exact_code_and_reason_say_the_device_was_revoked`, `only_a_body_whose_error_is_device_revoked_means_revoked` (`crates/remote_relay_client`)                                                                                |

## If something goes wrong

| Scenario                        | What happens                                                                                                                                                                                     |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The relay is malicious          | It sees metadata only. It can refuse service, drop or delay frames, or close sessions. It cannot read or forge content, and cannot pair a new device without a human confirming on your machine. |
| The relay is man-in-the-middled | The attacker is in the position of the relay, and gets the same nothing. Keys were pinned at pairing; a handshake without them fails.                                                            |
| A paired device is stolen       | The thief can use it as you. Revoke it ([below](#if-a-device-is-lost)). Until you do, assume a shell on your machine.                                                                            |
| Your account is compromised     | An attacker can list your devices and open the relay, but cannot control a Zode without pairing, and pairing needs you to confirm on that Zode.                                                  |

A malicious relay or account service can also tell a device that it was
revoked, which makes the device delete its pairings and key. See
[above](#what-the-relay-can-and-cannot-do) for the only answers that count.

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
- **A pins when you confirm on A.** Saying the digits differ on the other
  device afterwards does not undo that: the relay could hold that message back,
  so A does not wait for it. If you confirmed on A by mistake, remove the device
  under **Manage Trusted Devices…**.
- **Lockouts.** A takes one pairing at a time and a request expires after 120
  seconds. It asks you at most three times an hour, and the allowance counts
  questions that were actually shown to you, not requests. A request abandoned
  before you were asked neither holds pairing nor spends any of the three. Three
  failed attempts lock pairing, and the lock does not expire: you unlock it
  explicitly under **Manage Trusted Devices…**, with **Unlock Pairing**.
- **Answered requests are capped too.** Abandoning a request costs you nothing,
  but answering it lets the requester learn the digits before it reveals, so a
  relay could otherwise try codes without limit by closing every exchange whose
  digits it did not like. Zode therefore also counts requests it answered,
  whether or not you were asked afterwards: at most three an hour for one
  requesting device and twelve an hour for all of them. A relay can invent
  device ids, so the overall cap is the one that bounds guessing; the
  per-device cap is for a device that is merely broken. A request over either
  cap is turned away, and the message says roughly how long to wait.
- **Turning remote control off and on resets nothing.** The lockout and the
  hourly counts are kept for the signed-in account while remote control is off;
  only signing in as a different account starts them fresh.
- **What counts as a failed attempt.** A reveal that does not match its
  commitment, and **Reject**. A question that goes away without an answer, such
  as one another dialog replaces, turns the device away but is not a failed
  attempt: digits nobody compared did not fail to match.
- **You are told when a request is turned away.** Zode shows a message when it
  refuses a request because the hourly allowance is used up, because too many
  requests were answered this hour (the message says someone may be trying to
  guess a pairing code), because pairing is locked, or because the list of
  trusted devices is full (32 devices) or cannot be read. That last check happens before you are shown any digits, so you never
  compare digits for a device that cannot be saved.
- **Pairing a device again replaces it.** If the device is already trusted, the
  prompt says so and says that trusting it replaces what is saved for it. When
  the key is new, confirming also ends every session that device had under the
  old key.
- **Names are cleaned.** A device's name comes from the account service, which
  every device on your account can write to. Before Zode shows one it removes
  control characters and the characters that reorder text, and keeps at most 64
  characters. A name with nothing left becomes a generic one.

| Claim                                                           | Test                                                                                                                                                                                  |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A reveal that does not match its commitment is refused          | `a_reveal_that_does_not_match_the_commitment_is_refused` (`crates/remote_relay_protocol`)                                                                                             |
| A swapped requester key fails the commitment                    | `a_swapped_requester_key_fails_the_commitment` (`crates/remote_relay_protocol`)                                                                                                       |
| Repeated failures lock pairing until the user resets it         | `repeated_failures_lock_pairing_until_the_user_resets_it` (`crates/remote_relay_protocol`)                                                                                            |
| Three mismatches lock pairing on the host                       | `a_mismatched_commitment_ends_pairing_and_three_of_them_lock_it` (`crates/remote_control`)                                                                                            |
| A lockout does not expire; only unlocking ends it               | `a_lockout_does_not_expire_and_only_unlock_ends_it` (`crates/remote_control`)                                                                                                         |
| A lockout survives the connection dropping                      | `a_lockout_survives_the_connection_dropping_and_coming_back` (`crates/remote_control`)                                                                                                |
| Nothing is pinned and no session opens without confirmation     | `nothing_is_pinned_and_no_session_opened_without_confirmation` (`crates/remote_relay_client`)                                                                                         |
| A man in the middle that can finish the handshake is not pinned | `a_machine_in_the_middle_that_could_finish_the_handshake_is_still_not_pinned` (`crates/remote_relay_client`)                                                                          |
| A request abandoned before you were asked costs nothing         | `a_request_abandoned_before_anyone_was_asked_neither_holds_pairing_nor_spends_the_allowance` (`crates/remote_control`)                                                                |
| A question closed unanswered is not a failed attempt            | `closing_the_question_unanswered_is_not_a_failed_guess` (`crates/remote_control`); `a_question_replaced_by_another_modal_does_not_count_toward_the_lockout` (`crates/remote_control`) |
| You are told when the allowance is spent                        | `once_the_allowance_is_spent_the_person_is_told_why_nothing_was_asked` (`crates/remote_control`)                                                                                      |
| A full trust store is reported before you compare digits        | `a_full_trust_store_is_said_before_the_person_compares_digits` (`crates/remote_control`)                                                                                              |
| Re-pairing with a new key ends the sessions made with the old   | `re_pairing_a_device_with_a_new_key_ends_the_sessions_made_with_the_old_one` (`crates/remote_control`)                                                                                |
| A device name arrives cleaned and short                         | `names_from_the_directory_arrive_clean_and_short` (`crates/remote_relay_client`)                                                                                                      |

## A paired device has a shell on A

Be clear about what you grant. A paired controller can type into your agents
and terminals, so it can run any command you can. A Zode that controls another
can also open its projects, and the host then runs a project server as your
user: whatever you can do on that machine, it can do. Neither the viewer limits
nor `private_files` change this: those restrict what the web file browser shows,
not what a shell can read.

A paired browser can also resume and fork your past agent sessions. It can list
the sessions of the folders open on this Zode, with their titles and the start of
each conversation, and start one again. That starts another agent on this
machine, in the folder the session ran in, under the permission setting this
machine has for it. A device cannot change that setting, so if you chose **Run
Agents Without Permission Prompts** for the checkout a session ran in, a session
a device starts there runs without prompts too. The device already had a shell,
so this does not widen what it can do, but it does mean your session history is
visible to it.

The reach is limited to the open projects. Sessions outside the folders open on
this Zode, and sessions under a `private_files` path or a path hidden by
`file_scan_exclusions`, are not listed and cannot be started. A device names a
session by its id alone; the host looks up everything else itself. A device may
start one session at a time and no more than one every 3 seconds, and each time
a toast on this screen names the device and the session.

| Claim                                                                    | Test                                                                                                          |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| A session under a private or excluded path is neither listed nor started | `a_session_under_a_private_or_excluded_path_is_neither_listed_nor_opened` (`crates/remote_control`)           |
| A session outside the open folders, or unknown, starts nothing           | `a_session_the_host_does_not_hold_or_may_not_open_is_not_found_and_opens_nothing` (`crates/remote_control`)   |
| A name that cannot be a session id is refused before any lookup          | `a_resume_naming_something_that_cannot_be_an_id_is_not_found_and_the_session_lives` (`crates/remote_control`) |
| One session is opened at a time for a device, across its connections     | `one_device_has_one_session_opened_at_a_time_however_many_connections_it_has` (`crates/remote_control`)       |
| A session opened a moment ago blocks the next until the cooldown passes  | `a_session_opened_a_moment_ago_cannot_be_opened_again_until_the_cooldown_passes` (`crates/remote_control`)    |

### If a device is lost

1. On A, open **Manage Trusted Devices…** (account menu, status-bar indicator,
   or the `remote_control::ManageDevices` action) and choose **Forget** for that
   device. This ends its session at once; it cannot return without pairing again.
   It works while remote control is off, so you can forget a device before you
   turn remote control back on.
2. For a browser, also sign it out on the web devices page,
   `zodekit.site/account/devices`.
3. If in doubt, **Disconnect All Devices** or **Turn Off Remote Control**.

Revocation also wipes credentials on the revoked device itself: a device that
is told it has been revoked deletes its key and every pairing. Only the two
exact answers [described above](#what-the-relay-can-and-cannot-do) count. An
ordinary network failure does not: a dropped connection is retried with
backoff, and a sign-in that ended for another reason stops remote control until
you sign in again but deletes nothing.

**Forget**, a device the relay reports as revoked, and the wipe on a revoked
device each show a message if the change could not be saved, instead of looking
done. A forget that was not saved can leave the device trusted again after Zode
restarts.

Forgetting a host from the controlling Zode ends every live session with that
host as well.

The device key lives in your keychain once per release channel. Stable keeps
`zode://remote-device-key/{user_id}`; the other channels use
`zode://remote-device-key-{channel}/{user_id}`, where `{channel}` is `dev`,
`nightly` or `preview`. A development build that is revoked therefore never
deletes the key of the app you installed.

| Claim                                                            | Test                                                                                                                                 |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Forgetting a device ends its session and it cannot come back     | `forgetting_a_device_ends_its_session_and_it_cannot_come_back` (`crates/remote_control`)                                             |
| A revocation removes the pin and the session                     | `the_relay_announcing_a_revoked_device_removes_its_pin_and_its_session` (`crates/remote_control`)                                    |
| Only explicit revocation wipes pins and key                      | `only_an_explicit_device_revocation_wipes_the_pins_and_key` (`crates/remote_relay_client`)                                           |
| Revoking during an open project stops the project server         | `revoking_the_device_during_an_open_project_stops_the_project_server` (`crates/remote_control`)                                      |
| The relay closes a revoked device's sockets                      | `closes an editor's live socket when the device is revoked, within a second` (`web/backend/test/remote-relay.e2e-spec.ts`)           |
| Trusting a host to control says nothing about it controlling you | `hosts_pinned_for_control_never_become_devices_allowed_to_control` (`crates/remote_relay_client`)                                    |
| A sign-in that ended for another reason deletes nothing          | `a_revoked_sign_in_halts_remote_control_but_keeps_the_key_and_the_pins` (`crates/remote_control`)                                    |
| Devices can be listed and forgotten while remote control is off  | `trusted_devices_are_listed_and_forgotten_while_remote_control_is_off` (`crates/remote_control`)                                     |
| A forget that cannot be saved says so                            | `a_forget_that_cannot_be_saved_says_so_instead_of_looking_done` (`crates/remote_control`)                                            |
| Forgetting a host ends the sessions open to it                   | `forgetting_a_host_ends_the_sessions_open_to_it` (`crates/remote_relay_client`)                                                      |
| Each release channel has its own key entry                       | `no_two_channels_share_a_keychain_entry`, `removing_one_channels_key_leaves_the_other_channels_alone` (`crates/remote_relay_client`) |

## Off means off

With `remote_control.enabled` false, Zode opens no connection for remote
control and taps no terminal. Turning it off while sessions are open takes down
every connection, tap and project server. Quitting Zode does the same: sessions
end and project servers stop, so neither a project server nor the terminals a
device started in it outlive Zode. It does not change the `enabled` setting.

| Claim                                                    | Test                                                                                                                        |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| It ships off, with a 30-minute idle limit                | `remote_control_ships_off_with_a_thirty_minute_idle_limit` (`crates/remote_control`)                                        |
| While off, no socket is opened and no terminal is tapped | `while_disabled_no_socket_is_opened_and_no_terminal_is_tapped` (`crates/remote_control`)                                    |
| Switching off disconnects at once and stays off          | `turning_it_off_from_the_action_disconnects_at_once_and_stays_off` (`crates/remote_control`)                                |
| Turning it off stops the project server                  | `turning_remote_control_off_stops_the_project_server` (`crates/remote_control`)                                             |
| Quitting turns it off and stops the project server       | `quitting_zode_turns_remote_control_off`, `quitting_zode_stops_the_project_server_and_its_daemon` (`crates/remote_control`) |

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
