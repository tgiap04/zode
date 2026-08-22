$ErrorActionPreference = "Stop"

if (-not $env:GITHUB_ACTIONS) {
    Write-Error "Error: This script must be run in a GitHub Actions environment"
    exit 1
}

if (-not $env:GITHUB_REF_NAME) {
    # This should be the release tag 'v0.x.x'
    Write-Error "Error: GITHUB_REF_NAME is not set"
    exit 1
}

# Mirrors script/determine-release-channel. The channel is read off the shape of the
# tag rather than crates/zed/RELEASE_CHANNEL, and the tag is not required to match the
# version in Cargo.toml -- this fork releases straight from a tag.
#
#   v0.1.0        -> stable,  not a prerelease
#   v0.1.0-beta.1 -> preview, prerelease
$version = $env:GITHUB_REF_NAME -replace '^v', ''

if ($version -eq $env:GITHUB_REF_NAME) {
    Write-Error "Error: release tag $($env:GITHUB_REF_NAME) must start with 'v'"
    exit 1
}

if ($version -like "*-*") {
    $channel = "preview"
} else {
    $channel = "stable"
}

Write-Host "Publishing version: $version on release channel $channel"
Write-Output "RELEASE_CHANNEL=$channel" >> $env:GITHUB_ENV
Write-Output "RELEASE_VERSION=$version" >> $env:GITHUB_ENV

# The bundling scripts read the FILE, not this env var -- on Windows it drives the
# installer's whole naming block (app name, display name, icon, mutex, registry key).
# Leaving it at the checked-in value is how a tagged release ends up installing
# itself as a dev build.
$channel | Set-Content -Path "crates/zed/RELEASE_CHANNEL"
