package dev.gpui.box

import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest

/** Rust owns text. This adapter never keeps a second Editable model. */
class GpuiInputConnection(private val view: GpuiView, private val session: Long) : BaseInputConnection(view, false) {
    private fun valid() = view.generation == session
    private fun state() = NativeBridge.textState(session)
    private fun edit(operation: Int, text: CharSequence = "", a: Int = 0, b: Int = 0): Boolean {
        if (!valid()) return false
        NativeBridge.edit(session, operation, text.toString(), a, b)
        view.scheduleFrame()
        return true
    }
    override fun commitText(text: CharSequence, newCursorPosition: Int) = edit(0, text, newCursorPosition)
    override fun setComposingText(text: CharSequence, newCursorPosition: Int) = edit(1, text, newCursorPosition)
    override fun finishComposingText() = edit(2)
    override fun setSelection(start: Int, end: Int) = edit(3, a = start, b = end)
    override fun setComposingRegion(start: Int, end: Int) = edit(5, a = start, b = end)
    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int) = edit(4, a = beforeLength, b = afterLength)
    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        if (beforeLength < 0 || afterLength < 0 || !valid()) return false
        val text = NativeBridge.text(session)
        val selection = state()
        val start = selection[0].coerceIn(0, text.length)
        val end = selection[1].coerceIn(start, text.length)
        val before = beforeLength.coerceAtMost(text.codePointCount(0, start))
        val after = afterLength.coerceAtMost(text.codePointCount(end, text.length))
        return deleteSurroundingText(start - text.offsetByCodePoints(start, -before),
            text.offsetByCodePoints(end, after) - end)
    }
    override fun getTextBeforeCursor(length: Int, flags: Int): CharSequence {
        val text = NativeBridge.text(session); val start = state()[0].coerceIn(0, text.length)
        return text.substring((start - length.coerceAtLeast(0)).coerceAtLeast(0), start)
    }
    override fun getTextAfterCursor(length: Int, flags: Int): CharSequence {
        val text = NativeBridge.text(session); val end = state()[1].coerceIn(0, text.length)
        return text.substring(end, (end.toLong() + length.coerceAtLeast(0)).coerceAtMost(text.length.toLong()).toInt())
    }
    override fun getSelectedText(flags: Int): CharSequence {
        val text = NativeBridge.text(session); val state = state()
        return text.substring(state[0].coerceIn(0, text.length), state[1].coerceIn(0, text.length))
    }
    override fun getExtractedText(request: ExtractedTextRequest?, flags: Int): ExtractedText {
        val state = state()
        return ExtractedText().apply {
            text = NativeBridge.text(session); startOffset = 0
            partialStartOffset = -1; partialEndOffset = -1
            selectionStart = state[0]; selectionEnd = state[1]
        }
    }
    override fun sendKeyEvent(event: KeyEvent): Boolean {
        if (event.action == KeyEvent.ACTION_DOWN) return view.onKeyDown(event.keyCode, event)
        return true
    }
    override fun beginBatchEdit() = true
    override fun endBatchEdit() = true
}
