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
rather than from a GitHub release. The manifest of the latest published driver
set comes from `{ZODE_API_URL}/drivers/latest/zode-db-drivers-manifest.json`,
and each archive from `{ZODE_API_URL}/drivers/{version}/{asset}`, where
`{version}` is the one that manifest names. The archive is verified against the
manifest and its SHA-256 before anything is unpacked (see
`crates/database/src/install/`). Development and release builds download the
same set, whatever version the app itself reports, and the server keeps only
the set published last — there is no older one to fall back to. When the API
has no manifest to serve, the download fails with a message that says so and
points at `script/build-database-drivers` (or `make drivers`), not only the HTTP
status. A manifest with no build of that driver for your platform names
installing it by hand (below) as the other way out.

`ZODE_API_URL` defaults to `https://api.zodekit.site/api`
(`zode_account::api_url()`); point it at a backend you're running locally to
test the download path end to end, e.g.
`ZODE_API_URL=http://localhost:8000/api`. This is a public, unauthenticated
route — the panel must work for someone who has never signed in.

`script/bundle-mac` is what builds, signs and packages the drivers as those
release assets, and CI runs it that way. `-n` skips both the build and the
packaging; `make bundle` passes it, because a local bundle never carries
drivers and publishing them is the release job's work.

Downloaded drivers land under `<data_dir>/database_drivers/<id>/<version>/`,
where `<version>` is the one the manifest named — the published driver set's
version, not the app's. `data_dir` is:

- **macOS**: `~/Library/Application Support/Zode`
- **Linux**: `$XDG_DATA_HOME/zode` (typically `~/.local/share/zode`)
- **Windows**: `%LOCALAPPDATA%\Zode`

A downloaded driver is kept until you remove it: Zode never checks for a newer
set, and fetches the manifest only when you ask for a download. If more than one version directory is present (an old one could
not be deleted while its driver was running), the most recently installed one
wins. Whether a driver works with the app is decided when you connect, by the
protocol handshake. A driver that speaks a different protocol than this build is
refused, and the message says to remove it in the connection dialog and download
it again, or to update Zode if that does not help.

This is also the manual/offline install path: no route to `api.zodekit.site`
still works by placing the binary at
`<data_dir>/database_drivers/<id>/<version>/zode-db-<id>` (add `.exe` on
Windows). `<version>` can be any directory name of the form
`MAJOR.MINOR.PATCH[-pre]`, e.g. `0.0.0`. Zode does not run a driver from a
directory with any other name, and the next download of that driver deletes
every directory under `<id>/` except the version it installs. The
executable name and directory shape are exactly what the download path itself
produces, so a driver placed there by hand is indistinguishable from one that
was downloaded, remove button included.

That button is a trash icon ("Remove the downloaded driver") on an engine row of
the database panel's connection dialog. It appears only for a driver Zode
downloaded, since a driver beside the executable belongs to the checkout or the
bundle, and one named by `ZODE_DB_<ID>` to whoever named it. It deletes
`<data_dir>/database_drivers/<id>/`, every version it holds, and the next
connection to that engine is offered the download again.

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
