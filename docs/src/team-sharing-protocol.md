# Team sharing — protocol

This page describes everything Zode sends when a team shares `.env` files and
database connections, in enough detail to write a compatible client without
reading Zode's source. The fixed vectors at the bottom are checked by Zode's
own test suite (`crates/zode_team_vault/tests/vectors.rs`), so a version of this
page that disagrees with the code fails the build.

The server storing team data is closed-source, and you should not have to take
anybody's word for what it can read. It stores three things and opens none of
them: each member's public sharing key, one sealed copy of the team key per
member and key version, and vault entries sealed under the team key.

## Keys

```
recovery key "ZODE-…"  ──►  DEK                         your own, 32 bytes
                              │
                              └─► seals sharing key      X25519, one per user
                                        │
              team key v1, v2, …  ◄─────┘ opened from envelopes (Noise X)
              32 bytes each                  sealed to you by an admin
                    │
                    └─► every vault entry, and the team manifest
```

There is **no password and no key derivation anywhere in this design**, for the
same reason as [environment sync](./env-sync-protocol.md): every key is 32
bytes from the operating system's random number generator.

**The sharing key** is an X25519 key pair, one per user rather than per
machine. Its private half is sealed under your recovery key (`DEK`) and stored
in your own sync store as the `sharing-key` singleton, so every machine where
you have entered your recovery key opens the same team envelopes. Its public
half is registered with the server.

**The team key** is 32 random bytes per key version. An admin makes version 1;
a rotation makes the next one. Older versions stay, sealed to the members who
remain, so entries not yet re-sealed stay readable. Team keys are held in
memory only and are never written to disk.

**Fingerprints** are the first 16 bytes of `SHA-256(public key)`: sent as 32
lowercase hex digits, shown as eight groups of four (`2397 429a 87d7 …`). The
server computes the same value; Zode recomputes it from the key and refuses an
answer whose fingerprint does not match its key.

Public keys travel as unpadded base64url (43 characters).

## The sharing key slot

`sync/sharing-key` holds the private half sealed exactly like `env-key`: the
settings envelope (`AES-256-GCM`, random 96-bit nonce generated inside the
sealing function), AAD

```
user_id ‖ 0x1F ‖ "sharing-key" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(DEK)
```

and the plaintext is the 32-byte private key. Like `env-key`, it is stamped
with the **outer** key, because it is the envelope that hands a key over.

A slot that does not open under the recovery key on this machine is never
replaced on its own. The user may replace it explicitly: a new key pair is
sealed over the slot (`If-Match` the revision read, after checking once more
that it does not open), then registered with `PUT /api/account/sharing-key`
and `If-Match` the old fingerprint. The server deletes every envelope sealed
to the old key, in every team, and admins grant the member again.

A recovery-key rotation re-seals this slot and `env-key` before anything else,
both or neither, and adopts the new recovery key only once both are under it.

## Team key envelopes

Each envelope is one one-way Noise message,
`Noise_X_25519_AESGCM_SHA256`, from the admin (initiator, static key = their
sharing key) to the member (responder, static key = their sharing key). The
payload is the 32-byte team key. The message is exactly 128 bytes:

```
offset   0   e          32   the admin's ephemeral public key
offset  32   s          48   the admin's static public key, sealed (32 + 16-byte tag)
offset  80   payload    48   the team key, sealed (32 + 16-byte tag)
```

It travels as standard base64 in the `wrappedKey` field.

**The prologue** binds the envelope to who it is for:

```
"zode-team-key-v1" ‖ 0x1F ‖ teamId ‖ 0x1F ‖ recipientUserId ‖ 0x1F ‖ keyVersion
```

`keyVersion` is the decimal version as ASCII. An envelope replayed into another
team, to another member, or as another key version produces a different
handshake hash and fails to open. Replaying an envelope into its own slot
yields the key it already holds there, which is harmless.

**Opening** an envelope, a member checks, in order:

1. the message is exactly 128 bytes;
2. the ephemeral key is not a low-order point (snow does not check its
   Diffie–Hellman results, so this is refused before any is computed);
3. the Noise message authenticates under the prologue above;
4. the sealer's static key, now known, is not a low-order point; and
5. its fingerprint equals the **pinned** fingerprint of the member the server
   says sealed it (`wrappedByUserId`), or the member's own key for envelopes it
   sealed itself. A sealer with no pin is accepted only on a machine that has
   never opened this team (no key version seen, nothing pinned): its listed
   key is then pinned as *seen*, not verified. On any later open an unpinned
   sealer is refused until the user verifies its fingerprint.

Before any of that, the server's answer must hold at most one envelope per key
version; two for one version are both refused. An envelope failing any check
is not used. Check 5 is what stops a server from sealing a key of its own
choosing to a member: it can make an envelope that authenticates, but not one
sealed by a key this machine already knows, and it cannot introduce a new
sealer to a machine that has opened the team before.

**Sealing**, an admin refuses a low-order recipient key, and seals only to a
member whose key the user verified on this machine (see [Pins](#pins)). The envelope echoes the
recipient's fingerprint (`recipientFingerprint`) so the server can refuse a wrap
aimed at a key it no longer holds for them.

## Vault entries

Each entry, and the manifest, is sealed with the settings envelope under the
team key of one version. The AAD:

| Resource                        | AAD                                                                                          |
| ------------------------------- | -------------------------------------------------------------------------------------------- |
| `teams/<team>/vault/<entry_id>` | `team_id ‖ 0x1F ‖ "team-entry" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(team key) ‖ 0x1F ‖ team_id ‖ 0x1F ‖ entry_id` |

The owner field is the team rather than a user, because many members write the
same entries. The team appears again beside the entry id so the binding does not
depend on the owner field: an entry moved into another team's slot, or another
entry's, does not open.

**Framing.** The `.env` framing, with the key version as the first four bytes
of the framed payload:

```
offset  0   seq           u64, little-endian
offset  8   payload_len   u32, little-endian   (4 + entry length)
offset 12   key_version   u32, little-endian
offset 16   entry         the shared bytes
            padding       zero bytes, to the next multiple of 4096
```

The largest entry is 262140 bytes. `seq` and the key version are inside the
ciphertext, where the server cannot change them. A reader refuses an entry
whose inner key version is not the version whose key opened it, and an entry
whose `seq` is lower than one it has already seen for that entry — a server can
hand back an old blob, but it cannot manufacture a newer one.

**The manifest** lives at the reserved entry id `00000000000000000000000000000000`
and is sealed like any entry. It names the entries; the server sees only ids:

```json
{
  "v": 1,
  "entries": [
    { "entryId": "<32 hex>", "kind": "env", "label": ".env.production", "projectHint": "acme-api" },
    { "entryId": "<32 hex>", "kind": "db_connection", "label": "staging" }
  ]
}
```

**A database connection entry** is `{ "name", "driver", "url", "password"? }`.
`password` is present only when the person sharing ticked "Include password".

## Pins

The server hands out public keys, and the one thing this design cannot stop by
cryptography alone is a server **substituting** a member's public key with one
it holds: as a recipient, so the next grant is sealed to the server; or as a
sealer, so a team key of its choosing is opened. Zode's answer is fingerprints the user compares, and it is stated here
rather than hidden. Each machine remembers, per (team, member), a fingerprint
in one of two states:

- **verified**: the user compared it out of band and pressed Trust Key (or
  Accept New Key, for a change). The only state anything is sealed to: a grant
  and a rotation both refuse a member whose key is not verified, and neither
  pins anything on its own.
- **seen**: remembered without a comparison, only by a machine opening the team
  for the first time, for the member who sealed its envelopes. It opens those
  envelopes; it is shown as not verified, and nothing is sealed to it.

A later, different fingerprint is reported as **changed**. Nothing is sealed
to it and nothing sealed by it is opened until the user accepts the change,
one member at a time, after comparing fingerprints with that person. A record
written before pins had two states reads every pin as seen.

Compare fingerprints out of band — in person, or over a channel the server does
not control. A substitution done before a machine's first open of a team is
the residual risk; any one after it is refused.

## Key versions and rotation

A machine remembers the highest key version it has opened for each team, and
refuses a server that names a lower one as current. Without that, a server
could quietly roll a team back to a key a removed member still holds, and the
next entry would be sealed under it.

Rotation, run by an admin after someone leaves:

1. make the next key and seal it to every member who has a sharing key — the
   server refuses a rotation that leaves one out — after checking that each of
   those keys is verified on this machine; one that is not stops the rotation
   before anything is sent;
2. `POST /teams/:id/key/rotate` with `fromVersion`; the server moves the
   version by compare-and-swap, so of two admins rotating at once exactly one
   wins, and the other reads the keys again and continues under the winner's.
   Either way the client then reads `rotationRecommended` from the server
   again rather than clearing it: someone may have left after the rotation
   that landed;
3. re-seal every entry still under an older version, each write conditional on
   the revision just read, with `seq` raised by one.

A removed member keeps what they already decrypted, and old keys if their
editor still has them in memory. Only rotation and re-seal stop new entries
from being readable with those keys.

## What leaves your machine

All of it only when you use team sharing — opening the team tab, pulling,
sharing, granting or rotating. Nothing at startup and nothing on a timer.

| Request | Sends |
| --- | --- |
| `GET`/`PUT /api/sync/sharing-key` | Your sharing private key, sealed under your recovery key (first use only) |
| `GET`/`PUT /api/account/sharing-key` | Your sharing **public** key |
| `GET /api/teams/:id/key` | Nothing; receives your sealed team-key envelopes |
| `GET /api/teams/:id/members/keys` | Nothing; receives members' public keys and fingerprints |
| `PUT /api/teams/:id/key/envelopes`, `POST /api/teams/:id/key/rotate` | Team keys sealed to members (admins) |
| `GET`/`PUT`/`DELETE /api/teams/:id/vault[/:entryId]` | Entries sealed under the team key |

### What the server sees

| Sees | Does not see |
| --- | --- |
| Your user id, your team id | Any team key or private key |
| Members' public keys and fingerprints | Any project name, file name or path |
| Opaque 32-character entry ids | Any variable name or value |
| Entry length, rounded up to 4 KiB | Any connection string or password |
| Which key version sealed each entry, and who wrote it when | Which entry is a `.env` and which a connection |

## Fixed vectors

All keys below are fixed bytes, for reproducibility only.

```text zode-team-vectors
wrapper.private          = 41 repeated 32 times
wrapper.public           = 7a1a4e709bf085ac494aba0469b9b1eda0ab1f78b16aabb79ffeda90623e8522
wrapper.fingerprint      = 2397429a87d7d9ba0bbcf46f399ec017
wrapper.fingerprint.shown = 2397 429a 87d7 d9ba 0bbc f46f 399e c017
recipient.private        = 42 repeated 32 times
recipient.public         = 132c442be010fbd57e72603328aa76e71fccc1503aae219327d14d9c9993f472
recipient.fingerprint    = 82dab32a9aedffb4925a762f879f6fdd
team.id                  = 68b1f0c2a4d3e5f607182900
recipient.user           = 68b1f0c2a4d3e5f60718290b
key.version              = 1
prologue                 = 7a6f64652d7465616d2d6b65792d76311f3638623166306332613464336535663630373138323930301f3638623166306332613464336535663630373138323930621f31
teamkey.bytes            = 77 repeated 32 times
teamkey.kid              = e29442e61ad354e5
ephemeral.private        = 55 repeated 32 times
wrap.len                 = 128
wrap                     = 38ab664bd86f77d7e66bdd9ae0792913a94fd8b33a1260027e4b46c1f4884c675e539169258c9e71b1cf5a7ac0c30843c2aa259bdafeec00ff12ab45026ebd19cd8801bb288ed98411b597a508d03e6095026ff0050221b41be909589c6620f3c658f444de2ae230a8e64a660fdad3a00d130db6f103e6109419a532546607a4
frame.seq                = 3
frame.key.version        = 1
frame.entry.utf8         = A=1
frame.head               = 03000000000000000700000001000000413d3100
frame.sha256             = 93e50c83513b12ecd19959d16d7abe08e2e9cdb46ee08d70e069e69ef1b89a36
max.entry.bytes          = 262140
```

Reading `frame.head`: `0300000000000000` is `seq = 3`, `07000000` is
`payload_len = 7` (four bytes of key version and three of entry),
`01000000` is key version 1, `413d31` is `A=1`, and the `00` after it is the
first byte of padding.

The public keys and fingerprints above were also checked against OpenSSL's
X25519. Two sealed blobs are committed beside the test as well —
`tests/fixtures/team-entry-v1.b64` (entry `a1a1…a1`, team key `77…`, seq 3,
version 1, `A=1`) and `tests/fixtures/sharing-key-v1.b64` (the recipient's
private key sealed under a `DEK` of `11…` for `recipient.user`). They were
produced once by this code and must go on opening.
