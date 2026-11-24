# shellcheck shell=bash
# Whether this macOS bundle can be signed and notarized, sourced by script/bundle-mac.

macos_signing_variables=(
    MACOS_CERTIFICATE
    MACOS_CERTIFICATE_PASSWORD
    APPLE_NOTARIZATION_KEY
    APPLE_NOTARIZATION_KEY_ID
    APPLE_NOTARIZATION_ISSUER_ID
)

# Prints `sign` or `unsigned`, or explains on stderr and returns 1.
# Arguments: the release channel and the target directory (`release` or `debug`).
# Kept to what bash 3.2, the one macOS ships, understands.
macos_signing_decision() {
    local channel=$1 target_dir=$2
    local name missing="" present=0
    for name in "${macos_signing_variables[@]}"; do
        if [[ -n "${!name:-}" ]]; then
            present=$((present + 1))
        else
            missing="${missing} ${name}"
        fi
    done

    if [[ -z "${missing}" ]]; then
        if [[ -z "${MACOS_SIGNING_IDENTITY:-}" ]]; then
            echo "MACOS_SIGNING_IDENTITY must name the certificate in MACOS_CERTIFICATE to code sign." >&2
            return 1
        fi
        echo sign
        return 0
    fi

    # Half a configuration is a mistake, not a choice: say so instead of
    # quietly building something unsigned.
    if (( present > 0 )); then
        echo "Code signing is partly configured. Missing:${missing}" >&2
        return 1
    fi

    # A stable or preview release from CI goes to people's machines, where an
    # unsigned app is blocked by Gatekeeper. Refuse it rather than ship it.
    if [[ "${GITHUB_ACTIONS:-}" == "true" && "${target_dir}" == "release" ]] \
        && [[ "${channel}" == "stable" || "${channel}" == "preview" ]]; then
        echo "A ${channel} release must be signed and notarized. Missing:${missing}" >&2
        return 1
    fi

    echo unsigned
}
