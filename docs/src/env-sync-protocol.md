# Environment sync — protocol

This page is the whole of it. Everything Zode sends when you sync a `.env`
file is described here, in enough detail to write a compatible client without
reading Zode's source — and the fixed vectors at the bottom are checked by
Zode's own test suite, so a version of this page that disagrees with the code
fails the build rather than misleading you.

The point of writing it down is narrow and worth stating plainly: the server
storing your environment files is closed-source, and you should not have to
take anybody's word for what it can read. It can read the length of a blob
rounded up to 4 KiB, and the time you last wrote one. That is the list.

## Keys

```
recovery key "ZODE-…"  ──►  DEK                      32 bytes, from the OS CSPRNG
                              │
                              └─► wraps envDEK       32 bytes, from the OS CSPRNG
                                        │
                                        ├─► env-manifest
                                        └─► every env entry
```

There is **no password and no key derivation anywhere in this design.** That is
deliberate, and it is the single most important thing on this page. The server
holds your ciphertext; if the key that opens it came from something a person
can remember, the server could grind it offline at its leisure. A 256-bit key
from the CSPRNG cannot be ground. The cost you pay is that the recovery key
cannot be recovered — see [Your Zode Account](./account.md).

`envDEK` is separate from `DEK` so that revoking environment access after
losing a laptop does not force every other machine to re-enter a recovery key
for settings too. You still write down exactly one recovery key, because
`envDEK` is stored **wrapped under `DEK`**, in the `env-key` slot on the
server.

`kid` — the key fingerprint stamped on every envelope — is `SHA-256(key)[..8]`.
Its only job is telling "wrong key" apart from "rotated elsewhere"; both are
otherwise an identical authentication failure, and a user reading the wrong one
of those messages does the wrong thing.

## Envelope

Identical in shape to the settings envelope. JSON, then base64, and that base64
string is the entire request body field the server stores.

```json
{
  "v": 1,
  "alg": "AES-256-GCM",
  "kid": "<base64 of 8 bytes>",
  "nonce": "<base64 of 12 bytes>",
  "ct": "<base64 of ciphertext ‖ 16-byte tag>"
}
```

The nonce is generated inside the sealing function and cannot be supplied by a
caller. AES-GCM fails catastrophically if a nonce repeats under one key, and
the surest way to prevent that is to make it impossible to ask for.

## Additional authenticated data

Fields joined by `0x1F` (ASCII unit separator). `0x1F` rather than a printable
character because no user id, resource label or identifier can contain it, so
no two distinct inputs assemble the same AAD.

| Resource | AAD |
|---|---|
| `env/<entry_id>` | `user_id ‖ 0x1F ‖ "env" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(envDEK) ‖ 0x1F ‖ entry_id` |
| `sync/env-manifest` | `user_id ‖ 0x1F ‖ "env-manifest" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(envDEK)` |
| `sync/env-key` | `user_id ‖ 0x1F ‖ "env-key" ‖ 0x1F ‖ v ‖ 0x1F ‖ **kid(DEK)** |

`kid` is the raw 8 bytes, not its base64 form. `v` is the decimal version as
ASCII, so `1` is one byte.

Two things in that table are worth dwelling on.

**The trailing `entry_id`.** Without it, a server holding many blobs for one
user could serve entry B's ciphertext from entry A's slot and the client would
decrypt it happily — the GCM tag would still verify, because the tag covers
only the ciphertext. The settings envelope has no equivalent field because
there are exactly three settings slots and the label already distinguishes
them.

**`env-key` is stamped with the OUTER key.** It is the one envelope in the
system whose identity is `DEK` rather than `envDEK`, because it is the envelope
that hands `envDEK` over. If you are writing a compatible client and this looks
like a mistake, it is not.

## Plaintext framing

Before sealing, the payload is framed and padded. The **whole** framed block —
header included — is sealed, and the whole framed block is a multiple of 4096
bytes.

```
offset  0   seq           u64, little-endian
offset  8   payload_len   u32, little-endian
offset 12   payload       payload_len bytes
            padding       zero bytes, to the next multiple of 4096
```

Maximum `payload_len` is 262144 (256 KiB).

**Padding.** A `.env` holding four variables and one holding forty differ in
stored size, and watched over a few weeks that difference is a readable account
of what you are building. Rounding to 4 KiB removes it. Padding is inside the
ciphertext, so it costs the server nothing to store and tells it nothing.

**`seq`.** A counter that only goes up, written *inside* the ciphertext. A
client records the highest `seq` it has applied for each entry and refuses
anything lower. This is what makes a replay detectable: a server can hand back
an old blob, but it cannot manufacture a newer one, because manufacturing one
means encrypting, and it has no key. For settings a rollback is an
inconvenience. For an environment file it is the restoration of a credential
you revoked.

`seq` is **not** in the AAD. Putting it there would force a client to trust the
server's claim about which version it is being given before it could decrypt
anything — and a server that lied would produce an unexplainable
authentication failure rather than a clear "this is older than what you have".

## Manifest

The `sync/env-manifest` slot holds this, framed and sealed exactly like an
entry:

```json
{
  "v": 1,
  "projects": {
    "<project_id>": {
      "name": "acme-api",
      "entries": {
        "<entry_id>": { "path": "services/api/.env.production", "seq": 7 }
      }
    }
  }
}
```

`project_id` and `entry_id` are 128 random bits, lowercase hex, 32 characters.
**Random, not derived from anything** — not from a path, not from a git remote.
Binding a checkout to a project is a manual step in Zode, so nothing ever needs
to re-derive one, and an identifier with no derivation input is one the server
cannot reason backwards from.

`path` is relative to the bound worktree root and never absolute. An absolute
path is the one field that would describe how your disk is laid out, and it
would be wrong on the next machine anyway.

## What the server sees

| Sees | Does not see |
|---|---|
| Your user id | Any project name |
| Opaque 32-character hex identifiers | Any file name or path |
| Blob length, rounded up to 4 KiB | Any variable name |
| When a blob was written | Any value |
| A random per-write revision string | How many variables a file holds |

## Verifying this yourself

You do not have to trust this page.

- **Before each push**, Zode shows you the exact bytes it is about to send —
  the same value the request carries, built once and then sent, not a second
  rendering that ought to agree. It could not be a second rendering: the nonce
  is fresh per encryption, so re-encrypting to display would produce something
  different every time.
- Point a proxy such as `mitmproxy` at the editor and compare. See
  [Environment sync security](./env-sync-security.md) for what that does and
  does not establish.
- The vectors below are asserted against this file by
  `crates/zode_env_sync/tests/spec_vectors_match_docs.rs`. Edit a digit here
  and Zode's test suite goes red.

## Fixed vectors

```text zode-env-vectors
envdek.bytes        = 33 repeated 32 times
envdek.kid          = deb0e38ced1e41de
dek.bytes           = 11 repeated 32 times
dek.kid             = 02d449a31fbb267c
pack.seq            = 3
pack.payload.utf8   = A=1
pack.block.len      = 4096
pack.block.head     = 030000000000000003000000413d3100
pack.block.sha256   = 798c5f228598020d9aa13385dca724fd7c3d86dda6b57df0726b7a8a51bc2f16
max.payload.bytes   = 262144
pad.block.bytes     = 4096
```

Reading `pack.block.head`: `0300000000000000` is `seq = 3` as a little-endian
u64, `03000000` is `payload_len = 3`, `413d31` is `A=1`, and the `00` after it
is the first byte of 4081 bytes of padding.

Two sealed blobs are committed beside the test as well —
`tests/fixtures/env-entry-v1.b64` and `tests/fixtures/env-key-v1.b64`. They
were produced once by this code and must go on opening. A round-trip test
passes even when both sides of the format move together; these do not.
