package dev.gpui.box

import android.app.UiAutomation
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class PermissionProviderTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val packageName get() = instrumentation.targetContext.packageName

    private fun shell(command: String) {
        instrumentation.uiAutomation.executeShellCommand(command).use { descriptor ->
            java.io.FileInputStream(descriptor.fileDescriptor).use { it.readBytes() }
        }
    }

    private fun view(): GpuiView {
        val content = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
        return ((content.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView)
    }

    private fun nodes(): List<JSONObject> {
        val current = view()
        NativeBridge.accessibilitySnapshot(current.generation, true)
        NativeBridge.frame(current.generation)
        val array = JSONArray(NativeBridge.accessibilitySnapshot(current.generation, true))
        return (0 until array.length()).map(array::getJSONObject)
    }

    private fun action(label: String) {
        val deadline = SystemClock.uptimeMillis() + 10000
        var ready = false
        do {
            instrumentation.runOnMainSync { ready = activityRule.activity.canPresentPermissionUi() }
            if (ready) break
            SystemClock.sleep(50)
        } while (SystemClock.uptimeMillis() < deadline)
        assertTrue("Activity must regain foreground focus before $label", ready)
        instrumentation.runOnMainSync {
            val node = nodes().single { it.optString("label") == label }
            assertTrue(NativeBridge.accessibilityAction(view().generation, node.getString("id").toULong().toLong(), 0, ""))
            NativeBridge.frame(view().generation)
        }
    }

    private fun awaitValue(label: String, expected: String) {
        val deadline = SystemClock.uptimeMillis() + 10000
        var value = ""
        do {
            instrumentation.runOnMainSync {
                value = nodes().firstOrNull { it.optString("label") == label }?.optString("value") ?: ""
            }
            if (value.contains(expected)) return
            SystemClock.sleep(50)
        } while (SystemClock.uptimeMillis() < deadline)
        fail("Expected $expected in $label, received $value")
    }

    private fun systemNode(predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        val queue = java.util.ArrayDeque<AccessibilityNodeInfo>()
        instrumentation.uiAutomation.rootInActiveWindow?.let(queue::add)
        while (queue.isNotEmpty()) {
            val node = queue.removeFirst()
            if (predicate(node)) return node
            for (index in 0 until node.childCount) node.getChild(index)?.let(queue::add)
        }
        return null
    }

    private fun systemAction(id: String) {
        val deadline = SystemClock.uptimeMillis() + 10000
        do {
            val node = systemNode { it.viewIdResourceName?.endsWith(":id/$id") == true ||
                (id == "permission_deny_button" && it.viewIdResourceName?.endsWith(":id/permission_deny_and_dont_ask_again_button") == true) }
            if (node != null) { assertTrue(node.performAction(AccessibilityNodeInfo.ACTION_CLICK)); return }
            SystemClock.sleep(50)
        } while (SystemClock.uptimeMillis() < deadline)
        fail("System control unavailable: $id")
    }

    private fun awaitSettingsWindow() {
        val deadline = SystemClock.uptimeMillis() + 20000
        do {
            if (systemNode { it.packageName?.toString() == "com.android.settings" } != null) return
            SystemClock.sleep(100)
        } while (SystemClock.uptimeMillis() < deadline)
        fail("Android Settings did not become accessible")
    }

    private fun resetAndOpenGallery() {
        shell("pm revoke $packageName android.permission.POST_NOTIFICATIONS")
        shell("pm clear-permission-flags $packageName android.permission.POST_NOTIFICATIONS user-set user-fixed")
        instrumentation.runOnMainSync {
            instrumentation.targetContext.getSharedPreferences("gpui-box.permissions", 0).edit().clear().commit()
            activityRule.activity.updateHostState()
            if (nodes().any { it.optString("label") == "Open component gallery" }) {
                val node = nodes().single { it.optString("label") == "Open component gallery" }
                NativeBridge.accessibilityAction(view().generation, node.getString("id").toULong().toLong(), 0, "")
                NativeBridge.frame(view().generation)
            }
        }
        action("Refresh permission status")
        awaitValue("Notification permission status", "NotDetermined")
    }

    @After fun restorePlatformPage() {
        instrumentation.runOnMainSync {
            nodes().firstOrNull { it.optString("label") == "Back to platform checks" }?.let {
                NativeBridge.accessibilityAction(view().generation, it.getString("id").toULong().toLong(), 0, "")
                NativeBridge.frame(view().generation)
            }
        }
        instrumentation.getUiAutomation(UiAutomation.FLAG_DONT_USE_ACCESSIBILITY)
    }

    @Test fun denialPermanentRepeatAndSettingsResumeUseNativeStatus() {
        resetAndOpenGallery()
        action("Request notification permission")
        systemAction("permission_deny_button")
        awaitValue("Notification permission status", "can_request_again: true")
        action("Request notification permission")
        systemAction("permission_deny_button")
        awaitValue("Notification permission status", "can_request_again: false")
        awaitValue("Permission operation result", "prompted: true")
        action("Request notification permission")
        awaitValue("Permission operation result", "prompted: false")
        action("Open notification settings")
        awaitValue("Permission operation result", "Opened")
        awaitSettingsWindow()
        // Change actual Android authorization while the Activity is paused.
        // uiAutomation delegates the permission change to PackageManager.
        shell("pm grant $packageName android.permission.POST_NOTIFICATIONS")
        shell("input keyevent KEYCODE_BACK")
        awaitValue("Notification permission status", "Granted")
        instrumentation.runOnMainSync {
            assertFalse(NativeBridge.permissionResult(Long.MAX_VALUE, 0))
            assertEquals(1, activityRule.activity.permissions.status(0))
            val permissions = JSONObject(NativeBridge.hostSnapshot()).getJSONArray("permissions")
            val microphone = (0 until permissions.length()).map(permissions::getJSONObject).single { it.getString("kind") == "Microphone" }
            assertEquals("InvalidConfiguration", microphone.getString("error"))
            assertEquals("Some(RepairConfiguration)", microphone.getString("recovery"))
            val photos = (0 until permissions.length()).map(permissions::getJSONObject).single { it.getString("kind") == "Photos" }
            assertEquals("Unsupported", photos.getString("error"))
        }
    }

    @Test fun missingConfigurationAndUnsupportedKindsAreExplicit() {
        instrumentation.runOnMainSync {
            assertEquals(-2, activityRule.activity.permissions.status(2))
            assertEquals(-1, activityRule.activity.permissions.status(99))
            assertEquals(-1, activityRule.activity.permissions.settings(99))
            assertFalse(NativeBridge.permissionResult(Long.MAX_VALUE - 1, 0))
            assertFalse(NativeBridge.permissionPending(Long.MAX_VALUE - 1))
        }
    }

    @Test fun cameraGrantCompletesRustRequestAndSkipsAlreadyGrantedPrompt() {
        instrumentation.runOnMainSync {
            if (nodes().any { it.optString("label") == "Open component gallery" }) {
                val node = nodes().single { it.optString("label") == "Open component gallery" }
                NativeBridge.accessibilityAction(view().generation, node.getString("id").toULong().toLong(), 0, "")
                NativeBridge.frame(view().generation)
            }
        }
        action("Request camera permission")
        systemAction("permission_allow_foreground_only_button")
        awaitValue("Camera permission status", "Granted")
        awaitValue("Permission operation result", "prompted: true")
        action("Request camera permission")
        awaitValue("Permission operation result", "prompted: false")
    }
}
