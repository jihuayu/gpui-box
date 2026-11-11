package dev.gpui.box

import android.os.Handler
import android.os.Looper
import android.content.ClipData
import android.content.ClipboardManager
import android.view.Surface
import java.lang.ref.WeakReference
import java.util.concurrent.atomic.AtomicLong

/** Stable, process-owned bridge. Requests always unwind Rust before host work. */
object NativeBridge {
    private val handler = Handler(Looper.getMainLooper())
    private var view = WeakReference<GpuiView>(null)
    private val generations = AtomicLong()
    private val contacts = AtomicLong()
    private val attachments = AtomicLong()
    private val surfaces = AtomicLong()
    init { System.loadLibrary("gpui_box_android"); initialize() }

    fun bind(value: GpuiView) {
        checkMain()
        attachments.incrementAndGet()
        surfaces.set(0)
        view = WeakReference(value)
    }
    fun unbind(value: GpuiView) {
        checkMain()
        if (view.get() === value) {
            attachments.incrementAndGet()
            view.clear()
        }
    }
    fun surfaceAttached(value: GpuiView, generation: Long) {
        checkMain()
        if (view.get() === value) surfaces.set(generation)
    }
    fun surfaceDetached(generation: Long) {
        checkMain()
        surfaces.compareAndSet(generation, 0)
    }
    fun nextGeneration(): Long = generations.incrementAndGet()
    fun nextContact(): Long = contacts.incrementAndGet()
    fun checkMain() { check(Looper.myLooper() === Looper.getMainLooper()) }
    @JvmStatic fun openUrl(value: String): Boolean {
        checkMain()
        return (view.get()?.context as? MainActivity)?.openUrl(value) ?: false
    }

    @JvmStatic fun request(command: Int) {
        val attachment = attachments.get()
        val surface = surfaces.get()
        handler.post {
            val current = view.get() ?: return@post
            if (command != 0 && (attachments.get() != attachment || surfaces.get() != surface)) return@post
            when (command) {
                0 -> current.scheduleFrame()
                1 -> current.showKeyboard()
                2 -> current.hideKeyboard()
                3 -> (current.context as? MainActivity)?.finish()
                4 -> current.updateSelection()
                5 -> current.syncInputSession()
                7 -> current.updateAccessibility()
                6 -> (current.context as? MainActivity)?.updateBackRegistration()
            }
        }
    }
    @JvmStatic fun readClipboard(): String? {
        checkMain()
        val context = view.get()?.context ?: return null
        val clip = context.getSystemService(ClipboardManager::class.java).primaryClip ?: return null
        return if (clip.itemCount > 0) clip.getItemAt(0).coerceToText(context)?.toString() else null
    }
    @JvmStatic fun writeClipboard(text: String) {
        checkMain()
        view.get()?.context?.getSystemService(ClipboardManager::class.java)
            ?.setPrimaryClip(ClipData.newPlainText("gpui-box", text))
    }
    @JvmStatic fun checkMainThread() = checkMain()
    @JvmStatic external fun editorInfo(generation: Long): IntArray
    @JvmStatic external fun editorAction(generation: Long, session: Long): Boolean
    @JvmStatic external fun caretBounds(generation: Long, session: Long): FloatArray?
    @JvmStatic external fun accessibilitySnapshot(generation: Long, enabled: Boolean): String
    @JvmStatic external fun accessibilityAction(generation: Long, id: Long, action: Int, text: String): Boolean
    @JvmStatic external fun insets(generation: Long)
    @JvmStatic external fun initialize()
    @JvmStatic external fun attach(surface: Surface, generation: Long, width: Int, height: Int, density: Float)
    @JvmStatic external fun detach(generation: Long)
    @JvmStatic external fun resize(generation: Long, width: Int, height: Int, density: Float)
    @JvmStatic external fun frame(generation: Long)
    @JvmStatic external fun lifecycle(generation: Long, resumed: Boolean)
    @JvmStatic external fun hostState(generation: Long, resumed: Boolean, focused: Boolean,
        locked: Boolean, dark: Boolean, textScale: Float, locale: String, reducedMotion: Boolean)
    @JvmStatic external fun hostSnapshot(): String
    @JvmStatic external fun saveState(): ByteArray
    @JvmStatic external fun restoreState(state: ByteArray)
    @JvmStatic external fun touch(generation: Long, id: Long, phase: Int, x: Float, y: Float)
    @JvmStatic external fun text(generation: Long): String
    @JvmStatic external fun textState(generation: Long): IntArray
    @JvmStatic external fun inputSession(generation: Long): Long
    @JvmStatic external fun edit(generation: Long, session: Long, operation: Int, text: String, a: Int, b: Int): Boolean
    @JvmStatic external fun backEnabled(generation: Long): Boolean
    @JvmStatic external fun back(generation: Long): Boolean
    @JvmStatic external fun backGesture(generation: Long, phase: Int, x: Float, y: Float, progress: Float): Boolean
    @JvmStatic external fun key(generation: Long, key: String, modifiers: Int, down: Boolean, held: Boolean): Boolean
    @JvmStatic external fun memoryWarning()
    @JvmStatic external fun openUrlEvent(generation: Long, id: String, url: String): Boolean
    @JvmStatic external fun clipboardAction(generation: Long, session: Long, operation: Int): Boolean
}
