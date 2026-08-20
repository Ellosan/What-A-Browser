package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.Dialog
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.widget.BaseAdapter
import android.widget.Button
import android.widget.CheckBox
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView

/**
 * Rearranging the menu.
 *
 * Arrows rather than drag-and-drop. Dragging a row to reorder a list needs a
 * touch handler that fights the list's own scrolling, and this is a screen most
 * people open once; two buttons that always work are worth more here than a
 * gesture that sometimes does.
 *
 * Every change is saved as it is made — there is no "apply" — so closing the
 * panel by any route leaves the menu the way it looks on screen.
 */
object MenuPanel {

    fun show(activity: Activity, settings: Settings, onChanged: () -> Unit): Dialog {
        val palette = GlassSurface.palette(activity)
        val pad = GlassSurface.dp(activity, 18f).toInt()
        val layout = MenuLayout.parse(settings.menuLayout)

        val dialog = Dialog(activity)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)

        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad / 2)
        }

        root.addView(
            TextView(activity).apply {
                text = activity.getString(R.string.customize_menu)
                setTextColor(palette.text)
                textSize = 18f
                setPadding(pad / 2, 0, 0, pad / 2)
            },
        )

        val save = {
            settings.menuLayout = layout.serialize()
            onChanged()
        }
        val adapter = Adapter(activity, layout, palette, save)

        root.addView(
            ListView(activity).apply {
                this.adapter = adapter
                divider = null
                dividerHeight = 0
                layoutParams = LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    (activity.resources.displayMetrics.heightPixels * 0.55f).toInt(),
                )
            },
        )

        root.addView(
            Button(activity).apply {
                text = activity.getString(R.string.reset_menu)
                setTextColor(palette.accent)
                background = null
                gravity = Gravity.START or Gravity.CENTER_VERTICAL
                setOnClickListener {
                    layout.reset()
                    save()
                    adapter.notifyDataSetChanged()
                }
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

    private class Adapter(
        private val activity: Activity,
        private val model: MenuLayout,
        private val palette: GlassPalette,
        private val save: () -> Unit,
    ) : BaseAdapter() {

        override fun getCount() = model.items().size

        override fun getItem(position: Int) = model.items()[position]

        override fun getItemId(position: Int) = position.toLong()

        override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
            val item = model.items()[position]
            val pad = GlassSurface.dp(activity, 8f).toInt()
            val fixed = item.id in MenuLayout.ALWAYS

            val tick = CheckBox(activity).apply {
                text = activity.getString(MenuCatalog.labelRes(item.id))
                isChecked = item.visible
                // The items that lead back to this screen cannot be hidden, and
                // showing that as a disabled tick explains it without a sentence.
                isEnabled = !fixed
                setTextColor(if (fixed) palette.textMuted else palette.text)
                textSize = 15f
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                setOnClickListener {
                    model.toggle(item.id)
                    save()
                    notifyDataSetChanged()
                }
            }

            return LinearLayout(activity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(pad, pad / 2, pad, pad / 2)
                addView(tick)
                addView(arrow(R.string.up_glyph, R.string.move_up) { model.moveUp(item.id) })
                addView(arrow(R.string.down_glyph, R.string.move_down) { model.moveDown(item.id) })
            }
        }

        private fun arrow(glyph: Int, description: Int, move: () -> Unit): TextView {
            val pad = GlassSurface.dp(activity, 12f).toInt()
            return TextView(activity).apply {
                text = activity.getString(glyph)
                contentDescription = activity.getString(description)
                setTextColor(palette.textMuted)
                textSize = 14f
                setPadding(pad, pad / 2, pad, pad / 2)
                setOnClickListener {
                    move()
                    save()
                    notifyDataSetChanged()
                }
            }
        }
    }
}
