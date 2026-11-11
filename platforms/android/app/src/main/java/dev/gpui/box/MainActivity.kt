package dev.gpui.box

import android.app.Activity
import android.os.Bundle
import android.content.ComponentCallbacks2
import android.util.Log
import android.view.WindowInsets
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import android.widget.FrameLayout

class MainActivity : Activity() {
    private lateinit var gpui: GpuiView
    private var backRegistered = false
    private val backCallback = OnBackInvokedCallback {
        if (gpui.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true) {
            Log.i("gpui-box", "System back dismissed the IME")
            gpui.hideKeyboard()
        } else {
            val handled = NativeBridge.back(gpui.generation)
            Log.i("gpui-box", "System back handled=$handled generation=${gpui.generation}")
            if (!handled) finish()
        }
    }
    fun updateBackRegistration() {
        val enabled = NativeBridge.backEnabled(gpui.generation)
        if (enabled == backRegistered) return
        if (enabled) onBackInvokedDispatcher.registerOnBackInvokedCallback(
            OnBackInvokedDispatcher.PRIORITY_DEFAULT, backCallback)
        else onBackInvokedDispatcher.unregisterOnBackInvokedCallback(backCallback)
        backRegistered = enabled
    }
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        gpui = GpuiView(this)
        // Keep the first slice's drawable viewport clear of system UI and IME.
        // Full edge-to-edge GPUI WindowInsets animation is a later platform task.
        val content = FrameLayout(this)
        content.addView(gpui, FrameLayout.LayoutParams(-1, -1))
        content.setOnApplyWindowInsetsListener { view, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            val keyboard = insets.getInsets(WindowInsets.Type.ime())
            view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
            insets
        }
        setContentView(content)
        NativeBridge.bind(gpui)
    }
    override fun onResume() { super.onResume(); gpui.setResumed(true); updateBackRegistration() }
    override fun onPause() { gpui.setResumed(false); super.onPause() }
    override fun onDestroy() {
        if (backRegistered) onBackInvokedDispatcher.unregisterOnBackInvokedCallback(backCallback)
        NativeBridge.unbind(gpui)
        super.onDestroy()
    }
    @Suppress("DEPRECATION")
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        // API 35 retains UI_HIDDEN and BACKGROUND notifications.
        if (level >= ComponentCallbacks2.TRIM_MEMORY_UI_HIDDEN) NativeBridge.memoryWarning()
    }
    override fun onLowMemory() { super.onLowMemory(); NativeBridge.memoryWarning() }
}
