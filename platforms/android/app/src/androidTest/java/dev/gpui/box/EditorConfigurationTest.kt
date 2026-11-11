package dev.gpui.box

import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class EditorConfigurationTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)
    @Test fun editorConfigurationAndThreadGuardAreEnforced() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        var generation = 0L
        instrumentation.runOnMainSync {
            val root = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            val view = ((root.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView)
            generation = view.generation
            val density = view.resources.displayMetrics.density
            val contact = NativeBridge.nextContact()
            NativeBridge.touch(generation, contact, 0, 100f * density, 300f * density)
            NativeBridge.touch(generation, contact, 2, 100f * density, 300f * density)
            NativeBridge.frame(generation)
            val info = EditorInfo()
            val connection = view.onCreateInputConnection(info)!!
            assertEquals(EditorInfo.IME_ACTION_DONE, info.imeOptions and EditorInfo.IME_MASK_ACTION)
            val caret = NativeBridge.caretBounds(generation, NativeBridge.inputSession(generation))!!
            assertTrue(caret.all { it.isFinite() })
            assertTrue(caret[3] > 0f)
            assertTrue(connection.requestCursorUpdates(3))
            assertFalse(connection.requestCursorUpdates(0x4000))
            assertTrue(connection.performEditorAction(EditorInfo.IME_ACTION_DONE))
        }
        try {
            NativeBridge.inputSession(generation)
            fail("Off-main native calls must be rejected")
        } catch (_: IllegalStateException) { /* main Looper guard */ }
    }
}
