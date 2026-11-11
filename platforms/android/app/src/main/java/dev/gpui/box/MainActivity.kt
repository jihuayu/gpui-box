package dev.gpui.box

import android.app.Activity
import android.os.Bundle
import android.os.Build
import android.view.WindowInsets
import android.widget.FrameLayout

class MainActivity : Activity() {
    private lateinit var gpui: GpuiView
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        gpui = GpuiView(this)
        // Keep the first slice's drawable viewport clear of system UI and IME.
        // Full edge-to-edge GPUI WindowInsets animation is a later platform task.
        val content = FrameLayout(this)
        content.addView(gpui, FrameLayout.LayoutParams(-1, -1))
        content.setOnApplyWindowInsetsListener { view, insets ->
            if (Build.VERSION.SDK_INT >= 30) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                val keyboard = insets.getInsets(WindowInsets.Type.ime())
                view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
            } else {
                @Suppress("DEPRECATION")
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            }
            insets
        }
        setContentView(content)
        NativeBridge.bind(gpui)
    }
    override fun onResume() { super.onResume(); gpui.setResumed(true) }
    override fun onPause() { gpui.setResumed(false); super.onPause() }
    override fun onDestroy() { NativeBridge.unbind(gpui); super.onDestroy() }
}
