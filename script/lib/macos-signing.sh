# shellcheck shell=bash
# Whether this macOS bundle can be signed and notarized, and the keychain that
# signs it. Sourced by script/bundle-mac and script/check-macos-signing.

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

# Apple's Developer ID G2 intermediate, which every Developer ID Application
# certificate chains to. Imported next to the certificate so the identity is
# trusted whether or not the machine already has it.
macos_signing_intermediate_url="https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer"
macos_signing_intermediate_sha256="f16cd3c54c7f83cea4bf1a3e6a0819c8aaa8e4a1528fd144715f350643d2df3a"

# The user's keychain search list before macos_signing_setup_keychain changed it.
macos_signing_previous_keychains=()

# Creates <keychain>, imports the PKCS#12 file <p12> into it with <password>,
# puts it on codesign's search list, and checks that it holds a code signing
# identity named <identity>. On failure, says what the PKCS#12 file held and
# returns 1; the caller still runs macos_signing_cleanup_keychain.
macos_signing_setup_keychain() {
    local keychain=$1 p12=$2 password=$3 identity=$4
    local keychain_entry intermediate identities

    security create-keychain -p "$password" "$keychain" || echo ""
    security unlock-keychain -p "$password" "$keychain"
    # Without `-t` the keychain never locks itself again during the build.
    security set-keychain-settings "$keychain"
    # `security` guesses the format from the file name's extension, and the
    # release decodes the certificate into a temporary file without one.
    security import "$p12" -k "$keychain" -f pkcs12 -P "$password" -T /usr/bin/codesign

    intermediate=$(mktemp)
    if curl -fsSL --retry 3 -o "$intermediate" "$macos_signing_intermediate_url" \
        && [[ "$(shasum -a 256 "$intermediate" | cut -d ' ' -f 1)" == "$macos_signing_intermediate_sha256" ]]; then
        # Runners usually have it already, which `security` reports as an error.
        security import "$intermediate" -k "$keychain" > /dev/null 2>&1 || true
    else
        echo "warning: could not fetch Apple's Developer ID G2 intermediate; relying on the system's copy"
    fi
    rm -f "$intermediate"

    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" > /dev/null

    # codesign finds identities through the user's keychain search list, not
    # the default keychain, and a new keychain is not on that list.
    macos_signing_previous_keychains=()
    while IFS= read -r keychain_entry; do
        keychain_entry="${keychain_entry#"${keychain_entry%%[![:space:]]*}"}"
        keychain_entry="${keychain_entry%\"}"
        keychain_entry="${keychain_entry#\"}"
        [[ -n "$keychain_entry" ]] && macos_signing_previous_keychains+=("$keychain_entry")
    done < <(security list-keychains -d user)
    security list-keychains -d user -s "$keychain" ${macos_signing_previous_keychains[@]+"${macos_signing_previous_keychains[@]}"}

    # Every identity, trusted or not, so a missing chain shows up as such
    # rather than as a missing identity.
    identities="$(security find-identity -p codesigning "$keychain")"
    echo "$identities"
    if grep -qF "\"$identity\"" <<< "$identities"; then
        return 0
    fi

    local private_keys
    private_keys=$(security dump-keychain "$keychain" | grep -c "^class: 0x00000010" || true)
    echo "error: no code signing identity named \"$identity\" in the certificate file."
    echo "The keychain holds ${private_keys} private key(s) and these certificates (Apple's intermediate included):"
    security find-certificate -a -p "$keychain" \
        | openssl crl2pkcs7 -nocrl -certfile /dev/stdin 2>/dev/null \
        | openssl pkcs7 -print_certs -noout 2>/dev/null \
        | grep "subject=" | sed 's/^/  /'
    echo "A .p12 exported before the Developer ID .cer was installed holds only the private key."
    echo "Install the .cer into the keychain that holds the key, export the certificate from"
    echo "Keychain Access > My Certificates (it carries the key with it), then check it with"
    echo "script/check-macos-signing before updating MACOS_CERTIFICATE."
    return 1
}

# Restores the keychain search list and deletes <keychain>.
macos_signing_cleanup_keychain() {
    local keychain=$1
    # Only once setup has read the list: an empty `-s` would clear it.
    if (( ${#macos_signing_previous_keychains[@]} > 0 )); then
        security list-keychains -d user -s "${macos_signing_previous_keychains[@]}"
    fi
    security delete-keychain "$keychain" 2>/dev/null || true
}

# Puts <file>... into one zip at <zip> for notarization. `ditto -c -k`
# archives a single source, so the files are gathered into one folder first;
# notarytool wants the macOS metadata that ditto keeps and `zip` drops.
macos_signing_zip_for_notarization() {
    local zip=$1
    shift
    local staging
    staging="$(mktemp -d)/notarize"
    mkdir -p "$staging"
    cp -p "$@" "$staging/"
    /usr/bin/ditto -c -k --keepParent "$staging" "$zip"
    rm -rf "$(dirname "$staging")"
}

# Submits <file> (a .zip, .dmg or .pkg) to Apple's notary service, waits for
# the verdict, and returns 1 unless it is Accepted; a rejection prints Apple's
# log, which names every binary and the reason. `submit --wait` alone exits 0
# for an Invalid submission too.
macos_signing_notarize() {
    local file=$1 key_file output status submission_id result=0
    local notarytool
    notarytool="$(xcode-select -p)/usr/bin/notarytool"
    key_file=$(mktemp)
    printf '%s\n' "$APPLE_NOTARIZATION_KEY" > "$key_file"
    # The .p8 key as text; a base64 copy of it is accepted too. Anything else
    # reaches notarytool as "invalidAsn1", which names neither the secret nor
    # the cause.
    if ! grep -q "BEGIN PRIVATE KEY" "$key_file"; then
        if printf '%s' "$APPLE_NOTARIZATION_KEY" | base64 --decode 2>/dev/null | grep -q "BEGIN PRIVATE KEY"; then
            printf '%s' "$APPLE_NOTARIZATION_KEY" | base64 --decode > "$key_file"
        else
            rm -f "$key_file"
            echo "error: APPLE_NOTARIZATION_KEY is not the text of an App Store Connect API key (.p8)."
            echo "Set it from the file itself: gh secret set APPLE_NOTARIZATION_KEY < AuthKey_<KEY_ID>.p8"
            return 1
        fi
    fi
    local credentials=(--key "$key_file" --key-id "$APPLE_NOTARIZATION_KEY_ID" --issuer "$APPLE_NOTARIZATION_ISSUER_ID")

    output=$("$notarytool" submit --wait --output-format json "${credentials[@]}" "$file") || result=$?
    echo "$output"
    status=$(grep -o '"status" *: *"[^"]*"' <<< "$output" | head -n 1 | sed -E 's/.*"([^"]*)"$/\1/' || true)
    submission_id=$(grep -o '"id" *: *"[^"]*"' <<< "$output" | head -n 1 | sed -E 's/.*"([^"]*)"$/\1/' || true)

    if [[ $result -ne 0 || "$status" != "Accepted" ]]; then
        echo "error: notarization of $(basename "$file") ended as '${status:-unknown}' (notarytool exit ${result})."
        if [[ -n "$submission_id" ]]; then
            "$notarytool" log "$submission_id" "${credentials[@]}" || true
        fi
        rm -f "$key_file"
        return 1
    fi
    rm -f "$key_file"
    echo "Notarized $(basename "$file")"
}
