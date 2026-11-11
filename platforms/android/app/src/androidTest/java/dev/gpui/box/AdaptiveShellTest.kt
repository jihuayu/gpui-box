package dev.gpui.box

import android.view.KeyEvent
import android.app.UiAutomation
import android.content.pm.ActivityInfo
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class AdaptiveShellTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)

    private fun currentActivity(): MainActivity = ActivityLifecycleMonitorRegistry.getInstance()
        .getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()
        ?: activityRule.activity

    private fun view(activity: MainActivity): GpuiView {
        val root = activity.findViewById<FrameLayout>(android.R.id.content)
        return (root.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView
    }

    private fun nodes(view: GpuiView): List<JSONObject> {
        NativeBridge.accessibilitySnapshot(view.generation, true)
        NativeBridge.frame(view.generation)
        val array = JSONArray(NativeBridge.accessibilitySnapshot(view.generation, true))
        return (0 until array.length()).map(array::getJSONObject)
    }

    @After fun restorePlatformPageAndReleaseAccessibility() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.runOnMainSync {
            val view = view(currentActivity())
            repeat(4) {
                val array = JSONArray(NativeBridge.accessibilitySnapshot(view.generation, true))
                val nodes = (0 until array.length()).map(array::getJSONObject)
                val next = listOf("Close", "Back to overview", "Back to platform checks")
                    .firstNotNullOfOrNull { label -> nodes.firstOrNull { it.optString("label") == label } }
                if (next != null) NativeBridge.accessibilityAction(view.generation,
                    next.getString("id").toULong().toLong(), 0, "")
                NativeBridge.frame(view.generation)
            }
            NativeBridge.accessibilitySnapshot(view.generation, false)
        }
        instrumentation.getUiAutomation(UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
    }

    @Test fun rotationRetainsCatalogDraftSelectionAndAdaptiveDetail() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        fun action(view: GpuiView, label: String, operation: Int = 0, value: String = "") {
            val node = nodes(view).single { it.optString("label") == label }
            assertTrue(NativeBridge.accessibilityAction(view.generation,
                node.getString("id").toULong().toLong(), operation, value))
            NativeBridge.frame(view.generation)
        }
        fun rotate(orientation: Int, wide: Boolean) {
            var previous = 0L
            instrumentation.runOnMainSync {
                previous = view(currentActivity()).generation
                currentActivity().requestedOrientation = orientation
            }
            val deadline = android.os.SystemClock.uptimeMillis() + 10_000
            var ready = false
            do {
                instrumentation.waitForIdleSync()
                instrumentation.runOnMainSync {
                    val current = view(currentActivity())
                    ready = current.generation > previous && current.width > 0 &&
                        ((current.width > current.height) == wide) && nodes(current).isNotEmpty()
                }
                if (!ready) android.os.SystemClock.sleep(50)
            } while (!ready && android.os.SystemClock.uptimeMillis() < deadline)
            assertTrue("Rotation must attach a new eligible adaptive surface", ready)
        }
        instrumentation.waitForIdleSync()
        instrumentation.runOnMainSync {
            val current = view(currentActivity())
            if (nodes(current).any { it.optString("label") == "Open component gallery" }) action(current, "Open component gallery")
            action(current, "Section: Adaptive shell")
            action(current, "Catalog filter", 2, "003")
            action(current, "Apply catalog filter")
            val row = nodes(current).single { it.optString("label").startsWith("Catalog item 003") }
            action(current, row.getString("label"))
        }
        try {
            rotate(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE, true)
            instrumentation.runOnMainSync {
                val current = view(currentActivity())
                assertEquals("003", nodes(current).single { it.optString("label") == "Catalog filter" }.getString("value"))
                assertTrue(nodes(current).any { it.optString("value").startsWith("Selected catalog item · Catalog item 003") })
            }
        } finally {
            rotate(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT, false)
        }
        instrumentation.runOnMainSync {
            val current = view(currentActivity())
            assertEquals("003", nodes(current).single { it.optString("label") == "Catalog filter" }.getString("value"))
            assertTrue(nodes(current).single { it.optString("label").startsWith("Catalog item 003") }.optBoolean("selected"))
            action(current, "Clear catalog filter")
            action(current, "Back to overview")
            action(current, "Back to platform checks")
        }
    }

    @Test fun selectedTabsReachAndroidAccessibilityNodes() {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        fun await(label: String, selected: Boolean? = null): AccessibilityNodeInfo {
            val deadline = android.os.SystemClock.uptimeMillis() + 8_000
            do {
                val pending = java.util.ArrayDeque<AccessibilityNodeInfo>()
                automation.rootInActiveWindow?.let(pending::add)
                while (pending.isNotEmpty()) {
                    val node = pending.removeFirst()
                    if (node.contentDescription?.toString() == label &&
                        (selected == null || node.isSelected == selected)) return node
                    for (index in 0 until node.childCount) node.getChild(index)?.let(pending::add)
                }
                android.os.SystemClock.sleep(50)
            } while (android.os.SystemClock.uptimeMillis() < deadline)
            throw AssertionError("Android selected node unavailable: $label / $selected")
        }
        assertTrue(await("Open component gallery").performAction(AccessibilityNodeInfo.ACTION_CLICK))
        await("Section: Overview", true)
        await("Section: Adaptive shell", false).performAction(AccessibilityNodeInfo.ACTION_CLICK)
        await("Section: Adaptive shell", true)
        await("Section: Overview", false)
        assertTrue(await("Back to overview").performAction(AccessibilityNodeInfo.ACTION_CLICK))
        await("Section: Overview", true)
        assertTrue(await("Back to platform checks").performAction(AccessibilityNodeInfo.ACTION_CLICK))
    }

    @Test fun adaptiveCatalogAndSheetRetainStateAcrossSectionNavigation() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        instrumentation.runOnMainSync {
            val root = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            val view = (root.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView
            fun nodes(): List<JSONObject> {
                NativeBridge.accessibilitySnapshot(view.generation, true)
                NativeBridge.frame(view.generation)
                val array = JSONArray(NativeBridge.accessibilitySnapshot(view.generation, true))
                return (0 until array.length()).map(array::getJSONObject)
            }
            fun node(label: String) = nodes().singleOrNull { it.optString("label") == label }
                ?: throw AssertionError("Missing $label in ${nodes().map { it.optString("label") }}")
            fun action(label: String, operation: Int = 0, value: String = "") {
                assertTrue(NativeBridge.accessibilityAction(view.generation,
                    node(label).getString("id").toULong().toLong(), operation, value))
                NativeBridge.frame(view.generation)
            }
            fun values() = nodes().flatMap { listOf(it.optString("value"), it.optString("label")) }
            fun key(code: Int) {
                view.onKeyDown(code, KeyEvent(KeyEvent.ACTION_DOWN, code))
                view.onKeyUp(code, KeyEvent(KeyEvent.ACTION_UP, code))
                NativeBridge.frame(view.generation)
            }
            if (nodes().any { it.optString("label") == "Open component gallery" }) action("Open component gallery")
            action("Section: Adaptive shell")
            assertTrue(node("Section: Adaptive shell").optBoolean("selected"))
            assertFalse(node("Section: Overview").optBoolean("selected"))
            assertTrue(values().any { it.startsWith("Catalog results · 500 items") })
            val initialRows = nodes().filter { it.optString("label").startsWith("Catalog item ") }
            assertTrue("Only the virtual visible slice enters AccessKit", initialRows.size in 1..30)
            action("Jump to catalog end")
            assertTrue(nodes().any { it.optString("label").startsWith("Catalog item 499") })
            action("Catalog filter", 2, "003")
            action("Apply catalog filter")
            assertTrue(values().any { it.startsWith("Catalog results · 1 items") })
            val row = nodes().single { it.optString("label").startsWith("Catalog item 003") }
            action(row.getString("label"))
            assertTrue(node(row.getString("label")).optBoolean("selected"))
            action("Open catalog detail")
            assertTrue(values().any { it.startsWith("Selected catalog item · Catalog item 003") })
            action("Back to catalog list")
            action("Reduced motion")
            action("High contrast")
            action("Open adaptive sheet")
            assertNotNull(node("Adaptive shell sheet"))
            assertFalse(nodes().any { it.optString("label") == "Catalog filter" })
            assertFalse(nodes().any { it.optString("label") == "Section: Overview" })
            assertTrue(values().contains("Sheet motion · 0 ms"))
            key(KeyEvent.KEYCODE_ESCAPE)
            assertTrue(node("Open adaptive sheet").optBoolean("focused"))
            action("Open adaptive sheet")
            key(KeyEvent.KEYCODE_ENTER)
            assertTrue(node("Open adaptive sheet").optBoolean("focused"))
            action("Open adaptive sheet")
            key(KeyEvent.KEYCODE_SPACE)
            assertTrue(node("Open adaptive sheet").optBoolean("focused"))
            assertEquals("003", node("Catalog filter").getString("value"))
            action("Section: Overview")
            action("Section: Adaptive shell")
            assertEquals("003", node("Catalog filter").getString("value"))
            assertTrue(values().any { it.startsWith("Catalog results · 1 items") })
            action("Clear catalog filter")
            assertTrue(values().any { it.startsWith("Catalog results · 500 items") })
            action("Back to overview")
            action("Back to platform checks")
            NativeBridge.accessibilitySnapshot(view.generation, false)
        }
    }
}
