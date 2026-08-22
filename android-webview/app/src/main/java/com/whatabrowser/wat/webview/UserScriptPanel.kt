package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.AlertDialog
import android.app.Dialog
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.widget.BaseAdapter
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView
import android.widget.Toast
import java.util.concurrent.Executors

/**
 * Installing and managing userscripts.
 *
 * A script can come from an address or be pasted in. Either way it is read,
 * parsed, and shown for what it is — its name, its version, and the sites it
 * says it will run on — before anything is stored, because "which sites" is the
 * whole question with someone else's code.
 *
 * Installing is the only place this browser takes code from the network on
 * purpose, so the fetch is HTTPS-only, size-capped, and happens once: after it,
 * the copy on disk is what runs. Nothing is re-downloaded, and there is no
 * auto-update — a script that updated itself would be a different program than
 * the one that was read.
 */
object UserScriptPanel {

    private val worker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "userscript-install").apply { isDaemon = true }
    }

    fun show(activity: Activity, settings: Settings, onChanged: () -> Unit): Dialog {
        val palette = GlassSurface.palette(activity)
        val pad = GlassSurface.dp(activity, 18f).toInt()
        val store = UserScriptStore(activity)

        val dialog = Dialog(activity)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)

        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad / 2)
        }
        root.addView(
            TextView(activity).apply {
                text = activity.getString(R.string.manage_userscripts)
                setTextColor(palette.text)
                textSize = 18f
            },
        )
        root.addView(
            TextView(activity).apply {
                text = activity.getString(R.string.userscripts_note)
                setTextColor(palette.textMuted)
                textSize = 12f
                setPadding(0, GlassSurface.dp(activity, 4f).toInt(), 0, pad / 2)
            },
        )

        val adapter = Adapter(activity, store, palette, onChanged)
        root.addView(
            ListView(activity).apply {
                this.adapter = adapter
                divider = null
                dividerHeight = 0
                layoutParams = LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    (activity.resources.displayMetrics.heightPixels * 0.45f).toInt(),
                )
            },
        )

        root.addView(
            Button(activity).apply {
                text = activity.getString(R.string.install_from_address)
                setTextColor(palette.accent)
                background = null
                gravity = Gravity.START or Gravity.CENTER_VERTICAL
                setOnClickListener {
                    installFromAddress(activity, store) {
                        adapter.reload()
                        settings.userScripts = true
                        onChanged()
                    }
                }
            },
        )
        root.addView(
            Button(activity).apply {
                text = activity.getString(R.string.paste_a_script)
                setTextColor(palette.accent)
                background = null
                gravity = Gravity.START or Gravity.CENTER_VERTICAL
                setOnClickListener {
                    pasteScript(activity, store) {
                        adapter.reload()
                        settings.userScripts = true
                        onChanged()
                    }
                }
            },
        )

        dialog.setContentView(root)
        dialog.window?.apply {
            setBackgroundDrawable(GlassSurface.panel(activity, GlassGeometry.radiusLarge))
            setLayout(
                (activity.resources.displayMetrics.widthPixels * 0.94f).toInt(),
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
        }
        return dialog
    }

    private fun installFromAddress(activity: Activity, store: UserScriptStore, onInstalled: () -> Unit) {
        val input = EditText(activity).apply {
            hint = activity.getString(R.string.userscript_address_hint)
            setSingleLine()
        }
        val pad = GlassSurface.dp(activity, 20f).toInt()
        AlertDialog.Builder(activity)
            .setTitle(R.string.install_from_address)
            .setView(LinearLayout(activity).apply {
                setPadding(pad, pad / 2, pad, 0)
                addView(input)
            })
            .setPositiveButton(R.string.fetch) { _, _ ->
                val address = input.text.toString().trim()
                Toast.makeText(activity, R.string.fetching, Toast.LENGTH_SHORT).show()
                worker.execute {
                    val result = store.installFrom(address)
                    activity.runOnUiThread {
                        result.fold(
                            onSuccess = { script ->
                                confirm(activity, script)
                                onInstalled()
                            },
                            onFailure = { failure ->
                                AlertDialog.Builder(activity)
                                    .setTitle(R.string.userscript_not_installed)
                                    .setMessage(failure.message ?: failure.toString())
                                    .setPositiveButton(android.R.string.ok, null)
                                    .show()
                            },
                        )
                    }
                }
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun pasteScript(activity: Activity, store: UserScriptStore, onInstalled: () -> Unit) {
        val input = EditText(activity).apply {
            hint = activity.getString(R.string.paste_hint)
            minLines = 6
            maxLines = 12
            gravity = Gravity.TOP or Gravity.START
        }
        val pad = GlassSurface.dp(activity, 20f).toInt()
        AlertDialog.Builder(activity)
            .setTitle(R.string.paste_a_script)
            .setView(LinearLayout(activity).apply {
                setPadding(pad, pad / 2, pad, 0)
                addView(input)
            })
            .setPositiveButton(R.string.install) { _, _ ->
                val script = store.install(input.text.toString())
                if (script == null) {
                    Toast.makeText(activity, R.string.not_a_userscript, Toast.LENGTH_LONG).show()
                } else {
                    confirm(activity, script)
                    onInstalled()
                }
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    /**
     * What was installed, and the two things worth knowing about it: where it
     * runs, and anything it asked for that this browser does not have.
     */
    private fun confirm(activity: Activity, script: UserScript) {
        val missing = script.unsupportedGrants()
        val message = buildString {
            append(activity.getString(R.string.installed_runs_on, script.matches.joinToString(", ")))
            if (missing.isNotEmpty()) {
                append("\n\n")
                append(activity.getString(R.string.installed_missing, missing.joinToString(", ")))
            }
        }
        AlertDialog.Builder(activity)
            .setTitle(script.name + " " + script.version)
            .setMessage(message)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }

    private class Adapter(
        private val activity: Activity,
        private val store: UserScriptStore,
        private val palette: GlassPalette,
        private val onChanged: () -> Unit,
    ) : BaseAdapter() {

        private var rows = store.all()

        fun reload() {
            rows = store.all()
            notifyDataSetChanged()
        }

        override fun getCount() = rows.size

        override fun getItem(position: Int) = rows[position]

        override fun getItemId(position: Int) = position.toLong()

        override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
            val installed = rows[position]
            val pad = GlassSurface.dp(activity, 8f).toInt()

            val tick = CheckBox(activity).apply {
                text = installed.script.name + "  " + installed.script.version
                isChecked = installed.enabled
                setTextColor(palette.text)
                textSize = 15f
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                setOnCheckedChangeListener { _, checked ->
                    store.setEnabled(installed, checked)
                    onChanged()
                }
            }
            val where = TextView(activity).apply {
                text = installed.script.matches.joinToString(", ")
                setTextColor(palette.textMuted)
                textSize = 11f
                maxLines = 1
                ellipsize = android.text.TextUtils.TruncateAt.END
            }
            val remove = TextView(activity).apply {
                text = activity.getString(R.string.close_glyph)
                contentDescription = activity.getString(R.string.remove)
                setTextColor(palette.textMuted)
                textSize = 17f
                setPadding(pad, pad / 2, pad, pad / 2)
                setOnClickListener {
                    store.remove(installed)
                    reload()
                    onChanged()
                }
            }

            return LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(pad, pad / 2, pad, pad / 2)
                addView(
                    LinearLayout(activity).apply {
                        orientation = LinearLayout.HORIZONTAL
                        gravity = Gravity.CENTER_VERTICAL
                        addView(tick)
                        addView(remove)
                    },
                )
                addView(where)
            }
        }
    }
}
