package com.whatabrowser.wat.webview

/**
 * Requests that are refused before they leave the phone.
 *
 * Chrome has no setting for this — blocking trackers is what people install an
 * extension for — so it is one of the places this browser can do better than the
 * thing it is copying. What it is not is uBlock Origin: there are no filter
 * rules, no cosmetic filtering and no subscriptions, just a list of hosts whose
 * only purpose is measurement, matched by suffix.
 *
 * The list is deliberately small and boring. Every entry is a domain that exists
 * to follow people between sites, and none of them serve content a page needs to
 * work — which is the line that keeps this from breaking the web. Advertising
 * hosts that also serve images or video are left alone on purpose: a blocker that
 * makes pages look broken gets switched off, and then it protects nobody.
 *
 * Matching happens on every subresource of every page, on a background thread,
 * so it is a suffix comparison against a set and nothing more.
 */
object TrackerBlocker {

    /**
     * Analytics, fingerprinting and cross-site measurement hosts.
     *
     * Suffix matched, so `foo.google-analytics.com` is covered by
     * `google-analytics.com`.
     */
    val HOSTS = setOf(
        // Analytics
        "google-analytics.com",
        "analytics.google.com",
        "googletagmanager.com",
        "stats.g.doubleclick.net",
        "matomo.cloud",
        "mixpanel.com",
        "segment.io",
        "segment.com",
        "amplitude.com",
        "heap.io",
        "hotjar.com",
        "fullstory.com",
        "mouseflow.com",
        "luckyorange.com",
        "crazyegg.com",
        "quantserve.com",
        "scorecardresearch.com",
        "chartbeat.com",
        "parsely.com",
        "newrelic.com",
        "nr-data.net",
        "bugsnag.com",
        "sentry.io",
        // Cross-site advertising measurement
        "doubleclick.net",
        "adservice.google.com",
        "adsystem.com",
        "adnxs.com",
        "rubiconproject.com",
        "pubmatic.com",
        "openx.net",
        "criteo.com",
        "criteo.net",
        "taboola.com",
        "outbrain.com",
        "sharethrough.com",
        "smartadserver.com",
        "casalemedia.com",
        "bidswitch.net",
        "adcolony.com",
        "applovin.com",
        "unityads.unity3d.com",
        // Social buttons that report every page they appear on
        "connect.facebook.net",
        "facebook.com/tr",
        "pixel.facebook.com",
        "analytics.tiktok.com",
        "ads-twitter.com",
        "analytics.twitter.com",
        "static.ads-twitter.com",
        "px.ads.linkedin.com",
        "bat.bing.com",
        "clarity.ms",
        "yandex.ru/metrika",
        "mc.yandex.ru",
        // Fingerprinting and device identification
        "fingerprintjs.com",
        "fpjs.io",
        "deviceatlas.com",
        "iovation.com",
        "adjust.com",
        "appsflyer.com",
        "branch.io",
        "kochava.com",
        "singular.net",
    )

    /**
     * Whether a request to [host] should be refused.
     *
     * Suffix matching with a dot boundary, so `evil-google-analytics.com` is not
     * mistaken for the real thing and `www.google-analytics.com` is.
     */
    fun blocks(host: String): Boolean {
        if (host.isEmpty()) return false
        val lowered = host.lowercase().trimEnd('.')
        if (lowered in HOSTS) return true
        return HOSTS.any { blocked ->
            lowered.length > blocked.length &&
                lowered.endsWith(blocked) &&
                lowered[lowered.length - blocked.length - 1] == '.'
        }
    }

    /**
     * Whether a whole URL should be refused, by its host.
     *
     * Given a URL rather than a host because that is what a WebView hands over,
     * and because the parsing wants to be in one place with tests on it. A URL
     * this cannot make sense of is allowed: failing open is right here — the cost
     * of a mistake is a broken page, not a leak, since anything actually
     * dangerous is stopped by the scheme policy in [UrlResolver] long before.
     */
    fun blocksUrl(url: String): Boolean {
        val host = UrlResolver.hostOf(url).substringBefore(':')
        if (host == url) return false
        return blocks(host)
    }
}
