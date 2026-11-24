#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$SCCACHE_VERSION = "v0.10.0"
$SCCACHE_DIR = "./target/sccache"

function Install-Sccache {
    New-Item -ItemType Directory -Path $SCCACHE_DIR -Force | Out-Null

    $sccachePath = Join-Path $SCCACHE_DIR "sccache.exe"

    if (Test-Path $sccachePath) {
        Write-Host "sccache already cached: $(& $sccachePath --version)"
    }
    else {
        Write-Host "Installing sccache ${SCCACHE_VERSION} from GitHub releases..."

        $arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "i686" }
        $archive = "sccache-${SCCACHE_VERSION}-${arch}-pc-windows-msvc.zip"
        $basename = "sccache-${SCCACHE_VERSION}-${arch}-pc-windows-msvc"
        $url = "https://github.com/mozilla/sccache/releases/download/${SCCACHE_VERSION}/${archive}"

        $tempDir = Join-Path $env:TEMP "sccache-install"
        New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

        try {
            $archivePath = Join-Path $tempDir $archive
            Invoke-WebRequest -Uri $url -OutFile $archivePath
            Expand-Archive -Path $archivePath -DestinationPath $tempDir

            $extractedPath = Join-Path $tempDir $basename "sccache.exe"
            Move-Item -Path $extractedPath -Destination $sccachePath -Force

            Write-Host "Installed sccache: $(& $sccachePath --version)"
        }
        finally {
            Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
        }
    }

    $absolutePath = (Resolve-Path $SCCACHE_DIR).Path
    if ($env:GITHUB_PATH) {
        $absolutePath | Out-File -FilePath $env:GITHUB_PATH -Append -Encoding utf8
    }
    $env:PATH = "$absolutePath;$env:PATH"

    # Verify sccache is available in PATH - fail fast if not
    $sccacheCmd = Get-Command sccache -ErrorAction SilentlyContinue
    if (-not $sccacheCmd) {
        Write-Host "::error::sccache was installed but is not found in PATH"
        Write-Host "PATH: $env:PATH"
        Write-Host "Expected location: $absolutePath"
        if (Test-Path (Join-Path $absolutePath "sccache.exe")) {
            Write-Host "sccache.exe exists at expected location but is not in PATH"
            Write-Host "Directory contents:"
            Get-ChildItem $absolutePath | ForEach-Object { Write-Host "  $_" }
        } else {
            Write-Host "sccache.exe NOT found at expected location"
        }
        exit 1
    }
}

function Configure-Sccache {
    # The objects live in this repository's GitHub Actions cache, which is free.
    # The `expose_actions_cache` step before this one hands its URL and token to
    # `run:` steps; without them the build simply compiles without a cache.
    # There is deliberately no other backend: the R2 bucket this used before was
    # paid for by the gigabyte.
    if (-not $env:ACTIONS_RESULTS_URL -or -not $env:ACTIONS_RUNTIME_TOKEN) {
        Write-Host "The GitHub Actions cache is not reachable from this step, compiling without sccache"
        return
    }

    # Verify sccache is available before configuring
    $sccacheCmd = Get-Command sccache -ErrorAction SilentlyContinue
    if (-not $sccacheCmd) {
        Write-Host "::error::sccache not found in PATH, cannot configure RUSTC_WRAPPER"
        Write-Host "PATH: $env:PATH"
        exit 1
    }

    Write-Host "Configuring sccache with the GitHub Actions cache..."

    $baseDir = if ($env:GITHUB_WORKSPACE) { $env:GITHUB_WORKSPACE } else { (Get-Location).Path }

    # Use the absolute path to sccache binary for RUSTC_WRAPPER to avoid
    # any PATH race conditions between GITHUB_PATH and GITHUB_ENV
    $sccacheBin = (Get-Command sccache).Source

    # Set in current process
    $env:SCCACHE_GHA_ENABLED = "true"
    # A cache that cannot be reached must cost a slower build, never a red one:
    # on a server error the wrapper runs rustc itself.
    $env:SCCACHE_IGNORE_SERVER_IO_ERROR = "1"
    $env:SCCACHE_BASEDIR = $baseDir
    $env:RUSTC_WRAPPER = $sccacheBin

    # Also write to GITHUB_ENV for subsequent steps
    if ($env:GITHUB_ENV) {
        @(
            "SCCACHE_GHA_ENABLED=$($env:SCCACHE_GHA_ENABLED)"
            "SCCACHE_IGNORE_SERVER_IO_ERROR=$($env:SCCACHE_IGNORE_SERVER_IO_ERROR)"
            "SCCACHE_BASEDIR=$($env:SCCACHE_BASEDIR)"
            "RUSTC_WRAPPER=$($env:RUSTC_WRAPPER)"
        ) | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
    }

    Write-Host "✓ sccache configured with the GitHub Actions cache"
}

function Show-Config {
    Write-Host "=== sccache configuration ==="
    Write-Host "sccache version: $(sccache --version)"
    Write-Host "sccache path: $((Get-Command sccache).Source)"
    Write-Host "RUSTC_WRAPPER: $($env:RUSTC_WRAPPER ?? '<not set>')"
    Write-Host "SCCACHE_GHA_ENABLED: $($env:SCCACHE_GHA_ENABLED ?? '<not set>')"
    Write-Host "SCCACHE_BASEDIR: $($env:SCCACHE_BASEDIR ?? '<not set>')"

    Write-Host "=== sccache stats ==="
    sccache --show-stats
}

Install-Sccache
Configure-Sccache
Show-Config
