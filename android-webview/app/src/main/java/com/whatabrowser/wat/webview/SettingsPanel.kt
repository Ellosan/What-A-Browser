package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.AlertDialog
import android.app.Dialog
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.TextView
import android.widget.Toast

/**
 * Settings, in the sections Chrome uses, because that is what people know where
 * to look in.
 *
 * The privacy section is the point of the screen. Three of the switches in it are
 * things Chrome has no setting for at all — blocking trackers, refusing every
 * cookie, and throwing everything away on exit — and the notes at the bottom say
 * what this browser never does, which is the part a settings screen usually
 * leaves you to guess at.
 *
 * What is deliberately absent is as considered as what is here: nothing on this
 * screen can turn off certificate checking, allow cleartext, let a page reach
 * `file://`, or accept third-party cookies. See [Settings] for why.
 */
object SettingsPanel {

    fun show(
        activity: Activity,
        settings: Settings,
        store: BrowserStore,
        onChanged: () -> Unit,
    ): Dialog {
        val palette = GlassSurface.palette(activity)
        val pad = GlassSurface.dp(activity, 18f).toInt()

        val dialog = Dialog(activity)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)

        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad / 2)
        }
        root.addView(heading(activity, palette, activity.getString(R.string.settings), 18f))

        // --- general --------------------------------------------------------
        root.addView(section(activity, palette, R.string.section_general))

        val engineRow = row(activity, palette, activity.getString(R.string.search_engine), settings.engine.name)
        val homeRow = row(activity, palette, activity.getString(R.string.home_page), settings.homePage)
        val themeRow = row(activity, palette, activity.getString(R.string.theme), themeName(activity, settings.theme))

        engineRow.setOnClickListener {
            chooseEngine(activity, settings) {
                value(engineRow).text = settings.engine.name
                value(homeRow).text = settings.homePage
                onChanged()
            }
        }
        homeRow.setOnClickListener {
            chooseHomePage(activity, settings) {
                value(homeRow).text = settings.homePage
                onChanged()
            }
        }
        themeRow.setOnClickListener {
            chooseTheme(activity, settings) {
                value(themeRow).text = themeName(activity, settings.theme)
                onChanged()
                // The theme is chosen when a window is built, so this one has to
                // start again to show it.
                activity.recreate()
            }
        }
        root.addView(engineRow)
        root.addView(homeRow)
        root.addView(themeRow)
        root.addView(
            toggle(activity, palette, activity.getString(R.string.desktop_by_default), settings.desktopSite) {
                settings.desktopSite = it
            },
        )

        // --- privacy and security -------------------------------------------
        root.addView(section(activity, palette, R.string.section_privacy))
        root.addView(
            toggle(activity, palette, activity.getString(R.string.block_trackers), settings.blockTrackers) {
                settings.blockTrackers = it
                onChanged()
            },
        )
        root.addView(note(activity, palette, activity.getString(R.string.block_trackers_detail, TrackerBlocker.HOSTS.size)))
        root.addView(
            toggle(activity, palette, activity.getString(R.string.safe_browsing), settings.safeBrowsing) {
                settings.safeBrowsing = it
                onChanged()
            },
        )
        root.addView(note(activity, palette, activity.getString(R.string.safe_browsing_detail)))
        root.addView(
            toggle(activity, palette, activity.getString(R.string.block_all_cookies), settings.blockAllCookies) {
                settings.blockAllCookies = it
                onChanged()
            },
        )
        root.addView(note(activity, palette, activity.getString(R.string.block_all_cookies_detail)))
        root.addView(
            toggle(activity, palette, activity.getString(R.string.clear_on_exit), settings.clearOnExit) {
                settings.clearOnExit = it
            },
        )
        root.addView(
            toggle(activity, palette, activity.getString(R.string.keep_history), settings.keepHistory) {
                settings.keepHistory = it
                if (!it) store.clearHistory()
            },
        )
        root.addView(
            toggle(activity, palette, activity.getString(R.string.suggest_from_history), settings.suggestFromHistory) {
                settings.suggestFromHistory = it
                onChanged()
            },
        )
        root.addView(
            toggle(activity, palette, activity.getString(R.string.javascript), settings.javaScript) {
                settings.javaScript = it
                onChanged()
            },
        )
        root.addView(
            toggle(activity, palette, activity.getString(R.string.images), settings.images) {
                settings.images = it
                onChanged()
            },
        )

        val clearRow = row(
            activity,
            palette,
            activity.getString(R.string.clear_data),
            activity.getString(R.string.clear_data_detail),
        )
        clearRow.setOnClickListener {
            confirmClear(activity, store) {
                dialog.dismiss()
                onChanged()
            }
        }
        root.addView(clearRow)

        // --- userscripts ----------------------------------------------------
        root.addView(section(activity, palette, R.string.section_userscripts))
        root.addView(note(activity, palette, activity.getString(R.string.userscripts_detail)))
        root.addView(
            toggle(activity, palette, activity.getString(R.string.userscripts_enabled), settings.userScripts) {
                settings.userScripts = it
                onChanged()
            },
        )
        val scriptsRow = row(
            activity,
            palette,
            activity.getString(R.string.manage_userscripts),
            activity.getString(R.string.installed_count, UserScriptStore(activity).all().size),
        )
        scriptsRow.setOnClickListener {
            UserScriptPanel.show(activity, settings) {
                value(scriptsRow).text =
                    activity.getString(R.string.installed_count, UserScriptStore(activity).all().size)
                onChanged()
            }.show()
        }
        root.addView(scriptsRow)

        // --- accessibility ---------------------------------------------------
        root.addView(section(activity, palette, R.string.section_accessibility))
        root.addView(textSize(activity, palette, settings, onChanged))
        root.addView(
            toggle(activity, palette, activity.getString(R.string.force_zoom), settings.forceZoom) {
                settings.forceZoom = it
                onChanged()
            },
        )
        root.addView(
            toggle(activity, palette, activity.getString(R.string.darken_sites), settings.darkenSites) {
                settings.darkenSites = it
                onChanged()
            },
        )

        // --- about ------------------------------------------------------------
        root.addView(section(activity, palette, R.string.section_about))
        root.addView(
            note(
                activity,
                palette,
                activity.getString(R.string.about_detail, BuildConfig.VERSION_NAME, engineVersion(activity)),
            ),
        )
        root.addView(note(activity, palette, activity.getString(R.string.about_never)))

        val scroller = ScrollView(activity).apply {
            addView(root)
            isFillViewport = true
        }
        dialog.setContentView(scroller)
        dialog.window?.apply {
            setBackgroundDrawable(GlassSurface.panel(activity, GlassGeometry.radiusLarge))
            setLayout(
                (activity.resources.displayMetrics.widthPixels * 0.94f).toInt(),
                (activity.resources.displayMetrics.heightPixels * 0.86f).toInt(),
            )
        }
        return dialog
    }

    // --- the pieces a settings screen is made of -----------------------------

    private fun heading(activity: Activity, palette: GlassPalette, text: String, size: Float) =
        TextView(activity).apply {
            this.text = text
            setTextColor(palette.text)
            textSize = size
            setPadding(0, 0, 0, GlassSurface.dp(activity, 6f).toInt())
        }

    private fun section(activity: Activity, palette: GlassPalette, title: Int) =
        TextView(activity).apply {
            text = activity.getString(title)
            setTextColor(palette.accent)
            textSize = 13f
            setPadding(0, GlassSurface.dp(activity, 18f).toInt(), 0, 0)
        }

    /** The line under a switch that says what it actually does. */
    private fun note(activity: Activity, palette: GlassPalette, text: String) =
        TextView(activity).apply {
            this.text = text
            setTextColor(palette.textMuted)
            textSize = 12f
            setPadding(
                GlassSurface.dp(activity, 4f).toInt(),
                0,
                0,
                GlassSurface.dp(activity, 6f).toInt(),
            )
        }

    private fun row(
        activity: Activity,
        palette: GlassPalette,
        title: String,
        value: String,
    ): LinearLayout {
        val pad = GlassSurface.dp(activity, 10f).toInt()
        val valueView = TextView(activity).apply {
            text = value
            setTextColor(palette.textMuted)
            textSize = 13f
            maxLines = 1
            ellipsize = android.text.TextUtils.TruncateAt.MIDDLE
        }
        return LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(0, pad, 0, pad / 2)
            addView(
                TextView(activity).apply {
                    text = title
                    setTextColor(palette.text)
                    textSize = 15f
                },
            )
            addView(valueView)
            tag = valueView
        }
    }

    private fun value(row: LinearLayout): TextView = row.tag as TextView

    private fun toggle(
        activity: Activity,
        palette: GlassPalette,
        title: String,
        checked: Boolean,
        onChange: (Boolean) -> Unit,
    ): CheckBox = CheckBox(activity).apply {
        text = title
        isChecked = checked
        setTextColor(palette.text)
        textSize = 15f
        gravity = Gravity.CENTER_VERTICAL
        setPadding(paddingLeft, GlassSurface.dp(activity, 8f).toInt(), paddingRight, 0)
        setOnCheckedChangeListener { _, isChecked -> onChange(isChecked) }
    }

    /** Chrome has a slider for this, and it is the accessibility setting people use. */
    private fun textSize(
        activity: Activity,
        palette: GlassPalette,
        settings: Settings,
        onChanged: () -> Unit,
    ): View {
        val label = TextView(activity).apply {
            text = activity.getString(R.string.text_size, settings.textZoom)
            setTextColor(palette.text)
            textSize = 15f
            setPadding(0, GlassSurface.dp(activity, 8f).toInt(), 0, 0)
        }
        val slider = SeekBar(activity).apply {
            max = 150 // 50% to 200%
            progress = settings.textZoom - 50
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(bar: SeekBar, value: Int, fromUser: Boolean) {
                    // Steps of five: the difference between 112% and 113% is not
                    // a difference anyone can see.
                    val percent = ((value + 50) / 5) * 5
                    label.text = activity.getString(R.string.text_size, percent)
                    if (fromUser) {
                        settings.textZoom = percent
                        onChanged()
                    }
                }

                override fun onStartTrackingTouch(bar: SeekBar) = Unit

                override fun onStopTrackingTouch(bar: SeekBar) = Unit
            })
        }
        return LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            addView(label)
            addView(slider)
        }
    }

    // --- the choices ---------------------------------------------------------

    private fun chooseEngine(activity: Activity, settings: Settings, onChosen: () -> Unit) {
        val names = SearchEngines.ALL.map { it.name }.toTypedArray()
        val selected = SearchEngines.ALL.indexOfFirst { it.name == settings.engine.name }
        AlertDialog.Builder(activity)
            .setTitle(R.string.search_engine)
            .setSingleChoiceItems(names, selected) { choice, which ->
                settings.engine = SearchEngines.ALL[which]
                onChosen()
                choice.dismiss()
            }
            .show()
    }

    private fun chooseTheme(activity: Activity, settings: Settings, onChosen: () -> Unit) {
        val choices = Settings.ThemeChoice.entries
        val names = choices.map { themeName(activity, it) }.toTypedArray()
        AlertDialog.Builder(activity)
            .setTitle(R.string.theme)
            .setSingleChoiceItems(names, choices.indexOf(settings.theme)) { choice, which ->
                settings.theme = choices[which]
                onChosen()
                choice.dismiss()
            }
            .show()
    }

    private fun themeName(activity: Activity, choice: Settings.ThemeChoice): String =
        activity.getString(
            when (choice) {
                Settings.ThemeChoice.SYSTEM -> R.string.theme_system
                Settings.ThemeChoice.LIGHT -> R.string.theme_light
                Settings.ThemeChoice.DARK -> R.string.theme_dark
            },
        )

    private fun chooseHomePage(activity: Activity, settings: Settings, onChosen: () -> Unit) {
        val input = EditText(activity).apply {
            setText(settings.homePage)
            setSingleLine()
        }
        val pad = GlassSurface.dp(activity, 20f).toInt()
        val holder = LinearLayout(activity).apply {
            setPadding(pad, pad / 2, pad, 0)
            addView(input)
        }
        AlertDialog.Builder(activity)
            .setTitle(R.string.home_page)
            .setView(holder)
            .setPositiveButton(android.R.string.ok) { _, _ ->
                val resolved = UrlResolver.resolve(input.text.toString(), settings.searchTemplate)
                // Held to the same rule as anything else that gets loaded: a home
                // page is just a URL the browser opens without being asked.
                if (UrlResolver.decide(resolved) == UrlResolver.Decision.RENDER) {
                    settings.homePage = resolved
                    onChosen()
                } else {
                    Toast.makeText(activity, R.string.home_page_rejected, Toast.LENGTH_SHORT).show()
                }
            }
            .setNeutralButton(R.string.reset) { _, _ ->
                settings.resetHomePage()
                onChosen()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun confirmClear(activity: Activity, store: BrowserStore, onCleared: () -> Unit) {
        AlertDialog.Builder(activity)
            .setTitle(R.string.clear_data)
            .setMessage(R.string.clear_data_confirm)
            .setPositiveButton(R.string.clear) { _, _ ->
                clearEverything(store)
                Toast.makeText(activity, R.string.cleared, Toast.LENGTH_SHORT).show()
                onCleared()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    /** Also used on the way out, when "clear on exit" is on. */
    fun clearEverything(store: BrowserStore?) {
        runCatching {
            val cookies = CookieManager.getInstance()
            cookies.removeAllCookies(null)
            cookies.flush()
            WebStorage.getInstance().deleteAllData()
        }
        store?.clearHistory()
    }

    /** Which WebView is rendering, since on Android that is not one thing. */
    private fun engineVersion(activity: Activity): String = try {
        androidx.webkit.WebViewCompat.getCurrentWebViewPackage(activity)?.versionName
            ?: activity.getString(R.string.unknown)
    } catch (_: Exception) {
        activity.getString(R.string.unknown)
    }
}
