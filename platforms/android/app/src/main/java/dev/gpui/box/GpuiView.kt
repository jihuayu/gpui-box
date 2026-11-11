package dev.gpui.box

import android.content.Context
import android.graphics.Matrix
import android.util.SparseLongArray
import android.view.Choreographer
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.view.inputmethod.CursorAnchorInfo

class GpuiView(context: Context) : SurfaceView(context), SurfaceHolder.Callback {
    var generation = 0L
        private set
    private var attached = false
    private var resumed = false
    private var pendingFrame = false
    private var inputSession = 0L
    private var connection: GpuiInputConnection? = null
    private var monitorCursor = false
    private val accessibility = GpuiAccessibility(this)
    private val contacts = SparseLongArray()
    private val choreographer = Choreographer.getInstance()
    private val frameCallback = Choreographer.FrameCallback {
        pendingFrame = false
        if (attached && resumed) {
            NativeBridge.frame(generation)
            (context as? MainActivity)?.deliverPendingUrls()
            if (monitorCursor) updateCursorAnchor()
        }
    }
    private val ime get() = context.getSystemService(InputMethodManager::class.java)

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        holder.addCallback(this)
    }
    override fun onAttachedToWindow() { super.onAttachedToWindow(); accessibility.attach() }
    override fun onDetachedFromWindow() { accessibility.detach(); super.onDetachedFromWindow() }
    override fun getAccessibilityNodeProvider() = accessibility
    override fun dispatchHoverEvent(event: MotionEvent): Boolean = accessibility.hover(event) || super.dispatchHoverEvent(event)
    fun updateAccessibility() { if (attached) accessibility.refresh() }
    override fun surfaceCreated(holder: SurfaceHolder) { /* Size arrives in surfaceChanged. */ }
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        NativeBridge.checkMain()
        cancelContacts()
        if (attached) {
            NativeBridge.resize(generation, width, height, resources.displayMetrics.density)
        } else {
            generation = NativeBridge.nextGeneration()
            NativeBridge.surfaceAttached(this, generation)
            NativeBridge.attach(holder.surface, generation, width, height, resources.displayMetrics.density)
            attached = true
        }
        NativeBridge.lifecycle(generation, resumed)
        (context as? MainActivity)?.updateHostState()
        updateAccessibility()
        (context as? MainActivity)?.updateBackRegistration()
        scheduleFrame()
    }
    override fun surfaceDestroyed(holder: SurfaceHolder) {
        NativeBridge.checkMain()
        cancelContacts()
        choreographer.removeFrameCallback(frameCallback)
        pendingFrame = false
        closeInputConnection()
        // Synchronous JNI barrier. Native code must stop using this Surface.
        if (attached) NativeBridge.detach(generation)
        attached = false
        NativeBridge.surfaceDetached(generation)
        (context as? MainActivity)?.updateBackRegistration()
    }
    fun setResumed(value: Boolean) {
        if (!value) closeInputConnection()
        resumed = value
        if (attached) NativeBridge.lifecycle(generation, value)
        if (value) {
            scheduleFrame()
            syncInputSession()
        } else {
            cancelContacts()
        }
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
        NativeBridge.checkMain()
        if (!attached || !resumed) return false
        val action = event.actionMasked
        if (action == MotionEvent.ACTION_CANCEL) { cancelContacts(); return true }
        if (action == MotionEvent.ACTION_DOWN) {
            // A replacement stream must release every old GPUI contact even if
            // Android or a parent omitted its previous UP/CANCEL callback.
            cancelContacts()
            requestFocus()
        }
        val indices = if (action == MotionEvent.ACTION_MOVE) (0 until event.pointerCount).toList() else listOf(event.actionIndex)
        for (index in indices) {
            val pointer = event.getPointerId(index)
            val phase = when (action) {
                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> 0
                MotionEvent.ACTION_MOVE -> 1
                MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> 2
                else -> continue
            }
            if (phase == 0) {
                val previous = contacts.get(pointer, 0L)
                if (previous != 0L) NativeBridge.touch(generation, previous, 3, 0f, 0f)
                contacts.put(pointer, NativeBridge.nextContact())
            }
            val id = contacts.get(pointer, 0L)
            if (id != 0L) NativeBridge.touch(generation, id, phase, event.getX(index), event.getY(index))
            if (phase == 2) contacts.delete(pointer)
        }
        scheduleFrame()
        return true
    }
    override fun onCheckIsTextEditor(): Boolean {
        NativeBridge.checkMain()
        return attached && resumed && NativeBridge.inputSession(generation) != 0L
    }
    override fun onCreateInputConnection(info: EditorInfo): InputConnection? {
        NativeBridge.checkMain()
        val session = if (attached && resumed) NativeBridge.inputSession(generation) else 0L
        closeInputConnection()
        if (session == 0L) return null
        inputSession = session
        val configuration = NativeBridge.editorInfo(generation)
        info.inputType = configuration[0]
        info.imeOptions = configuration[1] or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        val state = NativeBridge.textState(generation)
        info.initialSelStart = state[0]
        info.initialSelEnd = state[1]
        return GpuiInputConnection(this, generation, session).also { connection = it }
    }
    private fun closeInputConnection() {
        connection?.closeConnection()
        connection = null
        inputSession = 0L
        monitorCursor = false
    }
    fun acceptsInput(value: GpuiInputConnection, surface: Long, session: Long): Boolean {
        NativeBridge.checkMain()
        return connection === value && attached && resumed && generation == surface &&
            session != 0L && NativeBridge.inputSession(surface) == session
    }
    fun syncInputSession() {
        NativeBridge.checkMain()
        val session = if (attached && resumed) NativeBridge.inputSession(generation) else 0L
        if (inputSession == session) return
        closeInputConnection()
        inputSession = session
        ime.restartInput(this)
    }
    fun showKeyboard() {
        if (attached && resumed) NativeBridge.frame(generation)
        if (!onCheckIsTextEditor()) return
        requestFocus()
        syncInputSession()
        ime.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
    }
    fun hideKeyboard() { ime.hideSoftInputFromWindow(windowToken, 0) }
    fun updateSelection() {
        if (!attached) return
        syncInputSession()
        if (connection?.isBatchEditing == true) return
        val state = NativeBridge.textState(generation)
        ime.updateSelection(this, state[0], state[1], state[2], state[3])
        if (monitorCursor) updateCursorAnchor()
    }
    fun requestCursorUpdates(mode: Int): Boolean {
        if (mode and 3.inv() != 0) return false
        monitorCursor = mode and 2 != 0
        if (mode and 1 != 0) {
            NativeBridge.frame(generation)
            updateCursorAnchor()
        }
        return true
    }
    private fun updateCursorAnchor() {
        val bounds = NativeBridge.caretBounds(generation, inputSession) ?: return
        val state = NativeBridge.textState(generation)
        val location = IntArray(2)
        getLocationOnScreen(location)
        val matrix = Matrix().apply { setTranslate(location[0].toFloat(), location[1].toFloat()) }
        val builder = CursorAnchorInfo.Builder().setMatrix(matrix).setSelectionRange(state[0], state[1])
            .setInsertionMarkerLocation(bounds[0], bounds[1], bounds[1] + bounds[3], bounds[1] + bounds[3],
                CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION)
        val text = NativeBridge.text(generation)
        if (state[2] >= 0 && state[3] in state[2]..text.length) builder.setComposingText(state[2], text.substring(state[2], state[3]))
        ime.updateCursorAnchorInfo(this, builder.build())
    }
    override fun onKeyDown(code: Int, event: KeyEvent): Boolean {
        if (!attached) return super.onKeyDown(code, event)
        val key = hardwareKey(code, event)
        if (key != null) return NativeBridge.key(generation, key, modifiers(event), true, event.repeatCount > 0)
        syncInputSession()
        val connection = connection ?: (onCreateInputConnection(EditorInfo()) as? GpuiInputConnection)
            ?: return super.onKeyDown(code, event)
        if (code == KeyEvent.KEYCODE_DEL) {
            val state = NativeBridge.textState(generation)
            if (state[0] != state[1]) connection.commitText("", 1)
            else connection.deleteSurroundingTextInCodePoints(1, 0)
            return true
        }
        val character = event.unicodeChar
        if (character > 0 && !event.isCtrlPressed) {
            return connection.commitText(String(Character.toChars(character)), 1)
        }
        return super.onKeyDown(code, event)
    }
    override fun onKeyUp(code: Int, event: KeyEvent): Boolean {
        val key = hardwareKey(code, event)
        if (attached && key != null) return NativeBridge.key(generation, key, modifiers(event), false, false)
        return super.onKeyUp(code, event)
    }
    private fun modifiers(event: KeyEvent): Int =
        (if (event.isShiftPressed) 1 else 0) or (if (event.isCtrlPressed) 2 else 0) or
        (if (event.isAltPressed) 4 else 0) or (if (event.isMetaPressed) 8 else 0)
    private fun hardwareKey(code: Int, event: KeyEvent): String? = when (code) {
        KeyEvent.KEYCODE_TAB -> "tab"
        KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> "enter"
        KeyEvent.KEYCODE_SPACE -> if (!onCheckIsTextEditor()) "space" else null
        KeyEvent.KEYCODE_ESCAPE -> "escape"
        KeyEvent.KEYCODE_DPAD_LEFT -> "left"
        KeyEvent.KEYCODE_DPAD_RIGHT -> "right"
        KeyEvent.KEYCODE_DPAD_UP -> "up"
        KeyEvent.KEYCODE_DPAD_DOWN -> "down"
        KeyEvent.KEYCODE_MOVE_HOME -> "home"
        KeyEvent.KEYCODE_MOVE_END -> "end"
        KeyEvent.KEYCODE_FORWARD_DEL -> "delete"
        else -> if ((event.isCtrlPressed || event.isMetaPressed) && code in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z)
            ('a'.code + code - KeyEvent.KEYCODE_A).toChar().toString() else null
    }
}
