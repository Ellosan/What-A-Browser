// The handful of GM_* functions a userscript can expect from this browser.
//
// A userscript manager is an extension, and this browser has no extensions — so
// what is here is a shim, not Tampermonkey. It provides the functions that do
// not need any privilege the page does not already have:
//
//   GM_addStyle, GM_setValue, GM_getValue, GM_deleteValue, GM_listValues,
//   GM_registerMenuCommand, GM_log, GM_info
//
// Everything else is deliberately absent, and absent *visibly*: the missing ones
// throw with their own name in the message, so a script that needs one fails
// where the problem is rather than three functions later. GM_xmlhttpRequest in
// particular is not here on purpose — its whole point is issuing cross-origin
// requests that the page could not make itself, which is exactly the privilege
// this browser does not hand to page content.
//
// Values are kept in localStorage under a per-script prefix, so they are the
// page's own storage: cleared when site data is cleared, gone with a private
// window, and never shared between scripts.
(function (scriptName, scriptVersion) {
    'use strict';

    var prefix = 'wat.userscript.' + scriptName + '.';

    function store() {
        try {
            return window.localStorage;
        } catch (error) {
            // A page with storage blocked, or a sandboxed frame.
            return null;
        }
    }

    window.GM_info = {
        script: { name: scriptName, version: scriptVersion },
        scriptHandler: 'WAT',
        version: scriptVersion
    };

    window.GM_log = function () {
        try {
            console.log.apply(console, arguments);
        } catch (error) { /* a page may have replaced console */ }
    };

    window.GM_addStyle = function (css) {
        var style = document.createElement('style');
        style.textContent = String(css);
        (document.head || document.documentElement).appendChild(style);
        return style;
    };

    window.GM_setValue = function (key, value) {
        var slot = store();
        if (!slot) return;
        try {
            slot.setItem(prefix + key, JSON.stringify(value));
        } catch (error) { /* full, or refused */ }
    };

    window.GM_getValue = function (key, fallback) {
        var slot = store();
        if (!slot) return fallback;
        try {
            var raw = slot.getItem(prefix + key);
            return raw === null ? fallback : JSON.parse(raw);
        } catch (error) {
            return fallback;
        }
    };

    window.GM_deleteValue = function (key) {
        var slot = store();
        if (!slot) return;
        try {
            slot.removeItem(prefix + key);
        } catch (error) { /* refused */ }
    };

    window.GM_listValues = function () {
        var slot = store();
        var found = [];
        if (!slot) return found;
        try {
            for (var i = 0; i < slot.length; i++) {
                var key = slot.key(i);
                if (key && key.indexOf(prefix) === 0) {
                    found.push(key.slice(prefix.length));
                }
            }
        } catch (error) { /* refused */ }
        return found;
    };

    // There is no browser menu a page can add to, so this records the commands
    // and does nothing with them. Scripts call it for optional extras and carry
    // on when it is a no-op; the alternative is failing outright.
    window.GM_registerMenuCommand = function (caption, action) {
        window.__watUserscriptMenu = window.__watUserscriptMenu || [];
        window.__watUserscriptMenu.push({ caption: String(caption), action: action });
        return window.__watUserscriptMenu.length - 1;
    };

    window.GM_unregisterMenuCommand = function () {};

    ['GM_xmlhttpRequest', 'GM_download', 'GM_openInTab', 'GM_setClipboard',
     'GM_notification', 'GM_getResourceText', 'GM_getResourceURL', 'GM_cookie'
    ].forEach(function (name) {
        if (window[name]) return;
        window[name] = function () {
            throw new Error(name + ' is not available in WAT: it needs a privilege ' +
                'this browser does not give page content. See docs/WEBVIEW.md.');
        };
    });
})(__WAT_SCRIPT_NAME__, __WAT_SCRIPT_VERSION__);
