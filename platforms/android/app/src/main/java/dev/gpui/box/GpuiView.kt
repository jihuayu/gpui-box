package dev.gpui.box

import android.content.Context
import android.text.InputType
import android.util.SparseLongArray
import android.view.Choreographer
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager

class GpuiView(context: Context) : SurfaceView(context), SurfaceHolder.Callback {
    var generation = 0L
        private set
    private var attached = false
    private var resumed = false
    private var pendingFrame = false
    private val contacts = SparseLongArray()
    private val choreographer = Choreographer.getInstance()
    private val frameCallback = Choreographer.FrameCallback {
        pendingFrame = false
        if (attached && resumed) NativeBridge.frame(generation)
    }
    private val ime get() = context.getSystemService(InputMethodManager::class.java)

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        holder.addCallback(this)
    }
    override fun surfaceCreated(holder: SurfaceHolder) { /* Size arrives in surfaceChanged. */ }
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        NativeBridge.checkMain()
        cancelContacts()
        if (attached) {
            NativeBridge.resize(generation, width, height, resources.displayMetrics.density)
        } else {
            generation = NativeBridge.nextGeneration()
            NativeBridge.attach(holder.surface, generation, width, height, resources.displayMetrics.density)
            attached = true
        }
        NativeBridge.lifecycle(generation, resumed)
        scheduleFrame()
    }
    override fun surfaceDestroyed(holder: SurfaceHolder) {
        NativeBridge.checkMain()
        cancelContacts()
        choreographer.removeFrameCallback(frameCallback)
        pendingFrame = false
        // Synchronous JNI barrier. Native code must stop using this Surface.
        if (attached) NativeBridge.detach(generation)
        attached = false
    }
    fun setResumed(value: Boolean) {
        resumed = value
        if (attached) NativeBridge.lifecycle(generation, value)
        if (value) scheduleFrame() else cancelContacts()
    }
    fun scheduleFrame() {
        if (attached && resumed && !pendingFrame) {
            pendingFrame = true
            choreographer.postFrameCallback(frameCallback)
        }
    }
    private fun cancelContacts() {
        for (index in 0 until contacts.size()) {
            NativeBridge.touch(generation, contacts.valueAt(index), 3, 0f, 0f)
        }
        contacts.clear()
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!attached) return false
        val action = event.actionMasked
        if (action == MotionEvent.ACTION_CANCEL) { cancelContacts(); return true }
        if (action == MotionEvent.ACTION_DOWN) requestFocus()
        val indices = if (action == MotionEvent.ACTION_MOVE) (0 until event.pointerCount).toList() else listOf(event.actionIndex)
        for (index in indices) {
            val pointer = event.getPointerId(index)
            val phase = when (action) {
                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> 0
                MotionEvent.ACTION_MOVE -> 1
                MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> 2
                else -> continue
            }
            if (phase == 0) contacts.put(pointer, NativeBridge.nextContact())
            val id = contacts.get(pointer, 0L)
            if (id != 0L) NativeBridge.touch(generation, id, phase, event.getX(index), event.getY(index))
            if (phase == 2) contacts.delete(pointer)
        }
        scheduleFrame()
        return true
    }
    override fun onCheckIsTextEditor() = true
    override fun onCreateInputConnection(info: EditorInfo): InputConnection? {
        if (!attached) return null
        info.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
        info.imeOptions = EditorInfo.IME_ACTION_DONE or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        val state = NativeBridge.textState(generation)
        info.initialSelStart = state[0]
        info.initialSelEnd = state[1]
        return GpuiInputConnection(this, generation)
    }
    fun showKeyboard() { requestFocus(); ime.restartInput(this); ime.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT) }
    fun hideKeyboard() { ime.hideSoftInputFromWindow(windowToken, 0) }
    fun updateSelection() {
        if (!attached) return
        val state = NativeBridge.textState(generation)
        ime.updateSelection(this, state[0], state[1], state[2], state[3])
    }
    override fun onKeyDown(code: Int, event: KeyEvent): Boolean {
        if (!attached) return super.onKeyDown(code, event)
        if (code == KeyEvent.KEYCODE_DEL) {
            val state = NativeBridge.textState(generation)
            val connection = GpuiInputConnection(this, generation)
            if (state[0] != state[1]) connection.commitText("", 1)
            else connection.deleteSurroundingTextInCodePoints(1, 0)
            return true
        }
        val character = event.unicodeChar
        if (character > 0 && !event.isCtrlPressed) {
            NativeBridge.edit(generation, 0, String(Character.toChars(character)), 1, 0)
            return true
        }
        return super.onKeyDown(code, event)
    }
}
