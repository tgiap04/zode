---
title: Developing Zode
description: "Guide to building and developing Zode from source."
---

# Developing Zode

See the platform-specific instructions for building Zode from source:

- [macOS](./development/macos.md)
- [Linux](./development/linux.md)
- [Windows](./development/windows.md)

## Database drivers

The database column talks to each engine through a separate binary
(`zode-db-sqlite`, `zode-db-postgres`, `zode-db-mysql`, `zode-db-mongodb`),
which the app starts as a process of its own. Nothing links against them, and
they are not in `default-members`, so neither `cargo run` nor `make build`
builds them.

A checkout without them behaves like a release: the database panel offers to
download each driver the first time you connect to its engine (see below). Build
them yourself when you are working on a driver or have no route to the API:

```sh
make drivers                             # beside `make build`
make drivers PROFILE=release-fast        # beside `make build PROFILE=release-fast`
script/build-database-drivers            # beside `cargo run`
script/build-database-drivers --release  # beside `cargo run --release`
```

Opt-in, because a driver beside the executable is used before any downloaded
one. Built on every `make build`, they would make a development build skip the
download for good, and leave the remove button below with nothing to remove.
Drivers built by an earlier run stay where they are and keep winning until you
delete them, e.g. `rm target/debug/zode-db-*`.

Resolution order, when the panel needs a driver: beside the running executable
first (what the commands above build into), then the download store below, then
whatever `ZODE_DB_<ID>` names — `ZODE_DB_POSTGRES=/path/to/zode-db-postgres`, a
full path to an existing file, not a name looked up on `PATH`. A value that is
not a file is logged and ignored rather than silently skipped. A checkout that
has just run `make drivers` always gets the driver it just built, never a
downloaded one.

### Where a release build gets its drivers

A release build ships no driver binaries at all — the release bundles used to
build and include them, but they no longer do. Each driver is fetched the
first time someone connects to the engine it speaks for, from Zode's own API
rather than from a GitHub release. The app asks
`{ZODE_API_URL}/drivers/latest/zode-db-drivers-manifest.json` only which
version is current. It then fetches the signed manifest of that version from
`{ZODE_API_URL}/drivers/{version}/zode-db-manifest-signed.tar.gz`, and each
archive from `{ZODE_API_URL}/drivers/{version}/{asset}`. The signed manifest has
an archive-shaped name only because the backend serves nothing else besides the
manifest's own name; nothing reads it as an archive. Development and release
builds download the same set, whatever version the app itself reports, and the
server keeps only the set published last — there is no older one to fall back
to.

Before it runs anything the app checks, in this order (see
`crates/database/src/signature.rs` and `crates/database/src/install/`):

1. **The signature.** The signed manifest is one header line,
   `zode-driver-manifest ed25519 <base64 signature>`, then the manifest exactly
   as it was signed. The app verifies it against the Ed25519 public key built
   into it before it believes any row or any checksum, because a checksum that
   comes from the channel serving the binary proves nothing. A manifest that is
   unsigned, signed by another key, or for a different version than the one
   `latest` names is refused.
2. **That it is not a rollback.** The app refuses a set older than the newest
   one this machine has installed, which it records in
   `<data_dir>/database_drivers/.newest-installed-version`. A signature proves
   who published a set, not when, and every set ever published stays signed.
   The same version is allowed: that is how a driver you removed comes back. If
   going back is deliberate, delete that file. Where there is no such record, as on a
   fresh install, a build refuses any set older than
   `MINIMUM_DRIVER_SET_VERSION` (`0.1.5-0`, in `crates/database/src/install/store.rs`),
   which is the lowest prerelease of the first set that was ever signed.
3. **That the driver speaks our protocol.** Each row carries `protocol`, the
   database protocol version the driver speaks. A driver this build cannot talk
   to is refused before it is downloaded: a newer one says to update Zode, an
   older one says no driver has been published for this version yet. A row with
   no `protocol` skips this check, and the handshake still has the last word.
4. **The download itself.** It stops once more arrives than the manifest said,
   whatever the manifest says (never more than 512 MiB), and it gives up when a
   connection delivers nothing for 30 seconds. Partial files are removed, also
   when you walk away from a download.
5. **The checksum.** The archive's SHA-256 is compared with the signed manifest
   before anything is unpacked.

When the API has no manifest to serve, or the set it serves is not signed, the
download fails with a message that says so and points at
`script/build-database-drivers` (or `make drivers`), not only the HTTP status. A
manifest with no build of that driver for your platform names installing it by
hand (below) as the other way out.

Until a signed set has been published, a build that carries this check cannot
download drivers. Drivers that are already installed keep working, and so do
older apps, which read the unsigned copy under `latest`.

`ZODE_API_URL` defaults to `https://api.zodekit.site/api`
(`zode_account::api_url()`); point it at a backend you're running locally to
test the download path end to end, e.g.
`ZODE_API_URL=http://localhost:8000/api`. Every URL the app follows must stay on
that origin and be HTTPS; plain HTTP is accepted only for the machine's own
loopback (`localhost`, `127.0.0.0/8` and `[::1]`). A backend you run yourself
has to publish a manifest signed with the key your build trusts, or the
download is refused. This is a public, unauthenticated route — the panel must
work for someone who has never signed in.

`script/bundle-mac` is what builds, signs and packages the drivers as those
release assets, and CI runs it that way. `-n` skips both the build and the
packaging; `make bundle` passes it, because a local bundle never carries
drivers and publishing them is the release job's work.

### Publishing and signing a driver set

The release job publishes with
`script/publish-drivers-to-web [--force] <directory> <version>`, where the
directory holds the `zode-db-*.tar.gz` archives and
`zode-db-drivers-manifest.json`. The script signs first, so a missing or wrong
key costs nothing. It then uploads the archives, uploads the signed manifest,
publishes the manifest last, and finally checks that `latest` names the version
and that the backend serves the signed manifest byte for byte as it was signed.
Two copies of the manifest go out: the plain one, which is what `latest` serves
and all an older app reads, and the signed one, which carries each driver's
`protocol` and is the one the app acts on.

The key comes from the environment, and one of the two is required:

| Variable                       | Holds                                                                                                          |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| `ZODE_DRIVER_SIGNING_KEY`      | The base64 of the PKCS#8 private key file, on one line. This is the CI secret.                                 |
| `ZODE_DRIVER_SIGNING_KEY_FILE` | The path of that PKCS#8 file, for a person publishing from a machine that holds it. It wins when both are set. |

The script refuses a version older than the one that is published now, because
a publish replaces the previous set for everybody, and a machine that already
installed the newer set would refuse the older one anyway. Pass `--force` when
going back is the point.

`cargo xtask driver-manifest` is what the script calls:

- `generate-key --out <file>` writes a new Ed25519 key, refusing to overwrite a
  file, and prints the public half to embed as `DRIVER_MANIFEST_PUBLIC_KEY` in
  `crates/database/src/signature.rs`.
- `sign --manifest <file> --out <file> [--key-file <file>]` writes the signed
  manifest. It refuses a key whose public half is not the one built into your
  checkout, since the app would refuse what it signs.
- `check-key [--key-file <file>]` fails unless the signing key is present and its
  public half is the one built into your checkout. It signs nothing.
- `compare-versions <version> <other>` prints `older`, `same` or `newer`, by
  semver, so `0.1.5-rc.1` is older than `0.1.5`.

In CI the release workflow does not wait until the end to find out the key is
wrong. An early `check_driver_signing_key` job builds `xtask` and runs
`xtask driver-manifest check-key` with `ZODE_DRIVER_SIGNING_KEY`, and the job
that creates the draft release needs it, so a missing or mismatched secret stops
the run before anything is created or uploaded. The asset upload job builds
`xtask` in a step of its own that holds no secret, and hands that binary to
`script/publish-drivers-to-web` as `ZODE_XTASK_BIN`, so the signing key and the
upload token are never in the environment while the tree compiles. Release assets
are uploaded with `gh release upload --clobber`, so running the job again after a
partial failure replaces what is already there.

The app trusts only the key it was built with, so rotating the key takes an app
release. Keep the private key out of the repository and out of logs. The tooling
never prints it.

Downloaded drivers land under `<data_dir>/database_drivers/<id>/<version>/`,
where `<version>` is the one the manifest named — the published driver set's
version, not the app's. `data_dir` is:

- **macOS**: `~/Library/Application Support/Zode`
- **Linux**: `$XDG_DATA_HOME/zode` (typically `~/.local/share/zode`)
- **Windows**: `%LOCALAPPDATA%\Zode`

A downloaded driver is kept until you remove it: Zode never checks for a newer
set, and fetches the manifest only when you ask for a download. If more than
one version directory is present (an old one could not be deleted while its
driver was running), the most recently installed one wins. Whether a driver works with the app is decided when you connect, by the
protocol handshake. A driver that speaks a different protocol than this build is
refused, and the message says to remove it in the connection dialog and download
it again, or to update Zode if that does not help.

This is also the manual/offline install path: no route to `api.zodekit.site`
still works by placing the binary at
`<data_dir>/database_drivers/<id>/<version>/zode-db-<id>` (add `.exe` on
Windows). `<version>` can be any directory name of the form
`MAJOR.MINOR.PATCH[-pre]`, e.g. `0.0.0`. Zode does not run a driver from a
directory with any other name. The next download of that driver deletes every
other version directory under `<id>/`, one you placed by hand included, and
clears away leftovers of an interrupted download once they are an hour old.

A driver placed by hand is found and used like any other, and it is offered for
removal too. Every install Zode makes writes a `.zode-installed` marker beside
the executable, and only a driver with that marker is known to be one Zode
downloaded, and so one that comes back with a click. A driver with no marker may
have been placed by hand, or downloaded by an earlier Zode that wrote none; Zode
cannot tell which.

That button is a trash icon on an engine row of the database panel's connection
dialog. It appears for a driver in the download store, not for one beside the
executable, which belongs to the checkout or the bundle, nor for one named by
`ZODE_DB_<ID>`, which belongs to whoever named it. Choosing it asks first. For a
driver Zode downloaded the tooltip reads "Remove the downloaded driver" and the
question is "Remove the downloaded driver?". For one with no marker the tooltip
reads "Remove the driver", and the question names the folder it would empty,
`<data_dir>/database_drivers/<id>/`, and says the driver cannot be downloaded
again if you placed it yourself. Either way **Remove** deletes that folder, every
version it holds, and the next connection to that engine is offered the download
again. Connections that are already open keep working until they are closed.

A driver that cannot be found is never started. Connecting to it fails at once
with ``the `postgres` driver is not installed -- download it from the connection
dialog, or in a development build run `script/build-database-drivers` ``, and
the engine reads "Not Downloaded" in the dialog (or "No Driver" for an engine
Zode publishes nothing for). Zode also logs an info line,
``database driver `zode-db-postgres` is not installed``, naming the place it
looked beside the executable.

Note that saving a connection's password uses the keychain, so on a development
build see the section below.

## Keychain access

Zode stores secrets in the system keychain.

However, when running a development build of Zode on macOS (and perhaps other
platforms) trying to access the keychain results in a lot of keychain prompts
that require entering your password over and over.

On macOS this is caused by the development build not having a stable identity.
Even if you choose the "Always Allow" option, the OS will still prompt you for
your password again the next time something changes in the binary.

This quickly becomes annoying and impedes development speed.

That is why, by default, when running a development build of Zode an alternative
credential provider is used to bypass the system keychain.

> **Note:** This is **only** the case for development builds. For all non-development
> release channels the system keychain is always used.

If you need to test something out using the real system keychain in a
development build, run Zode with the following environment variable set:

```
ZED_DEVELOPMENT_USE_KEYCHAIN=1
```

## Performance Measurements

Zode includes a frame time measurement system that can be used to profile how long it takes to render each frame. This is particularly useful when comparing rendering performance between different versions or when optimizing frame rendering code.

### Using ZED_MEASUREMENTS

To enable performance measurements, set the `ZED_MEASUREMENTS` environment variable:

```sh
export ZED_MEASUREMENTS=1
```

When enabled, Zode will print frame rendering timing information to stderr, showing how long each frame takes to render.

### Performance Comparison Workflow

Here's a typical workflow for comparing frame rendering performance between different versions:

1. **Enable measurements:**

   ```sh
   export ZED_MEASUREMENTS=1
   ```

2. **Test the first version:**

   - Checkout the commit you want to measure
   - Run Zode in release mode and use it for 5-10 seconds: `cargo run --release &> version-a`

3. **Test the second version:**

   - Checkout another commit you want to compare
   - Run Zode in release mode and use it for 5-10 seconds: `cargo run --release &> version-b`

4. **Generate comparison:**

   ```sh
   script/histogram version-a version-b
   ```

The `script/histogram` tool can accept as many measurement files as you like and will generate a histogram visualization comparing the frame rendering performance data between the provided versions.

### Using `util_macros::perf`

For benchmarking unit tests, annotate them with the `#[perf]` attribute from the `util_macros` crate. Then run `cargo
perf-test -p $CRATE` to benchmark them. See the rustdoc documentation on `crates/util_macros` and `tooling/perf` for
in-depth examples and explanations.

## ETW Profiling on Windows

Zode supports performance profiling with Event Tracing for Windows (ETW) to capture detailed performance data, including CPU, GPU, memory, disk, and file I/O activity. Data is saved to an `.etl` file, which can be opened in standard profiling tools for analysis.

ETW recordings may contain personally identifiable or security-sensitive information, such as paths to files and registry keys accessed, as well as process names. Please keep this in mind when sharing traces with others.

### Recording a trace

Open the command palette and run one of the following:

- `zed: record etw trace`: records CPU, GPU, memory, and I/O activity
- `zed: record etw trace with heap tracing`: includes heap allocation data for the Zode process

Zode will prompt you to choose a save location for the `.etl` file, then request administrator permission. Once granted, recording will begin.

### Saving or canceling

While a trace is recording, open the command palette and run one of the following:

- `zed: save etw trace`: stops recording and saves the trace to disk
- `zed: cancel etw trace`: stops recording without saving

Recordings automatically save after 60 seconds if not stopped manually.

## Contributor links

- [CONTRIBUTING.md](https://github.com/zed-industries/zed/blob/main/CONTRIBUTING.md)
- [Debugging Crashes](./development/debugging-crashes.md)
- [Code of Conduct](https://zed.dev/code-of-conduct)
- [Zode Contributor License](https://zed.dev/cla)
