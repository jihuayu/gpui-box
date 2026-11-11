package dev.gpui.box

import android.app.UiAutomation
import android.os.SystemClock
import android.view.KeyEvent
import android.view.WindowInsets
import android.view.WindowInsetsAnimation
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/** Actual IME and OS back dispatch share the same focused GPUI editor. */
@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class ImeBackTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()

    private fun view(): GpuiView {
        val root = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
        return (root.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView
    }

    private fun awaitMain(description: String, predicate: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000
        var ready = false
        var stable = 0
        do {
            ready = false
            instrumentation.runOnMainSync { ready = predicate() }
            stable = if (ready) stable + 1 else 0
            if (stable < 3) SystemClock.sleep(50)
        } while (stable < 3 && SystemClock.uptimeMillis() < deadline)
        assertTrue(description, ready && stable == 3)
    }

    private fun tap(view: GpuiView, y: Float) {
        val density = view.resources.displayMetrics.density
        val contact = NativeBridge.nextContact()
        NativeBridge.touch(view.generation, contact, 0, 100f * density, y * density)
        NativeBridge.touch(view.generation, contact, 2, 100f * density, y * density)
        NativeBridge.frame(view.generation)
    }

    private fun navigationClaim(view: GpuiView): Boolean {
        // Host backEnabled indicates an eligible visible surface. Observe the
        // application's listener using a cancelled gesture without committing it.
        NativeBridge.frame(view.generation)
        val density = view.resources.displayMetrics.density
        val claimed = NativeBridge.backGesture(view.generation, 0, 2f * density, 220f * density, 0f)
        NativeBridge.backGesture(view.generation, 3, 0f, 0f, 0f)
        return claimed
    }

    @Test fun firstBackHidesKeyboardBeforeLaterBackNavigates() {
        instrumentation.getUiAutomation(UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
        instrumentation.waitForIdleSync()
        val animations = mutableSetOf<WindowInsetsAnimation>()
        instrumentation.runOnMainSync {
            view().setWindowInsetsAnimationCallback(object : WindowInsetsAnimation.Callback(DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
                override fun onStart(animation: WindowInsetsAnimation, bounds: WindowInsetsAnimation.Bounds): WindowInsetsAnimation.Bounds {
                    if (animation.typeMask and WindowInsets.Type.ime() != 0) animations.add(animation)
                    return bounds
                }
                override fun onProgress(insets: WindowInsets, running: MutableList<WindowInsetsAnimation>): WindowInsets {
                    animations.retainAll(running.toSet())
                    return insets
                }
                override fun onEnd(animation: WindowInsetsAnimation) { animations.remove(animation) }
            })
        }
        try {
            repeat(2) { cycle ->
                val expected = "IME back $cycle 你好🙂"
                instrumentation.runOnMainSync {
                    val view = view()
                    while (NativeBridge.back(view.generation)) NativeBridge.frame(view.generation)
                    tap(view, 220f)
                    assertTrue("The counter must install its GPUI navigation claim", navigationClaim(view))
                }
                // A previous Activity may restore editor focus and auto-show
                // the IME. Settle the counter's blur/hide before requesting show.
                awaitMain("Counter focus transition must settle before editing") {
                    animations.isEmpty() && view().rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == false
                }
                instrumentation.runOnMainSync {
                    val view = view()
                    tap(view, 300f)
                    val editor = view.onCreateInputConnection(EditorInfo())!!
                    assertTrue(editor.finishComposingText())
                    assertTrue(editor.performContextMenuAction(android.R.id.selectAll))
                    assertTrue(editor.commitText(expected, 1))
                }
                awaitMain("The real software keyboard must finish appearing") {
                    animations.isEmpty() && view().rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true
                }
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                awaitMain("First system back must finish hiding the keyboard") {
                    animations.isEmpty() && view().rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == false
                }
                instrumentation.runOnMainSync {
                    val view = view()
                    assertFalse(activityRule.activity.isFinishing)
                    assertTrue("Keyboard dismissal must preserve the GPUI navigation claim", navigationClaim(view))
                    assertEquals(expected, NativeBridge.text(view.generation))
                }
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                awaitMain("Later system back must consume GPUI navigation") {
                    !navigationClaim(view())
                }
                instrumentation.runOnMainSync {
                    assertFalse(activityRule.activity.isFinishing)
                    assertEquals(expected, NativeBridge.text(view().generation))
                }
            }
        } finally {
            instrumentation.runOnMainSync {
                view().hideKeyboard()
                view().setWindowInsetsAnimationCallback(null)
            }
            instrumentation.getUiAutomation(UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
        }
    }
}
