package dev.gpui.box

import android.app.KeyguardManager
import android.app.UiAutomation
import android.os.ParcelFileDescriptor
import android.os.PowerManager
import android.os.SystemClock
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/** Real OS screen transitions, using the emulator's existing lock policy. */
@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class ScreenLifecycleTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()

    private fun shell(command: String) {
        val automation = instrumentation.getUiAutomation(UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
        ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command)).use {
            it.readBytes()
        }
    }

    private fun currentView(): GpuiView? {
        val activity = ActivityLifecycleMonitorRegistry.getInstance()
            .getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()
            ?: return null
        val root = activity.findViewById<FrameLayout>(android.R.id.content)
        return (root.getChildAt(0) as? FrameLayout)?.getChildAt(0) as? GpuiView
    }

    private fun awaitMain(description: String, predicate: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000
        var ready = false
        do {
            instrumentation.runOnMainSync { ready = predicate() }
            if (!ready) SystemClock.sleep(50)
        } while (!ready && SystemClock.uptimeMillis() < deadline)
        assertTrue(description, ready)
    }

    private fun awaitForeground() = awaitMain("The app must resume with an eligible focused surface") {
        val view = currentView()
        val snapshot = JSONObject(NativeBridge.hostSnapshot())
        view != null && view.generation != 0L && view.hasWindowFocus() &&
            snapshot.optBoolean("focused") && snapshot.optString("phase").startsWith("Foreground")
    }

    private fun tapEditor(view: GpuiView) {
        val density = view.resources.displayMetrics.density
        val contact = NativeBridge.nextContact()
        NativeBridge.touch(view.generation, contact, 0, 100f * density, 300f * density)
        NativeBridge.touch(view.generation, contact, 2, 100f * density, 300f * density)
        NativeBridge.frame(view.generation)
    }

    @Test fun screenSleepWakeRejectsOldEditorsAndRetainsTextAcrossRepeatedCycles() {
        val activity = activityRule.activity
        val power = activity.getSystemService(PowerManager::class.java)
        val keyguard = activity.getSystemService(KeyguardManager::class.java)
        awaitForeground()
        var expected = "Screen lifecycle 你好🙂"
        instrumentation.runOnMainSync {
            val view = currentView()!!
            tapEditor(view)
            val editor = view.onCreateInputConnection(EditorInfo())!!
            assertTrue(editor.performContextMenuAction(android.R.id.selectAll))
            assertTrue(editor.commitText(expected, 1))
            view.hideKeyboard()
        }
        try {
            repeat(3) { cycle ->
                lateinit var oldEditor: GpuiInputConnection
                var oldGeneration = 0L
                instrumentation.runOnMainSync {
                    val view = currentView()!!
                    oldGeneration = view.generation
                    oldEditor = view.onCreateInputConnection(EditorInfo()) as GpuiInputConnection
                    assertEquals(expected, NativeBridge.text(view.generation))
                }
                shell("input keyevent KEYCODE_SLEEP")
                awaitMain("Screen-off must close input and leave foreground state") {
                    val state = JSONObject(NativeBridge.hostSnapshot())
                    !power.isInteractive && currentView() == null &&
                        !state.optBoolean("focused") && state.optString("phase") == "Background" &&
                        NativeBridge.inputSession(oldGeneration) == 0L
                }
                var backgroundMillis = 0L
                instrumentation.runOnMainSync {
                    val state = JSONObject(NativeBridge.hostSnapshot())
                    assertEquals(keyguard.isDeviceLocked, state.getBoolean("locked"))
                    assertFalse(oldEditor.commitText("stale-off", 1))
                    assertNull(oldEditor.getTextBeforeCursor(100, 0))
                    backgroundMillis = state.getLong("activeMillis")
                }
                SystemClock.sleep(250)
                instrumentation.runOnMainSync {
                    assertEquals("Screen-off time must not accrue active use", backgroundMillis,
                        JSONObject(NativeBridge.hostSnapshot()).getLong("activeMillis"))
                }
                shell("input keyevent KEYCODE_WAKEUP")
                shell("wm dismiss-keyguard")
                awaitForeground()
                instrumentation.runOnMainSync {
                    val view = currentView()!!
                    assertFalse(oldEditor.commitText("stale-resumed", 1))
                    tapEditor(view)
                    assertEquals(expected, NativeBridge.text(view.generation))
                    val editor = view.onCreateInputConnection(EditorInfo())!!
                    assertTrue(editor.setSelection(expected.length, expected.length))
                    assertTrue(editor.commitText("+$cycle", 1))
                    expected += "+$cycle"
                    assertEquals(expected, NativeBridge.text(view.generation))
                    assertEquals(keyguard.isDeviceLocked,
                        JSONObject(NativeBridge.hostSnapshot()).getBoolean("locked"))
                    view.hideKeyboard()
                }
            }
        } finally {
            shell("input keyevent KEYCODE_WAKEUP")
            shell("wm dismiss-keyguard")
            instrumentation.getUiAutomation(UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
        }
    }
}
