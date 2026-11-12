package dev.gpui.box

import android.content.ComponentCallbacks2
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.view.KeyEvent
import android.view.WindowInsets
import android.view.WindowInsetsAnimation
import android.view.Choreographer
import android.view.WindowManager
import android.os.SystemClock
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class PlatformFoundationTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java, false, false)

    @Before fun launchWithoutAccessibilityFocusChanges() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.getUiAutomation(android.app.UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
        activityRule.launchActivity(null)
        instrumentation.waitForIdleSync()
    }

    private fun withView(action: (GpuiView) -> Unit) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        instrumentation.runOnMainSync {
            val root = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            val container = root.getChildAt(0) as FrameLayout
            action(container.getChildAt(0) as GpuiView)
        }
    }
    private fun tap(view: GpuiView, y: Float) {
        val density = view.resources.displayMetrics.density
        val contact = NativeBridge.nextContact()
        NativeBridge.touch(view.generation, contact, 0, 100f * density, y * density)
        NativeBridge.touch(view.generation, contact, 2, 100f * density, y * density)
        NativeBridge.frame(view.generation)
    }
    private fun clear(view: GpuiView): GpuiInputConnection {
        val connection = view.onCreateInputConnection(EditorInfo()) as GpuiInputConnection
        assertTrue(connection.finishComposingText())
        assertTrue(connection.performContextMenuAction(android.R.id.selectAll))
        assertTrue(connection.commitText("", 1))
        return connection
    }

    @Test fun clipboardAndNestedBatchEditsUseRustText() = withView { view ->
        tap(view, 300f)
        val connection = clear(view)
        assertTrue(connection.beginBatchEdit())
        assertTrue(connection.beginBatchEdit())
        assertTrue(connection.setComposingText("你好🙂", 1))
        assertTrue(connection.finishComposingText())
        assertTrue(connection.endBatchEdit())
        assertFalse(connection.endBatchEdit())
        assertFalse(connection.endBatchEdit())
        assertTrue(connection.performContextMenuAction(android.R.id.selectAll))
        assertTrue(connection.performContextMenuAction(android.R.id.copy))
        assertEquals("你好🙂", NativeBridge.readClipboard())
        assertTrue(connection.performContextMenuAction(android.R.id.cut))
        assertEquals("", NativeBridge.text(view.generation))
        assertTrue(connection.performContextMenuAction(android.R.id.paste))
        assertEquals("你好🙂", NativeBridge.text(view.generation))
        assertTrue(connection.setComposingRegion(0, 2))
        assertTrue(NativeBridge.textState(view.generation)[2] >= 0)
        val replacement = view.onCreateInputConnection(EditorInfo())!!
        assertEquals(-1, NativeBridge.textState(view.generation)[2])
        assertFalse(connection.commitText("closed", 1))
        assertTrue(replacement.finishComposingText())
        assertFalse(connection.deleteSurroundingText(-1, 0))
        assertFalse(connection.performContextMenuAction(-1))
    }

    @Test fun oldConnectionsCannotCrossFieldsOrResume() = withView { view ->
        tap(view, 300f)
        val first = clear(view)
        assertTrue(first.commitText("first", 1))
        val firstSession = NativeBridge.inputSession(view.generation)
        tap(view, 390f)
        assertFalse(first.commitText("stale", 1))
        assertNull(first.getTextBeforeCursor(10, 0))
        assertFalse(NativeBridge.edit(view.generation, firstSession, 0, "stale", 1, 0))
        val second = clear(view)
        assertTrue(second.commitText("second", 1))
        assertEquals("second", NativeBridge.text(view.generation))
        NativeBridge.frame(view.generation)
        // Repainting the same focused field must preserve its session.
        assertTrue(second.commitText("!", 1))
        val replacement = view.onCreateInputConnection(EditorInfo())!!
        assertFalse(second.commitText("closed", 1))
        assertTrue(replacement.commitText("?", 1))
        view.setResumed(false)
        assertFalse(replacement.commitText("paused", 1))
        assertEquals(0L, NativeBridge.inputSession(view.generation))
        view.setResumed(true)
        assertFalse(replacement.commitText("resumed-stale", 1))
        val resumed = view.onCreateInputConnection(EditorInfo())!!
        assertTrue(resumed.commitText("+", 1))
        assertEquals("second!?+", NativeBridge.text(view.generation))
        tap(view, 300f)
        assertEquals("first", NativeBridge.text(view.generation))
        tap(view, 220f)
        assertEquals(0L, NativeBridge.inputSession(view.generation))
        first.closeConnection()
        assertFalse(first.setSelection(0, 0))
    }

    @Test fun backDispatchAndMemoryNotificationsKeepRuntimeUsable() = withView { view ->
        // Drain any state retained by a previous Activity in this process.
        while (NativeBridge.back(view.generation)) NativeBridge.frame(view.generation)
        tap(view, 220f)
        val density = view.resources.displayMetrics.density
        assertTrue(NativeBridge.backGesture(view.generation, 0, 2f * density, 220f * density, 0f))
        assertTrue(NativeBridge.backGesture(view.generation, 1, 0f, 0f, 0.5f))
        assertTrue(NativeBridge.backGesture(view.generation, 3, 0f, 0f, 0f))
        // Cancellation leaves the counter and its claim intact.
        assertTrue(NativeBridge.backGesture(view.generation, 0, 2f * density, 220f * density, 0f))
        assertFalse(NativeBridge.backGesture(view.generation - 1, 2, 0f, 0f, 1f))
        assertTrue(NativeBridge.backGesture(view.generation, 2, 0f, 0f, 1f))
        NativeBridge.frame(view.generation)
        assertFalse(NativeBridge.backGesture(view.generation, 2, 0f, 0f, 1f))
        assertFalse(NativeBridge.back(view.generation))
        tap(view, 220f)
        assertTrue(NativeBridge.back(view.generation))
        NativeBridge.frame(view.generation)
        assertFalse(NativeBridge.back(view.generation))
        tap(view, 300f)
        val connection = clear(view)
        assertTrue(connection.commitText("before", 1))
        activityRule.activity.onTrimMemory(ComponentCallbacks2.TRIM_MEMORY_BACKGROUND)
        activityRule.activity.onLowMemory()
        assertTrue(connection.commitText(" after", 1))
        assertEquals("before after", NativeBridge.text(view.generation))
    }

    @Test fun recreationRetainsTextAndRejectsOldConnections() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        lateinit var oldConnection: GpuiInputConnection
        var oldGeneration = 0L
        withView { view ->
            tap(view, 300f)
            oldGeneration = view.generation
            oldConnection = clear(view)
            assertTrue(oldConnection.commitText("retained", 1))
            activityRule.activity.recreate()
        }
        var replacement: GpuiView? = null
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (replacement == null && SystemClock.uptimeMillis() < deadline) {
            instrumentation.waitForIdleSync()
            instrumentation.runOnMainSync {
                val activity = ActivityLifecycleMonitorRegistry.getInstance()
                    .getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()
                val root = activity?.findViewById<FrameLayout>(android.R.id.content)
                val container = root?.getChildAt(0) as? FrameLayout
                val view = container?.getChildAt(0) as? GpuiView
                if (view != null && view.generation > oldGeneration &&
                    NativeBridge.inputSession(view.generation) != 0L) replacement = view
            }
            if (replacement == null) SystemClock.sleep(50)
        }
        assertNotNull("A recreated Activity must attach a new eligible surface", replacement)
        instrumentation.runOnMainSync {
            val view = replacement!!
            assertFalse(oldConnection.commitText("stale", 1))
            assertEquals("retained", NativeBridge.text(view.generation))
            assertTrue(view.onCreateInputConnection(EditorInfo())!!.commitText("!", 1))
            assertEquals("retained!", NativeBridge.text(view.generation))
        }
    }

    @Test fun systemBackReachesTheGpuiListener() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var imeAnimationRunning = false
        withView { view ->
            // This case verifies navigation dispatch independently of the
            // IME's own higher-priority back callback and asynchronous startup.
            activityRule.activity.window.addFlags(WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM)
            while (NativeBridge.back(view.generation)) NativeBridge.frame(view.generation)
            view.setWindowInsetsAnimationCallback(object : WindowInsetsAnimation.Callback(DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
                override fun onPrepare(animation: WindowInsetsAnimation) {
                    if (animation.typeMask and WindowInsets.Type.ime() != 0) imeAnimationRunning = true
                }
                override fun onProgress(insets: WindowInsets, animations: MutableList<WindowInsetsAnimation>) = insets
                override fun onEnd(animation: WindowInsetsAnimation) {
                    if (animation.typeMask and WindowInsets.Type.ime() != 0) imeAnimationRunning = false
                }
            })
            tap(view, 220f)
            view.hideKeyboard()
            val container = view.parent as FrameLayout
            container.isFocusableInTouchMode = true
            container.requestFocus()
            view.context.getSystemService(InputMethodManager::class.java).restartInput(container)
        }
        // InputMethodManager dismissal completes asynchronously. Back during
        // that transition correctly belongs to the keyboard instead of GPUI.
        var keyboardVisible = true
        var hiddenSince = 0L
        val deadline = SystemClock.uptimeMillis() + 15_000
        while (SystemClock.uptimeMillis() < deadline) {
            instrumentation.waitForIdleSync()
            withView { view ->
                val ime = view.context.getSystemService(InputMethodManager::class.java)
                keyboardVisible = imeAnimationRunning || view.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true || ime.isActive(view)
            }
            if (keyboardVisible) hiddenSince = 0L
            else if (hiddenSince == 0L) hiddenSince = SystemClock.uptimeMillis()
            if (hiddenSince != 0L && SystemClock.uptimeMillis() - hiddenSince >= 500) break
            SystemClock.sleep(50)
        }
        assertFalse("The IME must finish hiding before testing navigation back", keyboardVisible)
        assertTrue("The hidden IME must remain stable before navigation back", hiddenSince != 0L && SystemClock.uptimeMillis() - hiddenSince >= 500)
        instrumentation.waitForIdleSync()
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        // A native back callback updates Rust state, then GPUI removes the old
        // listener during its next paced frame. Looper idle alone does not wait
        // for Choreographer or the window-manager callback round trip.
        val frames = CountDownLatch(1)
        instrumentation.runOnMainSync {
            var remaining = 3
            val callback = object : Choreographer.FrameCallback {
                override fun doFrame(time: Long) {
                    remaining -= 1
                    if (remaining == 0) frames.countDown()
                    else Choreographer.getInstance().postFrameCallback(this)
                }
            }
            Choreographer.getInstance().postFrameCallback(callback)
        }
        assertTrue("GPUI must receive paced frames after back dispatch", frames.await(5, TimeUnit.SECONDS))
        instrumentation.waitForIdleSync()
        withView { view ->
            assertFalse(activityRule.activity.isFinishing)
            NativeBridge.frame(view.generation)
            assertFalse("The system back action must consume the positive counter", NativeBridge.back(view.generation))
        }
    }
}
