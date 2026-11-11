package dev.gpui.box

import android.os.Handler
import android.os.Looper
import android.view.Surface
import java.lang.ref.WeakReference
import java.util.concurrent.atomic.AtomicLong

/** Stable, process-owned bridge. Requests always unwind Rust before host work. */
object NativeBridge {
    private val handler = Handler(Looper.getMainLooper())
    private var view = WeakReference<GpuiView>(null)
    private val generations = AtomicLong()
    private val contacts = AtomicLong()
    init { System.loadLibrary("gpui_box_android"); initialize() }

    fun bind(value: GpuiView) { checkMain(); view = WeakReference(value) }
    fun unbind(value: GpuiView) { checkMain(); if (view.get() === value) view.clear() }
    fun nextGeneration(): Long = generations.incrementAndGet()
    fun nextContact(): Long = contacts.incrementAndGet()
    fun checkMain() { check(Looper.myLooper() === Looper.getMainLooper()) }

    @JvmStatic fun request(command: Int) {
        handler.post {
            val current = view.get() ?: return@post
            when (command) {
                0 -> current.scheduleFrame()
                1 -> current.showKeyboard()
                2 -> current.hideKeyboard()
                3 -> (current.context as? MainActivity)?.finish()
                4 -> current.updateSelection()
            }
        }
    }
    @JvmStatic external fun initialize()
    @JvmStatic external fun attach(surface: Surface, generation: Long, width: Int, height: Int, density: Float)
    @JvmStatic external fun detach(generation: Long)
    @JvmStatic external fun resize(generation: Long, width: Int, height: Int, density: Float)
    @JvmStatic external fun frame(generation: Long)
    @JvmStatic external fun lifecycle(generation: Long, resumed: Boolean)
    @JvmStatic external fun touch(generation: Long, id: Long, phase: Int, x: Float, y: Float)
    @JvmStatic external fun text(generation: Long): String
    @JvmStatic external fun textState(generation: Long): IntArray
    @JvmStatic external fun edit(generation: Long, operation: Int, text: String, a: Int, b: Int)
}
