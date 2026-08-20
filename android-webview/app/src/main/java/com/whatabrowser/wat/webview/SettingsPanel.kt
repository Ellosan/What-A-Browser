package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.AlertDialog
import android.app.Dialog
import android.view.Gravity
import android.view.ViewGroup
import android.view.Window
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/**
 * Settings: the search engine, the home page, and getting rid of what the
 * browsing left behind.
 *
 * Short on purpose. Everything that decides whether this browser is safe —
 * certificate handling, mixed content, file access, third-party cookies — is in
 * [SecureWebView] and is not offered here, because a switch that weakens the
 * browser is a switch that eventually gets flipped and then forgotten about.
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
            setPadding(pad, pad, pad, pad)
        }

        root.addView(
            TextView(activity).apply {
                text = activity.getString(R.string.settings)
                setTextColor(palette.text)
                textSize = 18f
                setPadding(0, 0, 0, pad / 2)
            },
        )

        val engineRow = row(activity, palette, activity.getString(R.string.search_engine), settings.engine.name)
        val homeRow = row(activity, palette, activity.getString(R.string.home_page), settings.homePage)

        engineRow.setOnClickListener {
            chooseEngine(activity, settings) {
                value(engineRow).text = settings.engine.name
                // The home page follows the engine until it is set by hand, so
                // its line moves too.
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
        root.addView(engineRow)
        root.addView(homeRow)

        root.addView(
            toggle(activity, palette, activity.getString(R.string.desktop_by_default), settings.desktopSite) {
                settings.desktopSite = it
            },
        )
        root.addView(
            toggle(activity, palette, activity.getString(R.string.keep_history), settings.keepHistory) {
                settings.keepHistory = it
                if (!it) store.clearHistory()
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

        root.addView(
            TextView(activity).apply {
                text = activity.getString(R.string.about_detail, BuildConfig.VERSION_NAME, engineVersion(activity))
                setTextColor(palette.textMuted)
                textSize = 12f
                setPadding(0, pad, 0, 0)
            },
        )

        dialog.setContentView(root)
        dialog.window?.apply {
            setBackgroundDrawable(GlassSurface.panel(activity, GlassGeometry.radiusLarge))
            setLayout(
                (activity.resources.displayMetrics.widthPixels * 0.92f).toInt(),
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
        }
        return dialog
    }

    /** A title with its current value underneath, and something to tap. */
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
            // Held on the row so a setting that changes can update its line
            // without the panel being rebuilt underneath the reader.
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
        setPadding(paddingLeft, GlassSurface.dp(activity, 10f).toInt(), paddingRight, 0)
        setOnCheckedChangeListener { _, isChecked -> onChange(isChecked) }
    }

    private fun chooseEngine(activity: Activity, settings: Settings, onChosen: () -> Unit) {
        val names = SearchEngines.ALL.map { it.name }.toTypedArray()
        val selected = SearchEngines.ALL.indexOfFirst { it.name == settings.engine.name }
        AlertDialog.Builder(activity)
            .setTitle(R.string.search_engine)
            .setSingleChoiceItems(names, selected) { choice, which ->
                settings.engine = SearchEngines.ALL[which]
                // The home page follows the engine unless it has been set by hand,
                // so choosing an engine moves the home button with it.
                onChosen()
                choice.dismiss()
            }
            .show()
    }

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
                val cookies = CookieManager.getInstance()
                cookies.removeAllCookies(null)
                cookies.flush()
                WebStorage.getInstance().deleteAllData()
                store.clearHistory()
                Toast.makeText(activity, R.string.cleared, Toast.LENGTH_SHORT).show()
                onCleared()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    /** Which WebView is rendering, since on Android that is not one thing. */
    private fun engineVersion(activity: Activity): String = try {
        androidx.webkit.WebViewCompat.getCurrentWebViewPackage(activity)?.versionName
            ?: activity.getString(R.string.unknown)
    } catch (_: Exception) {
        activity.getString(R.string.unknown)
    }
}
