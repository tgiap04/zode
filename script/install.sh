#!/usr/bin/env sh
set -eu

# Downloads Zode from this repository's GitHub releases and installs it:
# Linux unpacks into ~/.local/, macOS copies the app into /Applications.
# Nothing here reads, writes or removes a Zed install.

main() {
    platform="$(uname -s)"
    arch="$(uname -m)"
    channel="${ZODE_CHANNEL:-${ZED_CHANNEL:-stable}}"
    version="${ZODE_VERSION:-${ZED_VERSION:-latest}}"
    version="${version#v}"
    bundle="${ZODE_BUNDLE_PATH:-${ZED_BUNDLE_PATH:-}}"
    # Use TMPDIR if available (for environments with non-standard temp directories)
    if [ -n "${TMPDIR:-}" ] && [ -d "${TMPDIR}" ]; then
        temp="$(mktemp -d "$TMPDIR/zode-XXXXXX")"
    else
        temp="$(mktemp -d "/tmp/zode-XXXXXX")"
    fi

    if [ "$platform" = "Darwin" ]; then
        platform="macos"
    elif [ "$platform" = "Linux" ]; then
        platform="linux"
    else
        echo "Unsupported platform $platform"
        exit 1
    fi

    case "$platform-$arch" in
        macos-arm64* | linux-arm64* | linux-armhf | linux-aarch64)
            arch="aarch64"
            ;;
        macos-x86* | linux-x86* | linux-i686*)
            arch="x86_64"
            ;;
        *)
            echo "Unsupported platform or architecture"
            exit 1
            ;;
    esac

    if command -v curl >/dev/null 2>&1; then
        curl () {
            command curl -fL "$@"
        }
    elif command -v wget >/dev/null 2>&1; then
        curl () {
            wget -O- "$@"
        }
    else
        echo "Could not find 'curl' or 'wget' in your path"
        exit 1
    fi

    "$platform" "$@"

    if [ "$(command -v zode)" = "$HOME/.local/bin/zode" ]; then
        echo "Zode has been installed. Run with 'zode'"
    else
        echo "To run Zode from your terminal, you must add ~/.local/bin to your PATH"
        echo "Run:"

        case "${SHELL:-}" in
            *zsh)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.zshrc"
                echo "   source ~/.zshrc"
                ;;
            *fish)
                echo "   fish_add_path -U $HOME/.local/bin"
                ;;
            *)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.bashrc"
                echo "   source ~/.bashrc"
                ;;
        esac

        echo "To run Zode now, '~/.local/bin/zode'"
    fi
}

download_url() {
    asset="$1"
    repo_url="https://github.com/tgiap04/zode/releases"
    case "$channel" in
        stable)
            if [ "$version" = "latest" ]; then
                echo "$repo_url/latest/download/$asset"
            else
                echo "$repo_url/download/v$version/$asset"
            fi
            ;;
        nightly)
            if [ "$version" != "latest" ]; then
                echo "A version cannot be combined with the nightly channel" >&2
                return 1
            fi
            echo "$repo_url/download/nightly/$asset"
            ;;
        *)
            echo "The $channel channel is not published; use stable or nightly" >&2
            return 1
            ;;
    esac
}

linux() {
    asset="zode-linux-$arch.tar.gz"
    if [ -n "$bundle" ]; then
        cp "$bundle" "$temp/$asset"
    else
        url="$(download_url "$asset")"
        echo "Downloading Zode from: $url"
        curl "$url" > "$temp/$asset"
    fi

    suffix=""
    if [ "$channel" != "stable" ]; then
        suffix="-$channel"
    fi

    appid=""
    case "$channel" in
      stable)
        appid="io.github.tgiap04.zode"
        ;;
      nightly)
        appid="io.github.tgiap04.zode.nightly"
        ;;
      preview)
        appid="io.github.tgiap04.zode.preview"
        ;;
      dev)
        appid="io.github.tgiap04.zode.dev"
        ;;
      *)
        echo "Unknown release channel: ${channel}. Using stable app ID."
        appid="io.github.tgiap04.zode"
        ;;
    esac

    # Unpack beside the install first, so an archive with a different layout is
    # rejected before anything under ~/.local is touched.
    mkdir -p "$temp/extract"
    tar -xzf "$temp/$asset" -C "$temp/extract"
    if [ ! -f "$temp/extract/zode$suffix.app/bin/zode" ]; then
        echo "The archive does not contain zode$suffix.app/bin/zode. It holds:"
        ls "$temp/extract"
        exit 1
    fi

    mkdir -p "$HOME/.local"
    rm -rf "$HOME/.local/zode$suffix.app"
    mv "$temp/extract/zode$suffix.app" "$HOME/.local/zode$suffix.app"

    # Setup ~/.local directories
    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"

    # Link the binary
    ln -sf "$HOME/.local/zode$suffix.app/bin/zode" "$HOME/.local/bin/zode"

    # Copy .desktop file
    desktop_file_path="$HOME/.local/share/applications/${appid}.desktop"
    cp "$HOME/.local/zode$suffix.app/share/applications/${appid}.desktop" "${desktop_file_path}"
    sed -i "s|Icon=zode|Icon=$HOME/.local/zode$suffix.app/share/icons/hicolor/512x512/apps/zode.png|g" "${desktop_file_path}"
    sed -i "s|Exec=zode|Exec=$HOME/.local/zode$suffix.app/bin/zode|g" "${desktop_file_path}"

    # Only a Zode 0.1.5 or older bundle carries this id inside a zed.app
    # directory (Zed ships dev.zed.Zed*), so Zed's own install stays unmentioned.
    if [ -f "$HOME/.local/zed$suffix.app/share/applications/${appid}.desktop" ]; then
        echo "An older Zode install remains at ~/.local/zed$suffix.app. To remove it, run:"
        echo "   rm -rf ~/.local/zed$suffix.app"
        echo "and, if ~/.local/bin/zed points into it, 'rm ~/.local/bin/zed'."
    fi
}

macos() {
    asset="Zode-$arch.dmg"
    if [ -n "$bundle" ]; then
        cp "$bundle" "$temp/$asset"
    else
        url="$(download_url "$asset")"
        echo "Downloading Zode from: $url"
        curl "$url" > "$temp/$asset"
    fi
    hdiutil attach -quiet "$temp/$asset" -mountpoint "$temp/mount"
    app="$(cd "$temp/mount/"; echo *.app)"
    case "$app" in
        Zode*.app) ;;
        *)
            echo "The disk image holds '$app', not a Zode app"
            hdiutil detach -quiet "$temp/mount"
            exit 1
            ;;
    esac
    echo "Installing $app"
    if [ -d "/Applications/$app" ]; then
        echo "Removing existing $app"
        rm -rf "/Applications/$app"
    fi
    ditto "$temp/mount/$app" "/Applications/$app"
    hdiutil detach -quiet "$temp/mount"

    mkdir -p "$HOME/.local/bin"
    # Link the binary
    ln -sf "/Applications/$app/Contents/MacOS/cli" "$HOME/.local/bin/zode"
}

main "$@"
