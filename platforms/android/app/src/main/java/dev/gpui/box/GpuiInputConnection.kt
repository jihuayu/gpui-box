package dev.gpui.box

import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest

/** Rust owns text. Connections are scoped to a surface, focus epoch, and host connection. */
class GpuiInputConnection(
    private val view: GpuiView,
    private val surface: Long,
    private val session: Long,
) : BaseInputConnection(view, false) {
    private var closed = false
    private var batchDepth = 0
    val isBatchEditing get() = batchDepth > 0

    private fun valid() = !closed && view.acceptsInput(this, surface, session)
    private fun state() = NativeBridge.textState(surface)
    private fun edit(operation: Int, text: CharSequence = "", a: Int = 0, b: Int = 0): Boolean {
        if (!valid()) return false
        val accepted = NativeBridge.edit(surface, session, operation, text.toString(), a, b)
        if (accepted) view.scheduleFrame()
        return accepted
    }
    override fun commitText(text: CharSequence, newCursorPosition: Int) = edit(0, text, newCursorPosition)
    override fun setComposingText(text: CharSequence, newCursorPosition: Int) = edit(1, text, newCursorPosition)
    override fun finishComposingText() = edit(2)
    override fun setSelection(start: Int, end: Int) = edit(3, a = start, b = end)
    override fun setComposingRegion(start: Int, end: Int) = edit(5, a = start, b = end)
    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength < 0 || afterLength < 0) return false
        return edit(4, a = beforeLength, b = afterLength)
    }
    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength < 0 || afterLength < 0 || !valid()) return false
        val text = NativeBridge.text(surface)
        val selection = state()
        val start = selection[0].coerceIn(0, text.length)
        val end = selection[1].coerceIn(start, text.length)
        val before = beforeLength.coerceAtMost(text.codePointCount(0, start))
        val after = afterLength.coerceAtMost(text.codePointCount(end, text.length))
        return deleteSurroundingText(
            start - text.offsetByCodePoints(start, -before),
            text.offsetByCodePoints(end, after) - end,
        )
    }
    override fun getTextBeforeCursor(length: Int, flags: Int): CharSequence? {
        if (!valid()) return null
        val text = NativeBridge.text(surface)
        val start = state()[0].coerceIn(0, text.length)
        return text.substring((start - length.coerceAtLeast(0)).coerceAtLeast(0), start)
    }
    override fun getTextAfterCursor(length: Int, flags: Int): CharSequence? {
        if (!valid()) return null
        val text = NativeBridge.text(surface)
        val end = state()[1].coerceIn(0, text.length)
        val limit = (end.toLong() + length.coerceAtLeast(0)).coerceAtMost(text.length.toLong()).toInt()
        return text.substring(end, limit)
    }
    override fun getSelectedText(flags: Int): CharSequence? {
        if (!valid()) return null
        val text = NativeBridge.text(surface)
        val selection = state()
        return text.substring(selection[0].coerceIn(0, text.length), selection[1].coerceIn(0, text.length))
    }
    override fun getExtractedText(request: ExtractedTextRequest?, flags: Int): ExtractedText? {
        if (!valid()) return null
        val selection = state()
        return ExtractedText().apply {
            text = NativeBridge.text(surface)
            startOffset = 0
            partialStartOffset = -1
            partialEndOffset = -1
            selectionStart = selection[0]
            selectionEnd = selection[1]
        }
    }
    override fun performContextMenuAction(id: Int): Boolean {
        if (!valid()) return false
        val operation = when (id) {
            android.R.id.selectAll -> 0
            android.R.id.copy -> 1
            android.R.id.cut -> 2
            android.R.id.paste, android.R.id.pasteAsPlainText -> 3
            else -> return false
        }
        return NativeBridge.clipboardAction(surface, session, operation)
    }
    override fun sendKeyEvent(event: KeyEvent): Boolean {
        if (!valid()) return false
        if (event.action == KeyEvent.ACTION_DOWN) return view.onKeyDown(event.keyCode, event)
        return true
    }
    override fun beginBatchEdit(): Boolean {
        if (!valid()) return false
        batchDepth += 1
        return true
    }
    override fun endBatchEdit(): Boolean {
        if (!valid() || batchDepth == 0) return false
        batchDepth -= 1
        if (batchDepth == 0) view.updateSelection()
        return batchDepth > 0
    }
    override fun closeConnection() {
        if (valid()) NativeBridge.edit(surface, session, 2, "", 0, 0)
        closed = true
        batchDepth = 0
        super.closeConnection()
    }
}
