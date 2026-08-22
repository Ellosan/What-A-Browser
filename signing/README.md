# Signing the release APKs

Android will not install an unsigned APK, and it will not install an update
signed with a different key than the one already there. So a release needs a key,
and it needs the *same* key every time.

There are two ways to give this repository one. The workflow prefers the first.

## The right way: repository secrets

Add these four under **Settings → Secrets and variables → Actions → Repository
secrets**:

| Secret | What goes in it |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | `base64 -w0 my-release-key.jks` |
| `ANDROID_KEYSTORE_PASSWORD` | the keystore's password |
| `ANDROID_KEY_ALIAS` | the key's alias |
| `ANDROID_KEY_PASSWORD` | the key's password |

The private key never leaves your machine or GitHub's secret store, and a
release signed with it is evidence that it came from whoever holds that key.

The release run prints which of the four it can see — names only, never values —
and refuses to publish if only some are set, since that is a typo rather than a
choice.

## The second way: a key committed here

If secrets are not available, put a keystore at `signing/release.jks` and its
password in `signing/release.properties`:

```sh
keytool -genkeypair -v \
    -keystore signing/release.jks -storetype JKS \
    -alias wat-release -keyalg RSA -keysize 4096 -validity 10950 \
    -dname "CN=What-A-Browser, O=What-A-Browser, C=GB" \
    -storepass CHOOSE_ONE -keypass CHOOSE_ONE

cat > signing/release.properties <<'EOF'
KEYSTORE_PASSWORD=CHOOSE_ONE
KEY_ALIAS=wat-release
KEY_PASSWORD=CHOOSE_ONE
EOF
```

The workflow will find them and sign every release with that key, so updates
install over each other.

**Be clear about what this is.** A key committed to a public repository is a
public key: anybody can sign an APK with it. That means it does two things and
not a third —

- it makes the APK installable, and
- it makes updates install over each other,
- but it is **not** evidence that a file came from you. Someone else can build an
  APK with this key that Android will happily install as an update over yours.

For a browser people sideload from a repository they already trust, that trade is
usually worth making rather than shipping APKs nobody can install. It stops being
worth it the moment the app is distributed anywhere its origin matters. Moving to
secrets later is one release away: the workflow prefers them the moment they
exist, and the only cost is that people have to uninstall once, because the key
changed.

## What the workflow does with neither

It publishes the per-architecture APKs unsigned, plus one debug-signed universal
APK so the release is usable, and says so in the release notes. The debug key is
made by the runner and thrown away, so those cannot be upgraded over either.
