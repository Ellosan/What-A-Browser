package com.whatabrowser.wat.webview

import androidx.startup.InitializationProvider

/**
 * Runs `androidx.startup` in the Tor window's process.
 *
 * kmp-tor registers where its native libraries live from an `androidx.startup`
 * initializer, and that initializer refuses to be started any other way: asked
 * directly, it throws "cannot be initialized lazily". Eager means *discovered by
 * a provider*, and `AppInitializer` records that per process — it is a set in an
 * object that each process has its own copy of.
 *
 * Android creates a `ContentProvider` only in the process that hosts it, and the
 * one `androidx.startup` declares lives in the main process. The Tor window is a
 * process of its own, on purpose, so nothing had ever run discovery there: tor
 * started, looked for `libtor.so`, and reported it missing while the file sat in
 * the app's own library directory.
 *
 * This is that provider, declared in the manifest against `:lion`. It inherits
 * `onCreate`, which calls `AppInitializer.discoverAndInitialize()` — package
 * private, and so not callable from here, which is why this is a subclass rather
 * than a line of code. Discovery reads the metadata of the *base* provider's
 * manifest entry, which is where kmp-tor's initializer is declared, and marks it
 * initialized in this process.
 *
 * It costs the ordinary window nothing: a provider bound to `:lion` is created
 * when that process starts, and never in the main one.
 */
class TorStartupProvider : InitializationProvider()
