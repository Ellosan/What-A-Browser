package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.Dialog
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.widget.BaseAdapter
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView

/**
 * The lists the browser shows over the page: tabs, bookmarks, history.
 *
 * One panel serves all three because they are the same thing — a titled list of
 * pages, each with something to open and sometimes something to remove — and
 * three near-identical layouts would be three places to fix a spacing bug.
 *
 * Built in code rather than XML. These panels are made of a heading, a list and a
 * button; the glass has to be applied from [GlassSurface] at runtime anyway, and
 * a layout file would only add a second place to look.
 */
object Panels {

    class Row(
        val title: String,
        val subtitle: String,
        val current: Boolean = false,
        val onRemove: (() -> Unit)? = null,
        val onClick: () -> Unit,
    )

    /**
     * @param action an optional button along the bottom — "New tab", "Clear history"
     */
    fun list(
        activity: Activity,
        heading: String,
        rows: List<Row>,
        emptyMessage: String,
        action: Pair<String, () -> Unit>? = null,
    ): Dialog {
        val palette = GlassSurface.palette(activity)
        val pad = GlassSurface.dp(activity, 18f).toInt()

        val dialog = Dialog(activity)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)

        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad / 2)
        }

        root.addView(
            TextView(activity).apply {
                text = heading
                setTextColor(palette.text)
                textSize = 18f
                setPadding(pad / 2, 0, 0, pad / 2)
            },
        )

        if (rows.isEmpty()) {
            root.addView(
                TextView(activity).apply {
                    text = emptyMessage
                    setTextColor(palette.textMuted)
                    textSize = 15f
                    setPadding(pad / 2, pad, pad / 2, pad)
                },
            )
        } else {
            val adapter = RowAdapter(activity, rows.toMutableList(), palette) { dialog.dismiss() }
            root.addView(
                ListView(activity).apply {
                    this.adapter = adapter
                    divider = null
                    dividerHeight = 0
                    // A dialog sizes itself to its content, so the list needs a
                    // ceiling of its own or forty tabs would run off the screen.
                    layoutParams = LinearLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        minOf(
                            rows.size * GlassSurface.dp(activity, 62f).toInt(),
                            (activity.resources.displayMetrics.heightPixels * 0.55f).toInt(),
                        ),
                    )
                },
            )
        }

        action?.let { (label, onAction) ->
            root.addView(
                Button(activity).apply {
                    text = label
                    setTextColor(palette.accent)
                    background = null
                    gravity = Gravity.START or Gravity.CENTER_VERTICAL
                    setOnClickListener {
                        dialog.dismiss()
                        onAction()
                    }
                },
            )
        }

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

    private class RowAdapter(
        private val activity: Activity,
        private val rows: MutableList<Row>,
        private val palette: GlassPalette,
        private val dismiss: () -> Unit,
    ) : BaseAdapter() {

        override fun getCount() = rows.size

        override fun getItem(position: Int) = rows[position]

        override fun getItemId(position: Int) = position.toLong()

        override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
            val row = rows[position]
            val pad = GlassSurface.dp(activity, 10f).toInt()

            val title = TextView(activity).apply {
                text = row.title
                // The tab being looked at is marked, or a switcher is a list of
                // pages with no you-are-here.
                setTextColor(if (row.current) palette.accent else palette.text)
                textSize = 15f
                maxLines = 1
                ellipsize = android.text.TextUtils.TruncateAt.END
            }
            val subtitle = TextView(activity).apply {
                text = row.subtitle
                setTextColor(palette.textMuted)
                textSize = 12f
                maxLines = 1
                ellipsize = android.text.TextUtils.TruncateAt.MIDDLE
            }
            val column = LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                addView(title)
                addView(subtitle)
                setOnClickListener {
                    dismiss()
                    row.onClick()
                }
            }

            val holder = LinearLayout(activity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(pad, pad, pad, pad)
                addView(column)
            }

            row.onRemove?.let { remove ->
                holder.addView(
                    TextView(activity).apply {
                        text = activity.getString(R.string.close_glyph)
                        contentDescription = activity.getString(R.string.close)
                        setTextColor(palette.textMuted)
                        textSize = 18f
                        setBackgroundColor(Color.TRANSPARENT)
                        setPadding(pad, pad / 2, pad, pad / 2)
                        setOnClickListener {
                            // Removing keeps the panel open: closing four tabs
                            // should not mean opening the switcher four times.
                            val index = rows.indexOf(row)
                            if (index >= 0) {
                                rows.removeAt(index)
                                notifyDataSetChanged()
                            }
                            remove()
                            if (rows.isEmpty()) dismiss()
                        }
                    },
                )
            }
            return holder
        }
    }
}
