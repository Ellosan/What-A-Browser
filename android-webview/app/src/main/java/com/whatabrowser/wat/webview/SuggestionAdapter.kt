package com.whatabrowser.wat.webview

import android.content.Context
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.BaseAdapter
import android.widget.Filter
import android.widget.Filterable
import android.widget.LinearLayout
import android.widget.TextView

/**
 * What the address bar offers as you type: bookmarks first, then history.
 *
 * The query runs against SQLite, and `AutoCompleteTextView` calls
 * [Filter.performFiltering] on a worker thread of its own — which is the whole
 * reason the suggestions are done this way rather than by watching the text and
 * querying directly. Typing must not wait for a disk read.
 *
 * Nothing is sent anywhere. These come from what this browser has already
 * visited; there is no suggestion service, and a search engine is not told what
 * is being typed until the reader presses go.
 */
class SuggestionAdapter(
    private val context: Context,
    private val store: BrowserStore,
) : BaseAdapter(), Filterable {

    private var rows: List<BrowserStore.Entry> = emptyList()

    override fun getCount() = rows.size

    override fun getItem(position: Int): BrowserStore.Entry = rows[position]

    override fun getItemId(position: Int) = position.toLong()

    override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
        val entry = rows[position]
        val palette = GlassSurface.palette(context)
        val pad = GlassSurface.dp(context, 10f).toInt()

        return LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(pad, pad, pad, pad)
            setBackgroundColor(palette.surfaceRaised)
            addView(
                TextView(context).apply {
                    text = if (entry.bookmarked) {
                        context.getString(R.string.bookmarked_label, entry.label)
                    } else {
                        entry.label
                    }
                    setTextColor(palette.text)
                    textSize = 15f
                    maxLines = 1
                    ellipsize = android.text.TextUtils.TruncateAt.END
                },
            )
            addView(
                TextView(context).apply {
                    text = entry.url
                    setTextColor(palette.textMuted)
                    textSize = 12f
                    maxLines = 1
                    ellipsize = android.text.TextUtils.TruncateAt.MIDDLE
                },
            )
        }
    }

    override fun getFilter(): Filter = object : Filter() {

        override fun performFiltering(constraint: CharSequence?): FilterResults {
            val query = constraint?.toString()?.trim().orEmpty()
            val found = if (query.length < MIN_QUERY) emptyList() else store.suggest(query)
            return FilterResults().apply {
                values = found
                count = found.size
            }
        }

        override fun publishResults(constraint: CharSequence?, results: FilterResults) {
            @Suppress("UNCHECKED_CAST")
            rows = results.values as? List<BrowserStore.Entry> ?: emptyList()
            if (rows.isEmpty()) notifyDataSetInvalidated() else notifyDataSetChanged()
        }

        /** What lands in the address bar when a suggestion is picked. */
        override fun convertResultToString(resultValue: Any?): CharSequence =
            (resultValue as? BrowserStore.Entry)?.url ?: ""
    }

    private companion object {
        /** One letter matches most of the history and helps nobody. */
        const val MIN_QUERY = 2
    }
}
