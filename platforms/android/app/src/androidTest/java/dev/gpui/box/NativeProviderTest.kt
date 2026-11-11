package dev.gpui.box

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.After
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class NativeProviderTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)

    @After fun releaseAccessibilityConnectionAndRestorePlatformPage() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.runOnMainSync {
            val content = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            val current = (content.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView
            NativeBridge.frame(current.generation)
            val array = JSONArray(NativeBridge.accessibilitySnapshot(current.generation, true))
            (0 until array.length()).map(array::getJSONObject).firstOrNull {
                it.optString("label") == "Back to platform checks"
            }?.let { back ->
                NativeBridge.accessibilityAction(current.generation, back.getString("id").toULong().toLong(), 0, "")
                NativeBridge.frame(current.generation)
            }
        }
        instrumentation.getUiAutomation(android.app.UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
        instrumentation.waitForIdleSync()
    }

    /** Queries travel through UiAutomation's accessibility connection to the provider. */
    private fun find(label: String): AccessibilityNodeInfo? {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        val root = automation.rootInActiveWindow ?: return null
        val pending = java.util.ArrayDeque<AccessibilityNodeInfo>()
        pending.add(root)
        while (pending.isNotEmpty()) {
            val node = pending.removeFirst()
            if (node.contentDescription?.toString() == label) return node
            for (index in 0 until node.childCount) node.getChild(index)?.let(pending::add)
        }
        return null
    }

    private fun awaitNode(label: String, predicate: (AccessibilityNodeInfo) -> Boolean = { true }): AccessibilityNodeInfo {
        val deadline = android.os.SystemClock.uptimeMillis() + 8000
        do {
            find(label)?.let { if (predicate(it)) return it }
            android.os.SystemClock.sleep(50)
        } while (android.os.SystemClock.uptimeMillis() < deadline)
        throw AssertionError("Accessible node unavailable or predicate failed: $label")
    }

    @Test fun androidProviderSupportsTextSelectionFocusAndActions() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // Attaching UiAutomation enables Android accessibility without installing TalkBack.
        instrumentation.uiAutomation
        instrumentation.waitForIdleSync()
        val field = awaitNode("Primary draft") { it.isEditable }
        assertEquals("android.widget.EditText", field.className.toString())
        assertTrue(field.performAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS))
        awaitNode("Primary draft") { it.isAccessibilityFocused }
        assertTrue(field.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "Provider 你好🙂")
        }))
        awaitNode("Primary draft") { it.text?.toString() == "Provider 你好🙂" }
        assertTrue(awaitNode("Primary draft").performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, Bundle().apply {
            putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, 2)
            putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, 8)
        }))
        awaitNode("Primary draft") { it.textSelectionStart == 2 && it.textSelectionEnd == 8 }
        assertTrue(awaitNode("Primary draft").performAction(AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS))
        assertTrue(awaitNode("Open component gallery").performAction(AccessibilityNodeInfo.ACTION_CLICK))
        awaitNode("Dark appearance")
    }

    @Test fun lifecycleServiceReceivesHostStateAndWarmIntent() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.uiAutomation
        instrumentation.waitForIdleSync()
        var view: GpuiView? = null
        var before = 0
        fun activationCount(current: GpuiView): Int {
            NativeBridge.frame(current.generation)
            val array = JSONArray(NativeBridge.accessibilitySnapshot(current.generation, true))
            return (0 until array.length()).map(array::getJSONObject)
                .single { it.optString("value").startsWith("Activation events ") }
                .getString("value").substringAfterLast(' ').toInt()
        }
        instrumentation.runOnMainSync {
            val content = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            val current = (content.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView
            view = current
            activityRule.activity.updateHostState()
            val state = JSONObject(NativeBridge.hostSnapshot())
            assertEquals(current.generation, state.getLong("generation"))
            assertTrue(state.getString("phase").startsWith("Foreground"))
            assertEquals(current.resources.configuration.fontScale.toDouble(), state.getDouble("textScale"), 0.001)
            assertEquals(current.resources.configuration.locales[0].toLanguageTag(), state.getString("locale"))
            NativeBridge.hostState(current.generation - 1, false, false, true, false, 5f, "xx", true)
            val unchanged = JSONObject(NativeBridge.hostSnapshot())
            assertEquals(state.getString("phase"), unchanged.getString("phase"))
            assertEquals(state.getString("locale"), unchanged.getString("locale"))
            assertEquals(state.getDouble("textScale"), unchanged.getDouble("textScale"), 0.001)
            NativeBridge.memoryWarning()
            assertEquals("Warning", JSONObject(NativeBridge.hostSnapshot()).getString("memory"))
            // Open gallery through a real ACTION_VIEW Intent, delivered by onNewIntent.
            activityRule.activity.startActivity(Intent(Intent.ACTION_VIEW,
                Uri.parse("gpui-box-example://gallery/warm")).setPackage(activityRule.activity.packageName))
        }
        instrumentation.waitForIdleSync()
        awaitNode("Back to platform checks")
        instrumentation.runOnMainSync {
            val current = view!!
            activityRule.activity.deliverPendingUrls()
            before = activationCount(current)
            assertTrue(before > 0)
            activityRule.activity.deliverPendingUrls()
            assertEquals(before, activationCount(current))
            val array = JSONArray(NativeBridge.accessibilitySnapshot(current.generation, true))
            val back = (0 until array.length()).map(array::getJSONObject).single { it.optString("label") == "Back to platform checks" }
            assertTrue(NativeBridge.accessibilityAction(current.generation, back.getString("id").toULong().toLong(), 0, ""))
            NativeBridge.frame(current.generation)
        }
    }
}
