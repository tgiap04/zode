# Team sharing — what can and cannot happen

A team on Zode's Team plan can share `.env` files and database connections. The
server that stores them is not open source. This page says what it can see,
what it cannot, and where the design is weaker than it sounds. The wire format
is in [Team sharing protocol](./team-sharing-protocol.md), with fixed vectors
that Zode's own test suite checks.

## The short version

|                                                              |                                                                                                   |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------- |
| Can the server read a shared `.env` or connection?           | No. Entries are sealed on a member's machine under a team key the server never receives.          |
| Can it tell which entry is a `.env` and which a connection?  | No. It sees opaque 32-character ids, lengths rounded up to 4 KiB, and who wrote each one when.    |
| Can it read a shared database password?                      | No. And a password is shared only when the person sharing ticks "Include password".               |
| Can it slip a key of its own into the team?                  | Not once this machine has opened the team. See [Fingerprints](#fingerprints).                     |
| Does removing someone stop them reading new entries?         | Their access ends at once; only a key rotation stops a copy of the old key from opening new data. |
| Can Zode recover a team's data if every member loses access? | **No.** Nobody can.                                                                               |

## What the server sees

| Sees                                                      | Does not see                                    |
| --------------------------------------------------------- | ----------------------------------------------- |
| Your user id and your team id                             | Any team key, or anyone's sharing private key   |
| Each member's sharing **public** key and its fingerprint  | Any project name, file name or path             |
| One sealed copy of the team key per member and version    | Any variable name or value                      |
| Opaque entry ids, entry lengths rounded up to 4 KiB       | Any connection string or password               |
| Which key version sealed each entry, who wrote it, when   | Which entry is a `.env` and which a connection  |

Your sharing private key is stored on the server too, at `sync/sharing-key`,
sealed under your recovery key exactly like your environment key. The server
holds the ciphertext and not the recovery key, so it cannot open it.

Nothing is sent at startup and nothing on a timer. Every request comes from
something you did: opening the Team tab of the environment vault, pressing
reload on the database panel's Team group, opening a team connection there,
opening the members window, pulling, sharing, granting, deleting or rotating. The full list of requests is
in the protocol page.

## Fingerprints

The one thing cryptography alone cannot stop here is a server **substituting**
a key: handing an admin a public key it holds instead of a member's, so the
next grant is sealed to the server; or sealing a team key of its own choosing
to you, in the name of a member it listed. Zode answers that with
fingerprints you compare yourself, and says plainly where that starts:

- The members window shows every member's key fingerprint as eight groups of
  four hex digits.
- A key is **verified** on this machine only after someone here pressed
  **Trust Key…**, which shows the fingerprint again and asks you to compare it
  with that person first — in person, or over a channel Zode's server does not
  carry. Every other key is shown as **not verified on this machine**.
- Nothing is sealed to a key that is not verified. An admin cannot grant a
  member the team key, and a rotation does not go ahead, until each recipient's
  key is verified here. Neither ever verifies a key on its own.
- A team key you receive is opened only if the member who sealed it has a key
  this machine already knows. A member it has never seen — a new admin, or a
  "member" the server invented — is refused, and the Team tab says who sealed
  the key and asks you to compare their fingerprint in the members window.
  Trusting it there opens the key.
- A key that differs from the one known earlier is shown as a warning, with
  both fingerprints. Nothing is sealed to it and nothing sealed by it is
  opened until you press **Accept New Key…**, one member at a time. A new
  machine or a reinstall changes a key legitimately; a substitution changes it
  too, which is why the comparison is yours to make.
- The server keeps one sealed copy of the team key per member and version. If
  it hands you two for the same version, neither is used.

**The first time is the residual risk.** A machine opening a team for the
very first time knows no one yet, so it takes the key of whoever sealed your
team key from the server's member listing, and remembers it as *seen*. A seen
key opens envelopes; it is still shown as **not verified**, and nothing is
sealed to it until you trust it. A substitution made before that first look
is not caught by the machine; one made after it is. Compare the sealer's
fingerprint in the members window after your first open to close that gap.

**Trust is per machine.** What you verified on your laptop is not known to your
desktop: on a second machine every member starts as not verified again, and
you compare fingerprints again before granting from it.

## Your own sharing key

Your sharing key is sealed under your recovery key and follows it to every
machine where you enter it. If the stored sharing key does not open with the
recovery key on this machine — it was sealed under a recovery key you no
longer have — the Team tab says so. First try entering the recovery key you
use elsewhere. If none opens it, **Replace My Sharing Key…** makes a new one.
That cannot be undone: every team key sealed to the old one is deleted, in
every team you are in, and an admin of each team must compare your new
fingerprint and grant you again.

## Removing a member, and rotating

Removing someone deletes their sealed copies of the team key on the server and
cuts their access to the vault at once. What it cannot do:

- take back what they already pulled — files on their disk, passwords in their
  keychain;
- take back the old team key if their editor still holds it in memory.

That is what rotation is for. After someone leaves, the server flags the team
and the members window highlights **Rotate Team Key…** for admins. Rotating
makes a new key version, seals it to every remaining member who has a sharing
key, and re-seals every entry under it. Each of those keys must be verified
on this machine: one that is not stops the rotation before anything is sent,
so a rotation is never the moment trust begins. A copy of the old key then
opens nothing written afterwards. If a rotation is interrupted, **Finish
Re-sealing** completes it without making another version. The flag is the
server's: if someone leaves while another admin's rotation lands, it stays
raised and the window asks you to rotate again.

Change the secrets themselves — database passwords, API keys — if someone who
left should no longer have them. No sharing scheme can unshare a value a person
has already seen.

## A lapsed plan

When the team's plan ends, the vault turns read-only: members can still pull
what was shared, and nothing new can be shared until the plan is active again.

## Where this is weaker than you might assume

**Every member who holds the team key can read every entry.** There are no
per-entry permissions inside a team. Share a production `.env` with a team only
if everyone in it should have it.

**Admins are trusted with the team key.** An admin can grant it to anyone in the
team after checking their fingerprint. The server enforces who is an admin; a
compromised admin account is a compromised team.

**Metadata is visible.** The server knows who is in a team, how many entries it
holds, roughly how large each is, and who wrote what when.

**Pulled files are ordinary files.** A pulled `.env` is written `0600` on macOS
and Linux, with a backup of what it replaced kept outside the project. Anything
that can read your files can read it, as with environment sync — see
[Environment sync security](./env-sync-security.md).

**Whoever shares a file names where it lands.** The path a pull writes to is
part of the shared entry. A `.env` or `.env.*` file is written straight away
(with a diff first if one is already there). Any other path — an editor
setting, a task file, anything inside `.git` — could change what your tools
run, so Zode asks before writing it, even when the file is new.
