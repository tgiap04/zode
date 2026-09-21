# Environment sync — what can and cannot happen

Zode can store your `.env` files so another machine can fetch them. The server
that holds them is not open source. This page exists because that is a
reasonable thing to be uneasy about, and because "trust us" is not an answer.

Everything here is either checkable by you or held by a test in this
repository. Where a claim is weaker than it sounds, it says so.

## The short version

|                                                           |                                                                            |
| --------------------------------------------------------- | -------------------------------------------------------------------------- |
| Can the server read your environment files?               | No. It has no key and never receives one.                                  |
| Can it tell which projects you have?                      | No. It sees 32 random hexadecimal characters per file.                     |
| Can it tell how many variables a file holds?              | No. Every blob is padded to a 4 KiB boundary.                              |
| Can it hand you back an old file?                         | It can try. The client detects it and writes nothing.                      |
| Can it hand you someone else's file?                      | No. The ciphertext is bound to your account and to that file's identifier. |
| Can Zode recover your data if you lose your recovery key? | **No.** Nobody can. That is what the rest of this page buys.               |

## The threat model, stated plainly

**Assumed hostile:** the server, its database, its backups, its operator,
anyone who obtains any of those, and anyone on the network between you and it.

**Assumed trusted:** your machine, your operating system's keychain, and the
Zode binary you are running. If any of those is compromised, this feature does
not save you — nothing at that layer can, because that is where the file is
read in plaintext to be used.

**Out of scope:** somebody who already has your unlocked laptop. Masked values
and the optional Touch ID check make casual shoulder-surfing harder. They are
not a defence against sustained access to an unlocked machine, and they are not
sold as one.

## What leaves your machine

One request per file, containing one field:

```json
{ "blob": "<base64 of an encrypted envelope>" }
```

The envelope, its additional authenticated data, the padding and the version
counter are all specified in [Environment sync protocol](./env-sync-protocol.md),
with fixed vectors that Zode's own test suite checks this documentation
against. Change a digit on that page and the build goes red.

## Two ways to check it yourself

**1. Watch the bytes before they go.** Zode shows the exact request body before
a push — not a re-rendering of it, the same value, built once and then sent.
`crates/zode_env_sync/tests/env_pull_decision.rs` fails if those two ever
differ, and it has to be that way: the nonce is fresh per encryption, so a
panel that re-encrypted your file to show it would display something different
every time and prove nothing.

Point a proxy such as `mitmproxy` at the editor and compare what it captured
against what the panel showed. They are the same string.

**2. Read the client.** `crates/zode_env_sync/` is about two thousand lines,
and its encryption layer contains no network code at all. That layering is
deliberate: it is what lets the encryption be checked against the fixed vectors
published in [the protocol page](./env-sync-protocol.md) rather than against a
running server.

_A standalone `zode env inspect` command is not shipped. The `zode` CLI is a
small launcher that does not link the editor's libraries, and reaching the
encryption from it would mean dragging the whole UI framework into a binary
kept deliberately thin. The panel and the proxy answer the same question
today._

## What the server could still do, and what happens when it tries

An end-to-end encrypted store does not stop a hostile server from
_misbehaving_. It stops it from succeeding quietly.

| It tries                             | What happens                                                                                                                                                  |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Serve file A's blob in file B's slot | Refused. The file's identifier is inside the authenticated data, so the tag fails.                                                                            |
| Serve another account's blob         | Refused. Your user id is in there too.                                                                                                                        |
| Serve a copy from last month         | Detected. Every blob carries a counter that only goes up, and your machine remembers the highest it has applied. You are told, and **nothing is written**.    |
| Delete your files                    | Visible, and not destructive locally: files already on your machine are untouched, and Zode never deletes a local file because the server stopped listing it. |
| Refuse to serve anything             | A denial of service. Real, and the honest answer is: keep your own backups.                                                                                   |
| Change the ciphertext                | Refused. AES-256-GCM authenticates it.                                                                                                                        |

The last row of the first table is the trade: there is no recovery path,
because a recovery path is a second way in, and a second way in is a way in for
whoever holds the database.

## Where Zode is weaker than you might assume

These are stated because a security page that only lists strengths is
advertising.

**Zode's AI agents are separate programs, and they read your files directly.**
The agent panel launches Claude Code, opencode and similar as external
processes. They see your filesystem — including `.env` — the same way any
program you run does. The `private_files` setting keeps those paths out of
collaboration sharing; it does **not** sandbox an external agent, and no
setting in Zode does. If that matters to you, do not point an agent at a
checkout holding production credentials. Environment sync neither causes this
nor fixes it.

**Timing and size are not hidden completely.** Padding to 4 KiB hides how many
variables a file holds. It does not hide that you pushed something at 02:14, or
that you have eleven files rather than two.

**"Verify it is you" is not available everywhere.** macOS uses Touch ID or your
login password. Windows and Linux currently cannot be asked, and Zode says so
on the button rather than showing a dialog that checks nothing.

**Windows file permissions are weaker.** On macOS and Linux a pulled `.env` is
written `0600` — readable only by you — and so is its backup. Windows has no
equivalent; the file inherits the permissions of the folder it lands in.

**Provenance, and a weaker form of it than you may expect.** Zode's releases
carry a signed attestation you can check:

```
gh attestation verify zode.dmg --repo tgiap04/zode
```

Read the small print, because it matters. The attestation is produced by
`.github/workflows/attest_release.yml` — public, in this repository — running
**after** the release is published, over the assets it downloads. It proves
those exact bytes were seen by that public workflow, at that commit, in this
repository.

It does **not** bind the build to its output the way an attestation issued
inside the build job would. That stronger form needs the `attestations: write`
permission, and the workflow generator this repository uses cannot express it;
the file above says so at the top. Neither form proves the compiler was honest
— only bit-for-bit reproducible builds would, and Zode does not have them.

## If you lose your recovery key

The stored files are gone. Not difficult to recover — gone. There is no support
request, because there is nothing on the server's side to recover them with.

The files on your machines are untouched, because Zode never deletes a local
copy. Write the key down.
