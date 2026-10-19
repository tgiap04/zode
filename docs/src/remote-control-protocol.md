# Remote control — protocol

This page is the whole of it. Everything Zode and a browser exchange when you
control a running Zode from another device is described here, in enough detail
to write a compatible client without reading Zode's source — and the fixed
vectors at the bottom are checked by Zode's own test suite, so a version of this
page that disagrees with the code fails the build rather than misleading you.

The point of writing it down is narrow: a relay server sits between your browser
and your editor, and it is not a party you should have to trust. It forwards
bytes it cannot read. What it can see is who connected, to which device, how
many bytes, and when. That is the list.

## Layers

```
browser  ◄──── WebSocket ────►  relay  ◄──── WebSocket ────►  Zode
   │                                                            │
   └──────────── Noise KK channel (end to end) ─────────────────┘
                  carries: control messages, data streams
```

| Layer            | Seen by relay   | Defined in                              |
| ---------------- | --------------- | --------------------------------------- |
| Relay frame      | yes             | [Relay frame](#relay-frame)             |
| Noise transport  | ciphertext only | [Encrypted channel](#encrypted-channel) |
| Inner frame      | no              | [Inner frame](#inner-frame)             |
| Control and data | no              | [Control messages](#control-messages)   |

## Relay frame

A WebSocket **text** frame is a JSON object the relay itself understands. Its
shape belongs to the relay's own API and is not described here, with one
exception: the pairing messages below travel in text frames, and the relay
forwards them without interpreting them.

A WebSocket **binary** frame is:

```
offset 0   session_id   u32, big-endian
offset 4   payload      1 to 65535 bytes — opaque to the relay
```

`session_id` tells the relay which of its two peers' conversations the bytes
belong to. The payload is a Noise handshake message or a Noise transport
message. A frame shorter than 5 bytes, or with a payload over 65535 bytes, is
malformed and closes the connection.

## Relay roles

The relay gives every WebSocket one of two roles, and a session is always
between one host and one client of the same account.

- **Host.** The Zode being controlled. Connects with its access token as a
  bearer, and nothing else in the URL, or with `role=host`.
- **Client.** The device that controls. A browser presents a single-use ticket.
  A Zode presents its access token as a bearer **and** `?role=client` on the
  relay URL.

The rules around the role are the relay's contract:

- `role` is read on the bearer path only. Absent or `host` means host; any other
  value — empty, wrong case, repeated — is refused before the upgrade with HTTP
  400 and the error `invalid_role`.
- A client socket is greeted with `hello` carrying `role` `client`, then a
  `presence` list of the account's hosts that are connected, which does not
  include the client's own device.
- A Zode checks the `role` in that `hello` against the role it asked for. A
  `hello.role` that does not match the socket's role stops the client as an
  incompatible relay, as an unknown `relay_protocol` does. Without the check, a
  relay that ignored `?role=` could register a client socket as the device's
  host and take the host slot from the real host.
- A client opens a session with `open`, naming a host's device id and a `mode`
  of `pair` or `session`. Opening to its own device id is answered with `error`
  `unauthorized`.
- The host is told `opened` with the client's device id and `peerKind` `ide` for
  a Zode, `web` for a browser.
- Peers are keyed by user, device **and role**, so one Zode can be a host and a
  client at once. A second socket in the same role replaces the first, which is
  closed with code 4409.
- Revoking a device closes both its host and its client sockets with code 4403
  and the reason `device_revoked`.
- In the device list, `online` means "can be connected to as a host": an editor
  that holds only a client socket is listed as offline.

## Encrypted channel

The channel is `Noise_KK_25519_AESGCM_SHA256`:

- **KK** — both sides already know the other's static public key. Pairing
  established it, so the handshake is two messages and authenticates both ends.
- **25519** — X25519 for Diffie–Hellman.
- **AESGCM** — AES-256-GCM, with the Noise nonce: four zero bytes followed by
  the 64-bit message counter, big-endian.
- **SHA256** — SHA-256 for hashing and HKDF.

The **initiator is the controlling device** — the browser, or another Zode
(see [Controlling Zode](#controlling-zode)); the responder is the Zode being
controlled.

```
  <- s                       (known before the handshake)
  -> s
  ...
  -> e, es, ss               message 1, may carry a payload
  <- e, ee, se               message 2, may carry a payload
```

Both pre-message keys are mixed into the transcript hash in that order:
initiator static first, then responder static.

### Prologue

```
"zode-remote/1" ‖ 0x1F ‖ user_id ‖ 0x1F ‖ initiator_device_id ‖ 0x1F ‖ responder_device_id
```

Fields are UTF-8. `0x1F` (ASCII unit separator) rather than a printable
character because no id can contain it, so no two distinct triples assemble the
same prologue. An empty field, or one containing `0x1F`, is refused.

The prologue is mixed into the transcript hash before anything else, so a
handshake made for one account and pair of devices does not open under another:
the first message fails to authenticate. A relay that tries to splice a
handshake meant for someone else's device into yours gets nothing.

### Handshake rules

- A handshake payload is at most 65487 bytes: the 65535-byte message limit less
  the 32-byte ephemeral key and the 16-byte tag.
- A handshake that fails to authenticate is dead. It is dropped, not retried on
  the same state.
- A public key — pinned, or an ephemeral key at the start of a handshake
  message — that is all zeros or any other small-order Curve25519 point is
  refused. Such a key makes the Diffie–Hellman result something an observer can
  compute. The check ignores the high bit, which X25519 ignores too.
- A responder drops a handshake that has not completed within 10 seconds, and
  holds at most 8 half-open handshakes at once. A relay can start handshakes on
  a device's behalf for free, so without a cap each one is memory the relay gets
  to spend.

### Transport

After the handshake each side holds two keys, one per direction. A message is
the AES-GCM sealing of its plaintext, with empty associated data and the next
counter value as nonce, giving `ciphertext ‖ 16-byte tag`.

- **A responder must not act on a session until it has decrypted the first valid
  transport message** — in practice `hello`. Handshake message 1 is not fresh: a
  relay that recorded it can replay it, and the responder cannot tell the
  difference until the peer proves it holds the derived keys by sending
  something under them. Until then the responder does not open a terminal,
  list a file, or answer a request. The initiator has no such wait, because
  message 2 carries a fresh ephemeral key and so cannot be a replay.
- Counters are implicit, so messages must arrive in order. The relay's single
  WebSocket per peer provides that.
- A message that fails to authenticate **closes the session for good**. There
  is no skipping a bad message and carrying on.
- A session ends after 2^32 messages in one direction. The peers run a fresh
  handshake instead of rekeying. A caller that holds a whole stream before
  sending it can ask for it to be sealed at once (`encrypt_data_chunks` in
  `remote_relay_protocol`); a stream that would not fit in the messages left is
  then refused whole, before any of it is sent, and the session is closed. Zode's
  relay client and host do not do that. They seal frame by frame as bytes
  arrive, so running out of messages ends the session partway through a stream,
  which the peer sees as the session closing and not as a stream that ended
  cleanly.
- Noise caps a message at 65535 bytes. Plaintext per message is held to 61440
  bytes (60 KiB), header included.

## Inner frame

The plaintext of each transport message:

```
offset 0   kind         u8      0 = control, 1 = data
offset 1   stream_id    u32, big-endian
offset 5   payload      0 to 61435 bytes
```

- **Control** frames always use `stream_id` 0 and carry one JSON message. An
  empty control payload is malformed.
- **Data** frames use a non-zero `stream_id` chosen by whoever opens the stream.
  An **empty** data payload marks the end of that stream.
- Bulk content longer than one frame is cut into several data frames on the same
  stream. Frames are never fragmented across Noise messages.

## Control messages

UTF-8 JSON, tagged by `t`. A field a peer does not recognise is **ignored**, so
a newer peer can add to a message without breaking an older one. A `t` nobody
recognises is an error: guessing what an unknown message meant is how a request
gets acted on twice or not at all.

Requests carry a `request_id` chosen by the sender, and the reply repeats it.
File contents, diffs and terminal output do not travel in a control message but
in data frames on the `stream_id` the reply names.

| `t`                   | Fields                                                                              | Direction         |
| --------------------- | ----------------------------------------------------------------------------------- | ----------------- |
| `hello`               | `relay_protocol`, `app_version`, `rpc_protocol`, `capabilities`                     | browser → Zode    |
| `hello_ack`           | same as `hello`                                                                     | Zode → browser    |
| `error`               | `code`, `message`, optional `request_id`                                            | either            |
| `ping` / `pong`       | none                                                                                | either            |
| `agent_list`          | `agents`: `[{id, name, status, title?, session_id?, agent?}]`                       | Zode → browser    |
| `agent_update`        | `agent`: `{id, name, status, title?, session_id?, agent?}`                          | Zode → browser    |
| `terminal_list`       | `terminals`: `[{id, title, columns, rows}]`                                         | Zode → browser    |
| `terminal_attach`     | `terminal_id`, `stream_id`                                                          | browser → Zode    |
| `terminal_attached`   | `terminal_id`, `stream_id`, `columns`, `rows`                                       | Zode → browser    |
| `terminal_detach`     | `terminal_id`                                                                       | browser → Zode    |
| `terminal_resized`    | `terminal_id`, `columns`, `rows`                                                    | either            |
| `terminal_closed`     | `terminal_id`, optional `exit_code`                                                 | Zode → browser    |
| `ide_open`            | `request_id`, `path`, optional `line`, `stream_id`, `app_version`, `proto_version`  | controller → Zode |
| `ide_opened`          | `request_id`, `stream_id`, `path_style`, `shell`, `default_shell`                   | Zode → controller |
| `files_list`          | `request_id`, optional `worktree_id`, `path`                                        | browser → Zode    |
| `files_list_reply`    | `request_id`, `entries`: `[{name, kind, size?, worktree_id?}]`, `truncated`, `more` | Zode → browser    |
| `file_read`           | `request_id`, `worktree_id`, `path`                                                 | browser → Zode    |
| `file_read_reply`     | `request_id`, `size`, `stream_id`                                                   | Zode → browser    |
| `diff_request`        | `request_id`, `worktree_id`                                                         | browser → Zode    |
| `diff_reply`          | `request_id`, `stream_id`, `truncated`                                              | Zode → browser    |
| `sessions_list`       | `request_id`, optional `before`, `limit`                                            | browser → Zode    |
| `sessions_list_reply` | `request_id`, `sessions`: `[entry]`, `more`, optional `next`                        | Zode → browser    |
| `session_resume`      | `request_id`, `session_id`, `fork`                                                  | browser → Zode    |
| `session_resumed`     | `request_id`, optional `open_agent_id`                                              | Zode → browser    |

`status` is one of `working`, `waiting_for_input`, `idle`, `finished`,
`failed`; a value a reader does not know is read as unknown rather than
failing the whole list. `kind` is one of `file`, `directory`, `symlink`, and
likewise degrades to `other`.

An agent summary may also carry `session_id`, the agent's own id for the
conversation the tab is writing, when the tab has one, and `agent`, which is
`claude`, `codex`, `copilot` or `opencode` and is left out for a custom agent. A
tab opened as a fork has no `session_id`. A reader that does not know the two
fields ignores them.

`capabilities` in `hello_ack` lists what the host serves: `terminal` always,
then `files`, `ide` and `sessions` in that order, each only when it is served. A
request whose capability is missing is answered with `error` code
`unsupported`.

After `terminal_attach`, terminal output flows Zode → browser and keystrokes
browser → Zode as data frames on the attach's `stream_id`, until
`terminal_detach` or `terminal_closed`.

The first bytes on a terminal stream are a snapshot of the screen: a full reset
(`ESC c`), the scrollback and screen redrawn, then the terminal's modes
(including the kitty keyboard flags the program asked for), the cursor, and the
pending-wrap state. It also carries what a program that redraws only what
changed depends on: the scroll region, the cursor saved with `ESC 7` along with
its attributes and character sets, and the character sets in use with the one
shifted in. After it, the raw output follows, and the two never overlap:
output the snapshot already drew is not sent again. Treat a stream as **bytes**,
not text: a snapshot larger than one frame is cut into frames at arbitrary
offsets, which may fall inside a UTF-8 character or an escape sequence, and the
same goes for live output.

A new snapshot (starting with `ESC c` again) replaces the screen whenever the
host's size changes, a slow reader fell too far behind, output was lost, the
program leaves the alternate screen, or a settle snapshot (below) is due; at
most one every 250 ms per terminal and device. The spacing holds across a
detach and a re-attach too: a device that attaches again is owed a snapshot no
sooner than one that stayed.

Two of those need a word:

- **Leaving the alternate screen.** A snapshot taken while a program is on the
  alternate screen carries that screen and not the primary one behind it, so the
  device has nothing to return to when the program leaves. The host sends a
  fresh snapshot then, to each device that was sent such a snapshot.
- **A slow reader.** When live output overflows a device's queue, the queue is
  dropped and a fresh snapshot is wanted. A snapshot that is already being sent
  is delivered whole, and the fresh one follows it: cutting it off would leave a
  device that cannot keep up with a blank screen for as long as the output
  lasts.

Not carried: underline colour, hyperlinks, the primary screen behind an
alternate one, and whether the saved cursor was waiting to wrap. Of the
character sets, only ASCII and line drawing exist on the host, so only those are
carried.

Limitation: the host's emulator sees bytes before it has drawn them. A snapshot
taken while a program is in the middle of a synchronized update (DEC mode 2026)
or an escape sequence can stand for output the screen does not show yet. When a
snapshot is taken while output is flowing, or while a synchronized update is
open, the host therefore sends one more, the settle snapshot, which repairs it:

- It is sent once the output has been quiet for 400 ms. Every new chunk of
  output restarts that wait, but it never runs past 2 s after the snapshot it
  repairs.
- If a synchronized update is still open when it is due, the host looks again
  every 50 ms, up to 20 times (about a second), and then sends it anyway, since
  a program that exits inside an update never ends it.
- A settle snapshot is not followed by another. A snapshot taken for another
  reason while one is waiting replaces it, and brings its own repeat only if it
  too was taken ahead of the screen.

A client should be ready for that second reset and draw it without flicker if it
can.

### Project server stream

A Zode that controls another one opens the other's project as if it were on
its own disk. The controlled Zode runs its project server for it and carries
that server's messages on one stream of the encrypted channel. A browser does
not use this.

- **Capability.** The host lists `ide` in `hello_ack` `capabilities` only when
  it can find a project server to run. Without it, `ide_open` is answered with
  `error` code `unsupported`.
- **Opening.** The controller sends `ide_open` with a `stream_id` it chooses
  (non-zero, not in use), the `app_version` it runs and the `proto_version` of
  the project server's own protocol. `path` is the folder it means to open, or
  empty while it has not chosen; the host does not restrict the server to it.
  A request missing `stream_id`, `app_version` or `proto_version`, or naming
  stream 0 or a stream from `0x80000000` up, is answered with `error` code
  `malformed` and the `request_id`.
- **Refused while earlier servers are going.** At most four project servers may
  be in the middle of being stopped at once. A device that opens a project
  while four are still on their way out is answered with `error` code
  `rate_limited` and the `request_id`, with the message "earlier project servers
  are still being stopped; try again shortly". Nothing is started, and the
  session carries on.
- **Answer.** `ide_opened` repeats `request_id` and `stream_id`, and tells the
  controller what it cannot learn from the stream before it has built a
  connection on it: `path_style` (`posix` or `windows`), the host's login
  `shell` and its `default_shell`.
- **Framing.** On the stream, bytes flow both ways until the stream ends. They
  are the project server's own framing, which this protocol does not interpret:
  each message is a 4-byte little-endian length followed by that many bytes.
  Treat the stream as bytes; a message may be cut across data frames. Neither the
  controller nor the project server reads a message longer than 256 MiB: a
  length over that ends the stream with an error. The host does read the
  controller's side far enough to tell a ping from a request, so that the idle
  limit can count requests and not the heartbeat. It never acts on what it
  reads, and the bytes go on to the server unchanged.
- **One per session.** A session has at most one project stream. A second
  `ide_open` while one is open is answered with `error` code `too_large` and the
  `request_id`; the first is untouched. After it ends, another may be opened.
- **Ending.** An empty data frame ends the stream from either side. The
  controller ending it stops the project server and every terminal it opened;
  the host ending it means the server exited. Neither ends the session: the
  mirror keeps working. Ending the session ends the stream the same way.
- **Stopping the server.** The host asks the server to stop, which makes it
  close its terminals first, and kills it if it is still there after 5 seconds.
  Where the system has `SIGTERM` that is the request. On Windows there is no
  such signal, so the host writes a `server.stop` file in the server's state
  directory, beside its pid file, and the server, which looks for the file every
  250 ms, stops and quits when it appears. A session that ends before the server
  has written its pid file is still stopped: the host waits up to 5 seconds for
  the file.
- **Terminals.** A project server holds at most 16 terminals for its controller
  at once, and a terminal that is still closing counts until its process has
  been collected. One more is refused with "too many terminals: at most 16 may
  be open at once".
- **Versions.** The project server's messages are only compatible between
  identical builds, and nothing is uploaded over the relay in this version. So
  `app_version` must equal the host's and `proto_version` must equal the host's
  project protocol version. If either differs, the host answers `error` code
  `version` with the `request_id`, in words that tell the person to update both
  Zodes to the same version, and starts nothing. The session carries on.
- **Who may ask.** Only a session that has already decrypted a valid message
  under its keys, and said `hello`, is acted on at all; the project server runs
  as the user of the controlled Zode and with no more privilege.

### Files

Read-only browsing of the folders open on the host. Nothing here writes, and no
program is run to produce a diff.

- **Capability.** The host lists `files` in `hello_ack` `capabilities` only when
  it can serve these requests. Without it they are answered with `error` code
  `unsupported` and the `request_id`.
- **Folders.** `files_list` without `worktree_id` lists the open folders: one
  entry each, with `worktree_id` set to the id to use afterwards (a decimal
  string). With a `worktree_id`, it lists the direct children of `path`, which is
  relative to that folder and uses `/`; an empty `path` is the folder itself.
  Entries come directories first, then by name without regard to case; `kind`
  is `file` or `directory`, `size` is set for files only, and an entry has no
  `worktree_id` below the roots. At most 2000 entries are listed, and no more
  than about 1.5 MiB of them once encoded; `truncated` says there were more.
- **Long listings.** A listing that does not fit one control message arrives
  as several `files_list_reply` messages with the same `request_id`, in order.
  Every one but the last has `more` set; `truncated` is the same on all of them.
  Append their `entries`.
- **Reading.** `file_read` needs `worktree_id` and `path`. The reply gives the
  file's `size` and a `stream_id`; the bytes follow in data frames on it, and an
  empty data frame ends the stream. Only UTF-8 text up to 1 MiB is sent: a file
  that is not text is answered with `error` code `binary`, a larger one with
  `too_large`. An empty file is a reply and an end of stream with no bytes
  between.
- **Diff.** `diff_request` needs `worktree_id`. The reply carries a `stream_id`;
  the unified diff of the working tree against `HEAD` (staged and unstaged
  changes, plus files git does not track yet) follows on it in the same way.
  Files git does not track yet and does not ignore are each shown as a new file,
  up to 100 of them. It covers only the files inside that folder, so a folder
  that is part of a larger repository does not show the rest of it, and leaves
  out whatever the folder does not show: private files and files the user's
  `file_scan_exclusions` hide, new files included. No external diff program and
  no textconv program is run. The repository's own git filters (a `clean`
  filter named in its attributes, say) still apply, as they do to any
  `git status` or `git diff` Zode runs. A changed submodule appears in its short
  form, the old and new commit, whatever the user's `diff.submodule` says. A
  changed path that is not valid UTF-8 is left out. At most 2 MiB is sent, cut at the end of a line. `truncated` says
  the diff is not all of the changes: it was cut, more than 100 new files
  exist, one could not be read, or 8 seconds into the read there were new files
  still to start. A folder that is not in a git repository, or
  a repository with no commit yet, is answered with `not_found`.
- **Time spent on a diff.** Git is given 20 seconds for one diff. If it takes
  longer, git is stopped and the request is answered with `internal`, which
  leaves room before the 30 seconds every request gets. A diff that nobody is
  waiting for any more, because the request timed out or the session ended, is
  stopped too, and one still waiting its turn is skipped, so it does not hold up
  the repository's other git work.
- **Streams the host opens.** The `stream_id` in a reply is chosen by the host
  and is at least `0x80000000`, so it never meets an id a browser picked for
  its own streams; a browser may not pick one that high for `terminal_attach`
  or `ide_open`, which are answered with `malformed`. Data frames are at most
  one frame long; the reply is sent before its first one.
- **What can be named.** A path is only looked up among the files the host has
  scanned in that folder, never on its disk. Anything else -- `..`, an absolute
  path, a path with `..` or doubled separators in it, a file the user's
  `file_scan_exclusions` hide, an unknown `worktree_id` -- is `not_found`. So
  is a file the user has marked private (the `private_files` setting) and
  everything inside a private folder: private files are never listed, read or
  shown in a diff. Symlinks are not served either: a symlink is not listed, and
  neither is anything reached through one, whether it points outside the folder
  or not. This is checked again when a file is read, against what is on disk
  then: a folder on the way to the file that has become a symlink leading out
  of the folder is not followed, and anything that is not an ordinary file, such
  as a named pipe or a socket, is `not_found`. Only folders the user can see are offered.
- **Limits.** A device may have four file requests being worked on at once, and
  at most 8 MiB of answers waiting for the relay. A request is let in only if
  the answers already waiting plus the largest answer (2 MiB) that each request
  being worked on, this one included, may yet produce still fit in 8 MiB; if
  not, it is answered with `rate_limited`. Those refusals, and the errors for
  requests that were accepted, are queued behind the control messages already
  waiting, so they arrive in order. Control messages are small and go ahead of
  file and diff frames: a reply does not wait behind a file, and a file's frames
  never pass the reply that names their stream. An error for a request that was never accepted --
  `unsupported`, or `malformed` for a missing `worktree_id` -- is sent at once
  and may arrive ahead of answers still waiting; its `request_id` says which
  request it answers. A device whose waiting answers have reached 8 MiB plus
  another 64 KiB is ignoring what it is told, and its session is ended when the
  next answer is queued for it. Only what the device asked for counts toward
  that. What the host pushes unasked, such as an agent's status or the list of
  terminals, never ends a session: a newer push replaces an older one about the
  same thing that is still waiting, and at most 256 pushes wait for a device. Ending a session cancels the requests still
  being worked on. A request that is not finished
  after 30 seconds is answered with `internal` and its place is freed. A
  request missing `worktree_id` where it is needed is `malformed`.

### Sessions

Lists the past agent conversations of the folders open on the host, and starts
one again for a device. The host's own record of each session decides which
agent it belongs to, where it ran and how it is resumed; the device names a
session by its id and nothing else.

A listing, then a resume of the session it shows as already running: the host
opens nothing and names the tab (`→` is browser to Zode, `←` the reply):

```text
→ {"t":"sessions_list","request_id":7}
← {"t":"sessions_list_reply","request_id":7,"sessions":[{"id":"0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f","agent":"claude","title":"Fix the terminal snapshot","preview":"Carry the scroll region","worktree_id":"3","folder":"zode/crates/terminal","branch":"main","updated_at":1760572800000,"can_fork":true,"open_agent_id":"terminal-42"}],"more":false,"next":{"updated_at":1760572800000,"id":"0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f"}}
→ {"t":"session_resume","request_id":9,"session_id":"0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f","fork":false}
← {"t":"session_resumed","request_id":9,"open_agent_id":"terminal-42"}
```

- **Capability.** The host lists `sessions` in `hello_ack` `capabilities` when
  it serves these requests. Without it, `sessions_list` and `session_resume` are
  answered with `error` code `unsupported` and the `request_id`. These messages
  came without a new `rpc_protocol`, so a client sends them only to a host that
  lists the capability.
- **Scope.** A session is listed only if the directory it ran in lies inside a
  visible folder of a local project open in a window on the host, matched by
  whole path components. A directory that climbs out with `..` belongs to no
  folder, and where open folders nest, the deepest one owns the session.
  `folder` is that folder's name followed by the path inside it, joined with
  `/`; a folder with no name of its own is called `root`. Absolute paths are
  never sent.
- **Hidden paths.** A session whose directory is at or under a path its folder
  marks `private_files`, or leaves out with `file_scan_exclusions`, is treated as
  not there: it is not listed, and resuming it is `not_found`. These are the two
  checks the file browser makes, taken from the settings of the folder that owns
  the directory.
- **Listing.** `sessions_list` answers with the sessions newest first: by
  `updated_at` descending, then `id` ascending. `limit` defaults to 50 and is
  held to 1 through 200. `before` is `{updated_at, id}`, copied from the `next`
  of an earlier reply, and the page starts after that entry. A request without
  `before` makes the host read the agents' session stores again first, unless a
  read finished less than 5 seconds ago, in which case it answers from that one.
  Later pages are read without another scan.
- **Long pages.** A page that does not fit one control message arrives as
  several `sessions_list_reply` messages with the same `request_id`, in order.
  Every one but the last has `more` set, and only the last carries `next`, and
  only when another page exists. Append their `sessions`.
- **Entries.** Each entry in `sessions` has these fields. `title`, `preview`,
  `folder` and `branch` are cut to 200 characters, the last being `…` when
  anything was cut.

| Field           | Meaning                                                                                      |
| --------------- | -------------------------------------------------------------------------------------------- |
| `id`            | The agent's own id for the session, which `session_resume` takes.                            |
| `agent`         | `claude`, `codex`, `copilot` or `opencode`.                                                  |
| `title`         | What the session is called. When that is empty, its `preview`.                               |
| `preview`       | The start of the conversation. Left out when empty.                                          |
| `worktree_id`   | The open folder it ran in, as `files_list` names it.                                         |
| `folder`        | The folder's name and the path inside it.                                                    |
| `branch`        | The branch the session recorded, when it recorded one.                                       |
| `updated_at`    | When the session was last updated, in Unix milliseconds.                                     |
| `can_fork`      | Whether `fork` may be set for it: true for Claude Code and opencode. Always sent.            |
| `open_agent_id` | The `id` in `agent_list` of a tab already running this session. Left out when there is none. |

- **Already open.** `open_agent_id` is set when a tab in `agent_list` has a
  `session_id` equal to the entry's `id` and its agent is running in it. A tab
  whose agent has exited does not count.
- **Trust.** `session_id` is checked for its shape before it is looked up: 1 to
  128 bytes of ASCII letters, digits, `.`, `_` and `-`, the first a letter or a
  digit, so it can never read as a flag or as `.` or `..`. The `id` inside
  `before` must have the same shape, and a session whose id does not is not
  listed. An id that does not fit is `not_found`.
- **Resuming.** `session_resume` with `fork` false, which is also the default,
  continues the session. If a tab already runs it, the host answers
  `session_resumed` with that tab's `open_agent_id` and opens nothing. Otherwise
  it opens a tab on the session in the window whose folder holds the directory,
  and answers `session_resumed` without `open_agent_id`. The tab is added after
  the answer and arrives like any other, as an `agent_update` whose
  `session_id` is the one asked for. A tab that is still starting its agent does
  not count as running yet, but it does hold its session: a second
  `session_resume` for that session opens nothing. Its answer is still a
  `session_resumed` without `open_agent_id`, and the tab that arrives is the one
  the first request opened.
- **Forking.** With `fork` true the host always opens a new tab, and the agent
  branches the conversation: Claude Code is started with
  `--resume <id> --fork-session` and opencode with `-s <id> --fork`. The agent
  chooses the id of the branch and does not report it, so the new tab's summary
  has no `session_id`. If the source has gone from the agent's store by the time
  the tab starts, the tab says the session is not available and does not open a
  blank one. `fork` on a session whose `can_fork` is false is `unsupported`.
- **Limits.** A `sessions_list` counts against the same allowance as a file
  request: four being worked on at once, and the 8 MiB of answers waiting for the
  relay, as described under Files. A device may have one `session_resume` being
  worked on at a time, counted across all its connections. After one that opened
  a tab has been answered, the next from that device is answered `rate_limited`
  for 3 seconds, whichever session it names and whatever its `fork`. The tab is
  added after the answer and an agent counts as running only once its process
  is, so until then `open_agent_id` is not set for it. An answer that only
  points at a running tab, and an error, open nothing and start no wait. A
  request not finished after 30 seconds is answered with `internal`.
- **Notice on the host.** When a session is opened for a device, the host shows
  a notice, `<device> resumed “<title>”` or `<device> forked “<title>”`, in the
  workspace that receives the tab and also in the one in front when that is a
  different one. The title is cut to 60 characters and the device name to 40.
  Each notice is separate, so a second does not replace the first, and each goes
  away after five seconds. An answer that only points at a running tab shows
  none.

Errors, in the `error` message with the `request_id`:

| Request          | `code`         | When                                                                                                                                                                         |
| ---------------- | -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| either           | `unsupported`  | The host does not serve sessions.                                                                                                                                            |
| `sessions_list`  | `not_found`    | `before.id` cannot be a session id.                                                                                                                                          |
| `sessions_list`  | `rate_limited` | The file and session allowance is used up.                                                                                                                                   |
| `sessions_list`  | `internal`     | The deadline passed.                                                                                                                                                         |
| `session_resume` | `not_found`    | The id cannot be a session id or names no session, the session is outside the open folders or under a private or excluded path, its directory is gone, or the window closed. |
| `session_resume` | `unsupported`  | The agent cannot resume it, or `fork` was set where `can_fork` is false.                                                                                                     |
| `session_resume` | `rate_limited` | A resume for this device is already being worked on, one was opened for it in the last 3 seconds, or its waiting answers are over the allowance.                             |
| `session_resume` | `internal`     | The deadline passed, or the host failed for another reason.                                                                                                                  |

A device that sends `sessions_list_reply` or `session_resumed` gets `malformed`:
they are answers, not requests.

### Errors

| `code`         | Meaning                                                    |
| -------------- | ---------------------------------------------------------- |
| `version`      | The peer speaks a protocol version this build does not.    |
| `malformed`    | The bytes did not parse as the message they claimed to be. |
| `unauthorized` | The sender may not use the device or resource it named.    |
| `not_found`    | The request names something that does not exist.           |
| `too_large`    | The request is over a size limit.                          |
| `binary`       | The file asked for is not UTF-8 text.                      |
| `rate_limited` | The sender is going faster than the receiver allows.       |
| `internal`     | The receiver failed for a reason that is not the sender's. |
| `unsupported`  | The receiver knows the request but does not serve it yet.  |

A receiver that meets a `code` it does not know treats it as a refusal of the
request that caused it. `version` is also the answer to an `ide_open` from a
build that differs; unlike a `hello` with an unknown `relay_protocol`, it does
not end the session.

### Versions

`relay_protocol` is `1`: the relay frame, the Noise parameters and the inner
frame. `rpc_protocol` is `1`: the control and data messages. A peer that
receives a `relay_protocol` it does not speak answers with `error` code
`version` and closes. It does not guess: reading a newer framing as an older one
means interpreting ciphertext as control messages.

## Pairing

Pairing teaches each device the other's static public key, and gives the two
people at the two screens a way to check that nobody in the middle swapped them.
Comparing a short number is the weak point of any such scheme, so it is built
so that grinding keypairs until the number matches does not work.

Roles: the **requester** is the browser, the **acceptor** is Zode. Keys are raw
32-byte X25519 public keys; nonces are 32 bytes from a CSPRNG. Messages are
JSON in relay text frames, with bytes encoded as standard padded base64.

```
requester                                   acceptor
   │  pair_request {public_key, commitment}    │
   │ ────────────────────────────────────────► │   commitment = SHA-256(pk_B ‖ n_B)
   │  pair_accept  {public_key, nonce}         │
   │ ◄──────────────────────────────────────── │   pk_A, n_A
   │  pair_reveal  {nonce}                     │
   │ ────────────────────────────────────────► │   n_B — acceptor checks the commitment
   │                                           │
   both compute SAS and show it
```

The acceptor recomputes `SHA-256(pk_B ‖ n_B)` from the key in the request and
the revealed nonce, and abandons pairing if it differs from the commitment. A
device refuses to pair with its own key.

**The short authentication string** is six digits:

```
digest = SHA-256("zode-pair/1" ‖ pk_A ‖ pk_B ‖ n_A ‖ n_B)
value  = (first 20 bits of digest, big-endian) mod 1000000
SAS    = value, zero-padded to six digits
```

`A` is the acceptor and `B` the requester regardless of which side computes it.

**Why a commitment.** A hash of the two public keys alone could be ground: a
machine in the middle generates keypairs until its two fake keys produce the
same six digits on both sides, about a million tries. With the requester
committing to its nonce first, the acceptor's nonce and key are fixed before
the requester's is revealed, so the digits are not something the attacker can
steer after the fact. Each guess then needs an answered request, which is
capped (below), and one wrong match fails the pairing visibly.

**Rules the acceptor enforces:**

- It holds **one pending pairing exchange at a time**. A second request while
  one is pending is refused as busy.
- A pending exchange **expires after 120 seconds** from the request. A reveal
  arriving later is refused, and a new request may then replace it.
- **After 3 failed attempts pairing is locked.** A failed attempt is a reveal
  that does not match the commitment, or digits the person says do not match. The
  lockout does not time out: only an explicit user action unlocks it, because a
  lockout that expires by itself only slows guessing down.
- **Answered requests are capped.** The acceptor counts each `pair_request` it
  answers with `pair_accept`, whether or not the person is asked afterwards:
  at most 3 an hour for one requesting device id and 12 an hour across all
  ids. Answering is what lets a requester learn the digits before it reveals,
  so without the cap a relay could close every exchange whose digits it did not
  like and try again at no cost to the person. A request over either cap is
  closed without an answer, and the person is told. A refused request does not
  count, and the count slides with the hour. Separately, the person is asked at
  most 3 times an hour; that count includes only questions actually shown.
- A reveal is one try. Whatever its outcome, the pending exchange ends.
- A public key that is all zeros, or any other small-order point, is refused in
  `pair_request` and `pair_accept`.

Once both people confirm the digits match, each side stores the other's public
key as a pinned static key for the Noise handshake.

### Controlling Zode

A Zode that controls another is the **requester** when pairing and the
**initiator** of the channel, in the roles the browser has. It follows the same
rules with one addition per step, because it is a device that holds a key it can
check against the account's device list:

- Nothing is sent after `pair_reveal`, and nothing is pinned, until the person
  at this screen has said the digits match. If they say they differ, the
  exchange is dropped.
- The public key the host presented while pairing must equal the key the
  account lists for that device. If it does not, pairing is refused; the digits
  are not even shown.
- After the confirmation, a `session` handshake with the pinned key must
  succeed, and the host must answer `hello`, before the host is pinned: the
  handshake is the second gate, and a key that cannot complete it is never kept.
- The hosts a Zode has paired with are stored separately from the devices it
  lets control it. Being trusted to control a Zode says nothing about whether
  that Zode may be controlled in return.

## Device keys in the browser

The browser's static key is generated by WebCrypto as `X25519` with
`extractable: false` and usage `deriveBits`, and stored as a `CryptoKeyPair` in
IndexedDB. Script on the page, including any compromised dependency, can ask
the key to perform a Diffie–Hellman but cannot read it out. Only the public key
is ever exported.

This stops exfiltration, not use. A page script that runs in the origin — a
cross-site scripting hole, a compromised dependency — can still ask the key to
perform Diffie–Hellman for as long as it runs, and so can still take part in a
handshake as this device. What non-extractability buys is that the key cannot be
copied away and used from somewhere else later. A browser without WebCrypto
X25519 is refused with a clear error rather than falling back to a library that
holds the key as bytes.

WebCrypto offers no ChaCha20-Poly1305, which is why the channel uses AES-GCM.
The Noise state machine is written out by hand in the browser on top of
WebCrypto's X25519, HMAC-SHA-256, SHA-256 and AES-GCM — existing JavaScript
Noise libraries take the private key as bytes, which would force it to be
extractable. The vectors below are produced by the Rust implementation (the
`snow` crate), and the browser must reproduce them byte for byte.

## What the relay sees

| Sees                                      | Does not see                      |
| ----------------------------------------- | --------------------------------- |
| Which account and device connected        | What is being controlled          |
| `session_id` and the length of each frame | Any message type or content       |
| When frames were sent                     | Any file, path or terminal output |

## Verifying this yourself

You do not have to trust this page.

- The vectors below are asserted against this file by the `remote_relay_protocol`
  test suite. Edit a digit here and Zode's tests go red.
- The browser keeps a copy of the same vectors as a fixture, checked by its own
  tests, so the two implementations cannot drift apart silently. `fixture.sha256`
  is the SHA-256 of the vector lines below, each as `name = value` followed by a
  newline, excluding the `fixture.sha256` line itself.

## Fixed vectors

The static keys are the X25519 test keys from RFC 7748 §6.1 (Alice as the
initiator, Bob as the responder). The ephemeral keys are fixed so the handshake
is reproducible; a real handshake never reuses one.

```text zode-remote-vectors
noise.protocol = Noise_KK_25519_AESGCM_SHA256
noise.user_id = user-0001
noise.initiator_device_id = device-browser
noise.responder_device_id = device-desktop
noise.prologue = 7a6f64652d72656d6f74652f311f757365722d303030311f6465766963652d62726f777365721f6465766963652d6465736b746f70
initiator.static.private = 77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a
initiator.static.public = 8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a
responder.static.private = 5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb
responder.static.public = de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f
initiator.ephemeral.private = 1111111111111111111111111111111111111111111111111111111111111111
responder.ephemeral.private = 2222222222222222222222222222222222222222222222222222222222222222
handshake.message1.payload.utf8 = hello
handshake.message1.ciphertext = 7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13cf78cfc936ebd851b8cad9ffa43cdace95d33248bb
handshake.message2.payload.utf8 = hello-ack
handshake.message2.ciphertext = 0faa684ed28867b97f4a6a2dee5df8ce974e76b7018e3f22a1c4cf2678570f201e9962d59102376d27a100ee30b5ecc30fadd03cbe21e3d26e
handshake.hash = 8dee97edbbb9d3828233563d5eabdda09fece7521c26cfa4b8a8844fe9baacb5
handshake.empty.message1.ciphertext = 7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13b35770df5cc53091f6c763a6cc845dc0
handshake.empty.message2.ciphertext = 0faa684ed28867b97f4a6a2dee5df8ce974e76b7018e3f22a1c4cf2678570f208f5b6645787da0523e8d34aa2ff36c4b
handshake.empty.hash = d8845252aa8d7524561873ec9d339b45b2af9d6bfb52144998e8e7a4c66d6dbd
transport.initiator_to_responder.0.frame = 00000000007b2274223a2270696e67227d
transport.initiator_to_responder.0.ciphertext = 8991c042126a331d03219a34c05345111fd4728bc029fe0a083f4c6807ff119465
transport.initiator_to_responder.1.frame = 01000000026c73202d6c610a
transport.initiator_to_responder.1.ciphertext = e9c66436322ee26742bf19bb690f0a4c013890c478e7226771609848
transport.responder_to_initiator.0.frame = 00000000007b2274223a22706f6e67227d
transport.responder_to_initiator.0.ciphertext = 71743ac15c1a08c255e6e973e6b6c3bdaaefbd4107820449ee98e61547ab07c00b
transport.initiator_to_responder.300.frame = 0100000002636f756e7465722d333030
transport.initiator_to_responder.300.ciphertext = d01d12be71c55172d88c626eec8ceef3bce397713801630420d5880d90701a73
frame.relay.session_id = 305419896
frame.relay.bytes = 12345678deadbeef
control.hello.json = {"t":"hello","relay_protocol":1,"app_version":"0.1.5","rpc_protocol":1,"capabilities":["terminal","files"]}
pairing.requester.public = b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1
pairing.acceptor.public = a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1
pairing.requester.nonce = b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2
pairing.acceptor.nonce = a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2
pairing.commitment = 59e69044db06938ded9e05949f501ae9ff1f28c0d538758a448b074363214566
pairing.sas = 318937
fixture.sha256 = 7f2b34c66fe4fe4da701db8b1f74c9965e52bcad30f1edbc7d06ead8ea97484a
```

`handshake.empty.*` is the same handshake with empty payloads in both
directions: an empty payload still carries a tag, and an implementation that
skips sealing it differs here. `transport.initiator_to_responder.300.*` is the
message at counter 300, a counter that needs more than one byte, with
pings at counters 2 to 299 in between.

Reading `transport.initiator_to_responder.0.frame`: `00` is `kind = control`,
`00000000` is `stream_id = 0`, and the rest is the UTF-8 of `{"t":"ping"}`. The
matching `ciphertext` is that frame sealed with counter 0 — seventeen bytes of
frame plus the 16-byte tag.
