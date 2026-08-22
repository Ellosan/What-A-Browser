#!/usr/bin/env bash
#
# Makes the release signing key and prints the four values to paste into
# GitHub's secrets.
#
#     ./signing/make-key.sh
#
# Run this on your own machine, not on a runner and not in a container you are
# going to throw away. The whole point of the key is that only you have it, and
# a key that has passed through somewhere else is a key that has.
#
# Needs `keytool`, which comes with any JDK — Android Studio ships one at
# Android Studio > Settings > Build Tools > Gradle > Gradle JDK, and on a Mac
# `/usr/libexec/java_home` will find it. Nothing else.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"

# Outside the repository by default, and that is not fussiness. `.gitignore`
# un-ignores `signing/release.jks` on purpose, because that path is the *other*
# route — a key committed here deliberately, and therefore public. A key meant
# for GitHub's secrets that lands on that path is one `git add .` away from
# being a key anybody can sign with.
keystore="${1:-$HOME/wat-release.jks}"
alias_name="wat-release"

if ! command -v keytool > /dev/null; then
    echo "keytool is not on the PATH. It comes with any JDK; install one, or on" >&2
    echo "a Mac try: export PATH=\"\$(/usr/libexec/java_home)/bin:\$PATH\"" >&2
    exit 1
fi

case "$(cd "$(dirname "$keystore")" 2> /dev/null && pwd)/" in
    "$repo"/*)
        echo "warning: $keystore is inside the repository." >&2
        echo "  If you commit it, the key is public and signs nothing but" >&2
        echo "  installability — see the second section of signing/README.md." >&2
        echo "  For the secrets route, keep it somewhere else." >&2
        echo >&2
        ;;
esac

# Refusing rather than overwriting. A keystore replaced by accident is every
# future release unable to install over the ones already on people's phones,
# and there is no undoing it.
if [ -e "$keystore" ]; then
    echo "$keystore already exists. Move it aside first if you really mean to" >&2
    echo "make a new key — the old one is the only thing that can sign updates" >&2
    echo "over the releases already published with it." >&2
    echo >&2
    echo "Its secrets, if this script made it, are in:" >&2
    echo "  $keystore.secrets" >&2
    echo >&2
    echo "If they are gone and no release has been signed with this key yet," >&2
    echo "deleting it and running this again costs nothing. If one has, it" >&2
    echo "costs every reader an uninstall, so be sure before you do." >&2
    exit 1
fi

# Generated rather than chosen. This password is never typed by a person: it is
# pasted into GitHub once and read out of the keystore file after that, so
# there is no reason for it to be memorable and every reason for it not to be.
#
# Read a fixed block and filter it, rather than filtering /dev/urandom and
# stopping at 32 characters. `tr < /dev/urandom | head -c 32` is the idiom
# everyone writes and it exits 141 under `pipefail`: head closes the pipe, tr
# takes the SIGPIPE, and the script dies before it ever reaches keytool.
password=""
while [ "${#password}" -lt 32 ]; do
    block="$(head -c 512 /dev/urandom | LC_ALL=C tr -dc 'A-Za-z0-9')"
    password="$password$block"
done
password="${password:0:32}"

# PKCS12 rather than JKS: JKS is Sun's own format, keytool warns about it on
# every use, and apksigner reads both. In PKCS12 the store and the key share
# one password, which is why the two secrets below hold the same value.
#
# 30 years, because a signing certificate that expires is an app that can no
# longer be updated, and there is no way to renew one after the fact.
keytool -genkeypair \
    -keystore "$keystore" -storetype PKCS12 \
    -alias "$alias_name" -keyalg RSA -keysize 4096 -validity 10950 \
    -dname "CN=What-A-Browser, O=Ellosan, C=GB" \
    -storepass "$password" -keypass "$password"

# One line, because one line is what a person can select and copy in a single
# gesture. Not because the runner needs it: `base64 -d` skips newlines quite
# happily, and a wrapped value decodes to the same bytes — that was checked.
# The failure this avoids is a half-selected paste, which decodes to a
# truncated file and fails much later, in the signer.
if base64 --help 2>&1 | grep -q -- "-w"; then
    encoded="$(base64 -w0 "$keystore")"
else
    # BSD base64, on macOS, wraps nothing by default.
    encoded="$(base64 < "$keystore" | tr -d '\n')"
fi

# On disk before anything is printed to the screen.
#
# A terminal is not storage. It scrolls, it gets cleared, and the password is
# the one value here that exists nowhere else — the alias is fixed, the base64
# can be regenerated from the keystore, but a lost password makes the keystore
# itself useless, and the refusal above then stands between you and a new one.
# So the four values land in a file first, and the screen is a convenience.
umask 077
printf '%s' "$encoded" > "$keystore.base64"
cat > "$keystore.secrets" <<EOF
# The four repository secrets for $keystore, as of $(date -u '+%Y-%m-%d').
# Settings > Secrets and variables > Actions > New repository secret.
#
# Keep this with the keystore, or delete it once the secrets are in GitHub —
# but not before, and never both this and the keystore's only copy in one
# place you might lose at once.

ANDROID_KEY_ALIAS=$alias_name
ANDROID_KEYSTORE_PASSWORD=$password
ANDROID_KEY_PASSWORD=$password

# ANDROID_KEYSTORE_BASE64 is the whole of $keystore.base64, on one line.
EOF
chmod 600 "$keystore.secrets" "$keystore.base64" "$keystore"

echo
echo "=============================================================="
echo "Four secrets, at Settings > Secrets and variables > Actions >"
echo "New repository secret. Names exactly as written."
echo "=============================================================="
echo
echo "ANDROID_KEY_ALIAS"
echo "$alias_name"
echo
echo "ANDROID_KEYSTORE_PASSWORD"
echo "$password"
echo
echo "ANDROID_KEY_PASSWORD"
echo "$password"
echo
echo "ANDROID_KEYSTORE_BASE64"
echo "  the whole of $keystore.base64 — one line, select all of it"
echo
echo "=============================================================="
echo
echo "All four are also written to"
echo "  $keystore.secrets"
echo "so clearing this terminal costs you nothing. Read them back with:"
echo "  cat '$keystore.secrets'"
echo
echo "Then keep $keystore somewhere you will still have it in five years,"
echo "and keep the password with it. Losing either means no future release can"
echo "install over this one: Android will refuse an update signed with a"
echo "different key, and every reader has to uninstall and lose their data."
echo
echo "None of these three files belongs in the repository, and by default none"
echo "of them is in it."
