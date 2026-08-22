// ==UserScript==
// @name         Adv-Microslop
// @namespace    adv-microslop@ellosan
// @version      1.0.6
// @description  The customized userscript version of an extension that converts Microsoft to Microslop.
// @author       Ellosan
// @match        *://*/*
// @grant        GM_setValue
// @grant        GM_getValue
// @grant        GM_registerMenuCommand
// @grant        GM_addStyle
// @run-at       document-end
// @icon         https://addons.mozilla.org/user-media/addon_icons/3031/3031445-64.png?modified=11098af6
// @license      MIT
// @downloadURL https://update.greasyfork.org/scripts/585569/Adv-Microslop.user.js
// @updateURL https://update.greasyfork.org/scripts/585569/Adv-Microslop.meta.js
// ==/UserScript==

// Bundled with WAT, with two of the replacement pairs removed. Nothing else is
// changed: same author, same licence, same behaviour. It ships switched off and
// is listed in Settings > Userscripts like any other, where it can be enabled,
// disabled or deleted. Its own settings panel opens from the browser menu, under
// the userscript commands item.
//
// The upstream script is at
// https://greasyfork.org/en/scripts/585569-adv-microslop

(function () {
    'use strict';

    // ─── Default Replacers ──────────────────────────────────────────────
    const DEFAULT_REPLACERS = [
        { from: 'Microsoft',              to: 'Microslop',              ignorePrefix: ['@', '#'], enabled: true  },
        { from: 'Satya Nadella',           to: 'Slopya Nuttela',         ignorePrefix: ['@', '#'], enabled: true  },
        { from: 'Satya Narayana Nadella',  to: 'Slopya Narayana Nuttela',ignorePrefix: ['@', '#'], enabled: true  },
        { from: 'Copilot',                to: 'Slopilot',               ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Windows',                to: 'Bindoj',                 ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Xbox',                   to: 'GreedYbox',              ignorePrefix: ['@', '#'], enabled: false },
        { from: 'OneDrive',               to: 'CloudTumor',             ignorePrefix: ['@', '#'], enabled: false },
        { from: 'GitHub',                 to: 'ShitHub',                ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Azure',                  to: 'Assure',                 ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Generative AI',          to: 'Degenerative AI',        ignorePrefix: ['@', '#'], enabled: false },
        { from: 'GenAI',                  to: 'DegenAI',                ignorePrefix: ['@', '#'], enabled: false },
        { from: 'LinkedIn',               to: 'SloppedIn',              ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Nvidia',                 to: 'Ngreedia',               ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Ubisoft',                to: 'Ubislop',                ignorePrefix: ['@', '#'], enabled: false },
        { from: 'OpenAI',                 to: 'ClosedAI',               ignorePrefix: ['@', '#'], enabled: false },
        { from: 'BitLocker',              to: 'SlopUnlocker',           ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Sam Altman',             to: 'Scam Conman',            ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Edge',                   to: 'Edgy',                   ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Bing',                   to: 'Ling',                   ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Deepseek',               to: 'Deepshit',               ignorePrefix: ['@', '#'], enabled: false },
        { from: 'TikTok',                 to: 'Autoktol',               ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Meta',                   to: 'Toualeta',               ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Facebook',               to: 'IKnowHowYourFaceLooks',  ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Google',                 to: 'Boogle',                 ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Chrome',                 to: 'DataStealerProMax',      ignorePrefix: ['@', '#'], enabled: false },
        { from: 'Steve Jobs',             to: 'Leave Jobless',          ignorePrefix: ['@', '#'], enabled: false },
    ];

    // ─── Storage helpers ────────────────────────────────────────────────
    function loadReplacers() {
        const saved = GM_getValue('replacers', null);
        if (!saved) return JSON.parse(JSON.stringify(DEFAULT_REPLACERS));

        // Merge: keep user toggles, but ensure new defaults are picked up
        const parsed = typeof saved === 'string' ? JSON.parse(saved) : saved;
        const map = new Map(parsed.map(r => [`${r.from}→${r.to}`, r]));
        return DEFAULT_REPLACERS.map(d => {
            const key = `${d.from}→${d.to}`;
            const existing = map.get(key);
            return existing ? { ...d, enabled: existing.enabled } : { ...d };
        });
    }

    function saveReplacers(replacers) {
        GM_setValue('replacers', JSON.parse(JSON.stringify(replacers)));
    }

    // ─── Core text replacement engine ───────────────────────────────────
    function runReplacements(root, replacers) {
        const processed = new WeakSet();

        const compiled = replacers
            .filter(r => !!r.enabled)
            .map(r => {
                const escaped = r.from.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
                return {
                    match: new RegExp(`\\b${escaped}\\b`, 'gi'),
                    ignore: r.ignorePrefix && r.ignorePrefix.length
                        ? new RegExp(`^[${r.ignorePrefix.map(c => c.replace(/[\\\]-]/g, '\\$&')).join('')}]`)
                        : null,
                    to: r.to,
                };
            });

        // Quick-test regex: bail out fast for nodes that have no matches at all
        const quickTest = new RegExp(
            replacers.map(r => r.from.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'),
            'i'
        );

        const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'TEXTAREA', 'INPUT', 'NOSCRIPT', 'CODE', 'PRE']);

        function walkNode(node) {
            if (processed.has(node)) return;

            if (node.nodeType !== Node.TEXT_NODE) {
                processed.add(node);
                node.childNodes.forEach(walkNode);
                return;
            }

            if (!node.nodeValue) return;
            const parent = node.parentElement;
            if (!parent || SKIP_TAGS.has(parent.tagName) || !quickTest.test(node.nodeValue)) return;

            let text = node.nodeValue;
            let changed = false;

            for (const rule of compiled) {
                text = text.replace(rule.match, (match, offset, fullStr) => {
                    if (node.nodeValue === rule.to) return match;
                    const before = fullStr[offset - 1];
                    const after  = fullStr[offset + match.length];
                    // Skip if surrounded by dots (e.g. file extensions, domain parts)
                    if (after === '.' || (before === '.' && after === '.')) return match;
                    // Skip if preceded by an ignored prefix (@ or #)
                    if (rule.ignore && before && rule.ignore.test(before)) return match;
                    changed = true;
                    return rule.to;
                });
            }

            if (changed) {
                processed.add(node);
                node.nodeValue = text;
            }
        }

        function replaceTitle() {
            if (!document.title || !quickTest.test(document.title)) return;
            let title = document.title;
            for (const rule of compiled) {
                title = title.replace(rule.match, rule.to);
            }
            document.title = title;
        }

        // Initial pass
        walkNode(root);
        replaceTitle();

        // Watch <title> for SPA changes
        const titleEl = document.querySelector('head title');
        if (titleEl) {
            new MutationObserver(() => replaceTitle()).observe(titleEl, {
                characterData: true,
                subtree: true,
            });
        }

        // Watch body for dynamic content (batched with debounce)
        let pending = [];
        let timer = null;

        function flush() {
            [...new Set(pending)].forEach(walkNode);
            pending = [];
            timer = null;
        }

        new MutationObserver(mutations => {
            for (const m of mutations) {
                if (m.type === 'characterData') {
                    pending.push(m.target);
                } else {
                    m.addedNodes.forEach(n => pending.push(n));
                }
            }
            if (timer !== null) clearTimeout(timer);
            timer = setTimeout(flush, 10);
        }).observe(root, {
            childList: true,
            subtree: true,
            characterData: true,
        });
    }

    // ─── Settings Panel ─────────────────────────────────────────────────
    function openSettingsPanel() {
        // Prevent duplicate panels
        if (document.getElementById('adv-microslop-panel')) {
            document.getElementById('adv-microslop-panel').remove();
            return;
        }

        const replacers = loadReplacers();

        // Inject CSS
        GM_addStyle(`
            #adv-microslop-panel {
                position: fixed;
                top: 0; right: 0;
                width: 420px;
                max-width: 100vw;
                height: 100vh;
                background: #fff;
                color: #1f2937;
                z-index: 2147483647;
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
                box-shadow: -4px 0 24px rgba(0,0,0,0.18);
                display: flex;
                flex-direction: column;
                overflow: hidden;
                animation: adv-ms-slide-in 0.25s ease-out;
            }
            @keyframes adv-ms-slide-in {
                from { transform: translateX(100%); opacity: 0; }
                to   { transform: translateX(0);    opacity: 1; }
            }
            #adv-microslop-panel * {
                box-sizing: border-box;
                margin: 0;
                padding: 0;
            }
            .adv-ms-header {
                display: flex;
                align-items: center;
                gap: 10px;
                padding: 16px 20px;
                border-bottom: 1px solid #e5e7eb;
                background: #fafafa;
                flex-shrink: 0;
            }
            .adv-ms-header img {
                width: 36px;
                height: 36px;
                border-radius: 6px;
            }
            .adv-ms-header h2 {
                font-size: 20px;
                font-weight: 700;
                color: #737373;
                letter-spacing: -0.02em;
            }
            .adv-ms-close {
                margin-left: auto;
                background: none;
                border: none;
                font-size: 22px;
                cursor: pointer;
                color: #6b7280;
                padding: 4px 8px;
                border-radius: 6px;
                line-height: 1;
            }
            .adv-ms-close:hover {
                background: #f3f4f6;
                color: #111827;
            }
            .adv-ms-body {
                flex: 1;
                overflow-y: auto;
                padding: 16px 20px;
            }
            .adv-ms-label {
                font-size: 11px;
                font-weight: 600;
                text-transform: uppercase;
                letter-spacing: 0.05em;
                color: #9ca3af;
                margin-bottom: 10px;
                display: block;
            }
            .adv-ms-card {
                display: flex;
                align-items: center;
                gap: 10px;
                padding: 10px 12px;
                border: 1.5px solid #e5e7eb;
                border-radius: 10px;
                margin-bottom: 8px;
                cursor: pointer;
                transition: all 0.15s ease;
                user-select: none;
            }
            .adv-ms-card:hover {
                border-color: #d1d5db;
                background: #f9fafb;
            }
            .adv-ms-card.active {
                border-color: #6366f1;
                background: #eef2ff;
            }
            .adv-ms-card input[type="checkbox"] {
                accent-color: #6366f1;
                width: 16px;
                height: 16px;
                flex-shrink: 0;
                cursor: pointer;
                pointer-events: none;
            }
            .adv-ms-card-text {
                font-size: 13px;
                font-weight: 500;
                color: #374151;
                line-height: 1.4;
            }
            .adv-ms-card-text .adv-ms-arrow {
                color: #9ca3af;
                margin: 0 4px;
            }
            .adv-ms-card-text .adv-ms-to {
                color: #6366f1;
                font-weight: 600;
            }
            .adv-ms-footer {
                padding: 12px 20px;
                border-top: 1px solid #e5e7eb;
                background: #fafafa;
                flex-shrink: 0;
            }
            .adv-ms-footer p {
                font-size: 11px;
                color: #9ca3af;
                text-align: center;
                line-height: 1.5;
            }
        `);

        // Build panel
        const panel = document.createElement('div');
        panel.id = 'adv-microslop-panel';

        // Header
        const header = document.createElement('div');
        header.className = 'adv-ms-header';
        header.innerHTML = `
            <img src="https://addons.mozilla.org/user-media/addon_icons/3031/3031445-64.png?modified=11098af6" alt="icon">
            <h2>Adv-Microslop</h2>
        `;
        const closeBtn = document.createElement('button');
        closeBtn.className = 'adv-ms-close';
        closeBtn.textContent = '✕';
        closeBtn.onclick = () => panel.remove();
        header.appendChild(closeBtn);
        panel.appendChild(header);

        // Body
        const body = document.createElement('div');
        body.className = 'adv-ms-body';

        const label = document.createElement('span');
        label.className = 'adv-ms-label';
        label.textContent = 'Replacers';
        body.appendChild(label);

        replacers.forEach((r, idx) => {
            const card = document.createElement('div');
            card.className = 'adv-ms-card' + (r.enabled ? ' active' : '');
            const cb = document.createElement('input');
            cb.type = 'checkbox';
            cb.checked = !!r.enabled;

            const text = document.createElement('div');
            text.className = 'adv-ms-card-text';
            text.innerHTML = `${escapeHtml(r.from)} <span class="adv-ms-arrow">→</span> <span class="adv-ms-to">${escapeHtml(r.to)}</span>`;

            card.appendChild(cb);
            card.appendChild(text);

            card.addEventListener('click', () => {
                replacers[idx].enabled = !replacers[idx].enabled;
                cb.checked = replacers[idx].enabled;
                card.classList.toggle('active', replacers[idx].enabled);
                saveReplacers(replacers);
            });

            body.appendChild(card);
        });

        panel.appendChild(body);

        // Footer
        const footer = document.createElement('div');
        footer.className = 'adv-ms-footer';
        footer.innerHTML = '<p>Kindly refresh the page after you have disabled any replacer.</p>';
        panel.appendChild(footer);

        document.body.appendChild(panel);
    }

    function escapeHtml(str) {
        const div = document.createElement('div');
        div.textContent = str;
        return div.innerHTML;
    }

    // ─── Init ───────────────────────────────────────────────────────────
    const replacers = loadReplacers();
    runReplacements(document.body, replacers);

    // Register Tampermonkey menu command to open settings
    GM_registerMenuCommand('⚙️ Adv-Microslop Settings', openSettingsPanel);
})();
