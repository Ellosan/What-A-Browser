# Release notes

What is in each release of the Android browser, in the words that go on the
GitHub release itself.

The release workflow reads this file: it takes the section whose heading matches
the version being published and puts it under "What is in it" in the notes, and
it **fails the release** if there is no section for that version. That is the
point of the file. The notes used to live inside the workflow, where nobody
edited them while changing the app, and 0.1.7 shipped describing 0.1.2.

Everything else in the published notes — how to install, whether the APKs are
signed, what was verified — is written by the run, because it describes what
that run actually did rather than what was intended.

Newest first. One `##` heading per version, and the version alone, no `v`.

## 0.2.0

**A downloads page, in the browser.** Everything downloaded, with where it came
from and how far along it is, live while it runs. Tap a finished one to open it;
hold any of them to share it, copy its address, try it again or delete it. The
system's downloads app is still one button away, but it is no longer the whole
feature.

**A userscript ships with the browser.**
[Adv-Microslop](https://greasyfork.org/en/scripts/585569-adv-microslop) by
Ellosan, MIT, with two of its replacement pairs removed and nothing else
changed. It arrives switched off and is listed in Settings → Userscripts beside
any script you install yourself, where it can be enabled, disabled or deleted.

**A script's own settings can be opened.** Userscripts register their
configuration through `GM_registerMenuCommand`, which in a browser extension puts
an entry in a toolbar this browser does not have — so until now that
configuration was code with no way to reach it. The new **Script commands** menu
item lists what the scripts on the page registered and runs the one you pick.

**Tor works.** Hiding lion was confirmed on a phone for the first time — Android
10, reaching a `.onion` address, which has no entry in the public DNS and no
route outside Tor, so a page that renders is a circuit that exists. This is the
first release that is not marked as a pre-release.

**The PC browser starts again, on Firefox.** The desktop build of WAT's own
engine is gone: it rendered its own pages beautifully and could not open the real
web, and shipping it as "the PC version" was not honest. The PC browser is now a
Firefox fork under `firefox/`, which is the only way to get Gecko — it has no
embedding API. That work is scaffolding in this release, not a browser yet.

## 0.1.7

Chrome's settings, and more privacy settings than Chrome has: tracker blocking
on by default, referrers, JavaScript, images, DNS, and clearing on exit.
Userscripts instead of extensions, installed from an address or pasted in, with
strict and tested `@match` handling.

## 0.1.6

Tor starts. The resource initializer runs in the Tor window's own process, from
a provider bound to it, after 0.1.5 asked for it the one way it refuses to be
asked. Diagnostics that can be copied out of the app when it does not.

## 0.1.5

More of the Tor failure written down where it can be read, and the first attempt
at initializing the tor resources outside the main process.

## 0.1.4

Diagnostics for the Tor window, and screenshots allowed again in it — the
`FLAG_SECURE` that blocked them was making the one error nobody could report the
one error nobody could photograph.

## 0.1.3

Tor carried in the app, so there is no second app to install. A better app icon,
drawn by the project's own rasterizer. More of Apple's Liquid Glass: the
refraction, the rim light and the tilt.

## 0.1.2

The pages behind and ahead, from holding back or forward. Two private modes,
hiding cat and hiding lion, each in its own process. A menu you can rearrange.

## 0.1.1

The everyday browser: tabs, bookmarks, history, downloads, uploads, find in
page, sharing, desktop sites, fullscreen video and settings.
