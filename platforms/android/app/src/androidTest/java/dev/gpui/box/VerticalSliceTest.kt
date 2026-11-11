package dev.gpui.box

import androidx.test.rule.ActivityTestRule
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout

/** Runs against the real GPUI field after a touch focuses it. */
@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class VerticalSliceTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)
    @Test
    fun testRustAuthoritativeCompositionAndUtf16() {
        val activity = activityRule.activity
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        instrumentation.runOnMainSync {
            val root = activity.findViewById<FrameLayout>(android.R.id.content)
            val container = root.getChildAt(0) as FrameLayout
            val view = container.getChildAt(0) as GpuiView
            val generation = view.generation
            // GPUI click recognition consumes raw touch and registers the field.
            val density = activity.resources.displayMetrics.density
            NativeBridge.touch(generation, Long.MAX_VALUE - 1, 0, 100f * density, 300f * density)
            NativeBridge.touch(generation, Long.MAX_VALUE - 1, 2, 100f * density, 300f * density)
            NativeBridge.frame(generation)
            val connection = view.onCreateInputConnection(EditorInfo())!!
            val length = NativeBridge.text(generation).length
            assertTrue(connection.setSelection(0, length))
            assertTrue(connection.commitText("", 1))
            assertEquals("", NativeBridge.text(generation))
            assertTrue(connection.setComposingText("ni", 1))
            assertEquals("ni", NativeBridge.text(generation))
            assertEquals(listOf(2, 2, 0, 2), NativeBridge.textState(generation).toList())
            assertTrue(connection.setComposingText("你好", 1))
            assertEquals("你好", NativeBridge.text(generation))
            assertTrue(connection.commitText("你好🙂", 1))
            assertEquals("你好🙂", NativeBridge.text(generation))
            assertEquals(listOf(4, 4, -1, -1), NativeBridge.textState(generation).toList())
            assertEquals("你好🙂", connection.getTextBeforeCursor(99, 0).toString())
            assertTrue(connection.setSelection(2, 4))
            assertEquals("🙂", connection.getSelectedText(0).toString())
            assertTrue(connection.commitText("！", 1))
            assertEquals("你好！", NativeBridge.text(generation))
            assertTrue(connection.finishComposingText())
            assertTrue(connection.commitText("🙂", 1))
            assertTrue(connection.deleteSurroundingTextInCodePoints(1, 0))
            assertEquals("你好！", NativeBridge.text(generation))
            // A stale surface generation cannot change the current field.
            NativeBridge.edit(generation - 1, 0, "stale", 1, 0)
            assertEquals("你好！", NativeBridge.text(generation))
            NativeBridge.frame(generation)
        }
    }
}
