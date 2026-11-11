package dev.gpui.box

import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class ResponsiveModelTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)

    @Test fun committedDraftQueriesAndActivationUseRetainedScreenState() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        instrumentation.runOnMainSync {
            val root = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            val view = (root.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView
            val generation = view.generation
            fun nodes(): List<JSONObject> {
                NativeBridge.accessibilitySnapshot(generation, true)
                NativeBridge.frame(generation)
                val array = JSONArray(NativeBridge.accessibilitySnapshot(generation, true))
                return (0 until array.length()).map(array::getJSONObject)
            }
            fun node(label: String): JSONObject {
                val current = nodes()
                return current.singleOrNull { it.optString("label") == label }
                    ?: throw AssertionError("Missing $label in ${current.map { it.optString("label") }}")
            }
            fun activationStatus() = nodes().single { it.optString("value").startsWith("Activation events ") }.getString("value")
            fun action(label: String, action: Int = 0, value: String = "") {
                assertTrue(NativeBridge.accessibilityAction(generation, node(label).getString("id").toULong().toLong(), action, value))
                NativeBridge.frame(generation)
            }
            fun contains(value: String) = nodes().any { it.optString("value") == value }
            if (nodes().any { it.optString("label") == "Open component gallery" }) action("Open component gallery")
            action("Primary draft", 2, "Committed 你好🙂")
            assertFalse(contains("Committed primary draft · Committed 你好🙂"))
            action("Commit primary draft")
            assertTrue(contains("Committed primary draft · Committed 你好🙂"))
            action("Primary draft", 2, "Uncommitted draft")
            assertTrue(contains("Committed primary draft · Committed 你好🙂"))
            action("Large text")
            assertEquals("Uncommitted draft", node("Primary draft").getString("value"))
            action("Start item request")
            assertEquals("Loading items…", node("Item query status").getString("value"))
            action("Complete item request")
            assertTrue(contains("Saved item 你好"))
            action("Fail item refresh")
            assertTrue(contains("Saved item 你好"))
            assertEquals("Offline. Showing saved items.", node("Item query status").getString("value"))
            action("Clear account data")
            assertFalse(contains("Saved item 你好"))
            action("Large text")
            val before = activationStatus()
            assertFalse(NativeBridge.openUrlEvent(generation - 1, "stale-event", "gpui-box-example://gallery"))
            assertTrue(NativeBridge.openUrlEvent(generation, "model-test-event", "gpui-box-example://gallery"))
            NativeBridge.frame(generation)
            val after = activationStatus()
            assertNotEquals(before, after)
            assertTrue(NativeBridge.openUrlEvent(generation, "model-test-event", "gpui-box-example://gallery"))
            NativeBridge.frame(generation)
            assertEquals(after, activationStatus())
            action("Back to platform checks")
            NativeBridge.accessibilitySnapshot(generation, false)
        }
    }
}
