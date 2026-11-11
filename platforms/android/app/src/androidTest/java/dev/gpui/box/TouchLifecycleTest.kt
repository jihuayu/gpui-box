package dev.gpui.box

import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/** Exercises Kotlin MotionEvent ownership before the existing GPUI recognizer. */
@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class TouchLifecycleTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)

    private fun withView(action: (GpuiView) -> Unit) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        instrumentation.runOnMainSync {
            val root = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            action((root.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView)
        }
    }

    private fun count(view: GpuiView): Int {
        // The platform counter deliberately owns one committed-back handler per
        // positive count. Consume it to observe actual application callbacks.
        var count = 0
        while (NativeBridge.back(view.generation)) {
            NativeBridge.frame(view.generation)
            count += 1
            assertTrue("Counter callback must remain bounded", count < 100)
        }
        return count
    }

    private fun reset(view: GpuiView) {
        while (NativeBridge.back(view.generation)) NativeBridge.frame(view.generation)
        assertEquals(0, count(view))
    }

    private fun event(view: GpuiView, action: Int, ids: IntArray = intArrayOf(0), y: Float = 220f): Boolean {
        val properties = ids.map { id -> MotionEvent.PointerProperties().also {
            it.id = id
            it.toolType = MotionEvent.TOOL_TYPE_FINGER
        } }.toTypedArray()
        val coordinates = ids.map { MotionEvent.PointerCoords().also {
            it.x = 100f * view.resources.displayMetrics.density
            it.y = y * view.resources.displayMetrics.density
            it.pressure = 1f
            it.size = 1f
        } }.toTypedArray()
        val now = SystemClock.uptimeMillis()
        val event = MotionEvent.obtain(now, now, action, ids.size, properties, coordinates,
            0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
        return try { view.onTouchEvent(event) } finally { event.recycle() }
    }

    @Test fun cancelledAndPannedContactsCannotBecomeCounterTaps() = withView { view ->
        reset(view)
        repeat(20) { index ->
            assertTrue(event(view, MotionEvent.ACTION_DOWN, intArrayOf(index % 7)))
            assertTrue(event(view, MotionEvent.ACTION_CANCEL, intArrayOf(index % 7)))
            event(view, MotionEvent.ACTION_UP, intArrayOf(index % 7))
        }
        assertEquals(0, count(view))
        event(view, MotionEvent.ACTION_DOWN)
        event(view, MotionEvent.ACTION_MOVE, y = 160f)
        event(view, MotionEvent.ACTION_MOVE, y = 120f)
        event(view, MotionEvent.ACTION_CANCEL, y = 120f)
        event(view, MotionEvent.ACTION_UP, y = 120f)
        assertEquals("A pan must not activate its original button", 0, count(view))
        event(view, MotionEvent.ACTION_DOWN)
        event(view, MotionEvent.ACTION_UP)
        assertEquals(1, count(view))
    }

    @Test fun replacementDownAndReorderedPointerIndicesRetainContactIdentity() = withView { view ->
        reset(view)
        event(view, MotionEvent.ACTION_DOWN, intArrayOf(4))
        // Missing UP is repaired at a replacement ACTION_DOWN with reused ID.
        event(view, MotionEvent.ACTION_DOWN, intArrayOf(4))
        event(view, MotionEvent.ACTION_UP, intArrayOf(4))
        assertEquals(1, count(view))
        event(view, MotionEvent.ACTION_DOWN, intArrayOf(7))
        event(view, MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), intArrayOf(7, 3))
        event(view, MotionEvent.ACTION_MOVE, intArrayOf(3, 7))
        // The secondary pointer is index zero after Android reordered indices.
        event(view, MotionEvent.ACTION_POINTER_UP, intArrayOf(3, 7))
        assertEquals("Secondary UP must not complete the primary tap", 0, count(view))
        event(view, MotionEvent.ACTION_UP, intArrayOf(7))
        assertEquals(1, count(view))
        // A duplicated pointer-down must release its former native contact.
        event(view, MotionEvent.ACTION_DOWN, intArrayOf(5))
        event(view, MotionEvent.ACTION_POINTER_DOWN, intArrayOf(5))
        event(view, MotionEvent.ACTION_UP, intArrayOf(5))
        assertEquals(1, count(view))
        event(view, MotionEvent.ACTION_DOWN, intArrayOf(7))
        event(view, MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), intArrayOf(7, 3))
        event(view, MotionEvent.ACTION_POINTER_UP, intArrayOf(7, 3))
        assertEquals(1, count(view))
        event(view, MotionEvent.ACTION_UP, intArrayOf(3))
        assertEquals("Ignored secondary contact cannot gain the primary tap", 0, count(view))
    }

    @Test fun pausedHostRejectsLateTouchesAndResumesWithFreshContacts() = withView { view ->
        reset(view)
        event(view, MotionEvent.ACTION_DOWN, intArrayOf(2))
        view.setResumed(false)
        assertFalse(event(view, MotionEvent.ACTION_UP, intArrayOf(2)))
        assertFalse(event(view, MotionEvent.ACTION_DOWN, intArrayOf(2)))
        val lateContact = NativeBridge.nextContact()
        val density = view.resources.displayMetrics.density
        NativeBridge.touch(view.generation, lateContact, 0, 100f * density, 220f * density)
        NativeBridge.touch(view.generation, lateContact, 2, 100f * density, 220f * density)
        view.setResumed(true)
        event(view, MotionEvent.ACTION_UP, intArrayOf(2))
        assertEquals("Pause must cancel its old contact", 0, count(view))
        repeat(10) {
            event(view, MotionEvent.ACTION_DOWN, intArrayOf(2))
            event(view, MotionEvent.ACTION_UP, intArrayOf(2))
        }
        assertEquals(10, count(view))
    }
}
