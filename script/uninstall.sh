#!/usr/bin/env sh
set -eu

# Uninstalls Zode that was installed using the install.sh script.
# It never touches Zed's own paths (zed*.app, ~/.config/zed, ~/.local/share/zed,
# ~/.zed_server, Application Support/Zed).

check_remaining_installations() {
    platform="$(uname -s)"
    if [ "$platform" = "Darwin" ]; then
        # Check for any Zode variants in /Applications
        remaining=$(ls -d /Applications/Zode*.app 2>/dev/null | wc -l)
        [ "$remaining" -eq 0 ]
    else
        # Check for any Zode variants in ~/.local
        remaining=$(ls -d "$HOME/.local/zode"*.app 2>/dev/null | wc -l)
        [ "$remaining" -eq 0 ]
    fi
}

# Removes the ~/.local/bin/zode link only when it points into this Zode
# install, so a link of the same name from anything else survives.
remove_cli_link() {
    link="$HOME/.local/bin/zode"
    [ -L "$link" ] || return 0
    target="$(readlink "$link")"
    case "$target" in
        "$1"*) rm -f "$link" ;;
    esac
}

prompt_remove_preferences() {
    printf "Do you want to keep your Zode preferences? [Y/n] "
    read -r response
    case "$response" in
        [nN]|[nN][oO])
            rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/zode"
            echo "Preferences removed."
            ;;
        *)
            echo "Preferences kept."
            ;;
    esac
}

main() {
    platform="$(uname -s)"
    channel="${ZODE_CHANNEL:-${ZED_CHANNEL:-stable}}"

    if [ "$platform" = "Darwin" ]; then
        platform="macos"
    elif [ "$platform" = "Linux" ]; then
        platform="linux"
    else
        echo "Unsupported platform $platform"
        exit 1
    fi

    "$platform"

    echo "Zode has been uninstalled"
}

linux() {
    suffix=""
    if [ "$channel" != "stable" ]; then
        suffix="-$channel"
    fi

    appid=""
    db_suffix="stable"
    case "$channel" in
      stable)
        appid="io.github.tgiap04.zode"
        db_suffix="stable"
        ;;
      nightly)
        appid="io.github.tgiap04.zode.nightly"
        db_suffix="nightly"
        ;;
      preview)
        appid="io.github.tgiap04.zode.preview"
        db_suffix="preview"
        ;;
      dev)
        appid="io.github.tgiap04.zode.dev"
        db_suffix="dev"
        ;;
      *)
        echo "Unknown release channel: ${channel}. Using stable app ID."
        appid="io.github.tgiap04.zode"
        db_suffix="stable"
        ;;
    esac

    data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/zode"

    # Remove the binary symlink, if it points into this install
    remove_cli_link "$HOME/.local/zode$suffix.app/"

    # Remove the app directory
    rm -rf "$HOME/.local/zode$suffix.app"

    # Remove the .desktop file
    rm -f "$HOME/.local/share/applications/${appid}.desktop"

    # Remove the database directory for this channel
    rm -rf "$data_dir/db/0-$db_suffix"

    # Remove socket file
    rm -f "$data_dir/zed-$db_suffix.sock"

    # Remove the entire Zode directory if no installations remain
    if check_remaining_installations; then
        rm -rf "$data_dir"
        prompt_remove_preferences
    fi
}

macos() {
    # The bundle identifiers here must match the `[package.metadata.bundle-*]`
    # sections in crates/zed/Cargo.toml -- they are what macOS keys caches,
    # preferences and saved state on, so a stale one leaves all of it behind.
    app="Zode.app"
    db_suffix="stable"
    app_id="io.github.tgiap04.zode"
    case "$channel" in
      nightly)
        app="Zode Nightly.app"
        db_suffix="nightly"
        app_id="io.github.tgiap04.zode.nightly"
        ;;
      preview)
        app="Zode Preview.app"
        db_suffix="preview"
        app_id="io.github.tgiap04.zode.preview"
        ;;
      dev)
        app="Zode.app"
        db_suffix="dev"
        app_id="io.github.tgiap04.zode.dev"
        ;;
    esac

    # Remove the app bundle
    if [ -d "/Applications/$app" ]; then
        rm -rf "/Applications/$app"
    fi

    # Remove the binary symlink, if it points into this install
    remove_cli_link "/Applications/$app/"

    # Remove the database directory for this channel
    rm -rf "$HOME/Library/Application Support/Zode/db/0-$db_suffix"

    # Remove app-specific files and directories
    rm -rf "$HOME/Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments/$app_id.sfl"*
    rm -rf "$HOME/Library/Caches/$app_id"
    rm -rf "$HOME/Library/HTTPStorages/$app_id"
    rm -rf "$HOME/Library/Preferences/$app_id.plist"
    rm -rf "$HOME/Library/Saved Application State/$app_id.savedState"

    # Remove the entire Zode directory if no installations remain
    if check_remaining_installations; then
        rm -rf "$HOME/Library/Application Support/Zode"
        rm -rf "$HOME/Library/Logs/Zode"

        prompt_remove_preferences
    fi
}

main "$@"
