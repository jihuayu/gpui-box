package dev.gpui.box

import android.view.KeyEvent
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.json.JSONArray
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class AccessibilityGalleryTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)
    @Test fun accessKitTreeExposesEditableFieldsAndAdaptiveControls() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        instrumentation.runOnMainSync {
            val root = activityRule.activity.findViewById<FrameLayout>(android.R.id.content)
            val view = (root.getChildAt(0) as FrameLayout).getChildAt(0) as GpuiView
            val generation = view.generation
            NativeBridge.accessibilitySnapshot(generation, true)
            NativeBridge.frame(generation)
            var nodes = JSONArray(NativeBridge.accessibilitySnapshot(generation, true))
            val fields = (0 until nodes.length()).map { nodes.getJSONObject(it) }.filter { it.getString("class") == "android.widget.EditText" }
            assertEquals(2, fields.size)
            val initialPrimary = fields.single { it.optString("label") == "Primary draft" }
            assertTrue(NativeBridge.accessibilityAction(generation, initialPrimary.getString("id").toULong().toLong(), 2, "Accessible draft"))
            NativeBridge.frame(generation)
            nodes = JSONArray(NativeBridge.accessibilitySnapshot(generation, true))
            val primary = (0 until nodes.length()).map { nodes.getJSONObject(it) }
                .single { it.optString("label") == "Primary draft" }
            assertTrue(primary.optBoolean("selectable"))
            val primaryId = primary.getString("id").toULong().toLong()
            assertTrue(NativeBridge.accessibilityAction(generation, primaryId, 3, "[2,7]"))
            NativeBridge.frame(generation)
            assertEquals(2, NativeBridge.textState(generation)[0])
            assertEquals(7, NativeBridge.textState(generation)[1])
            assertFalse(NativeBridge.accessibilityAction(generation, primaryId, 3, "[-1,7]"))
            assertFalse(NativeBridge.accessibilityAction(generation, primaryId, 3, "[0,999999]"))
            val bounds = primary.getJSONArray("bounds")
            assertTrue("AccessKit bounds are physical pixels within the viewport", bounds.getDouble(2) <= view.width)
            val gallery = (0 until nodes.length()).map { nodes.getJSONObject(it) }
                .single { it.optString("label") == "Open component gallery" }
            assertTrue(NativeBridge.accessibilityAction(generation, gallery.getString("id").toULong().toLong(), 0, ""))
            NativeBridge.frame(generation)
            nodes = JSONArray(NativeBridge.accessibilitySnapshot(generation, true))
            val labels = (0 until nodes.length()).map { nodes.getJSONObject(it).optString("label") }
            assertTrue("Gallery exposes the theme control", labels.contains("Dark appearance"))
            assertTrue("Gallery exposes the large-text control", labels.contains("Large text"))
            assertFalse(NativeBridge.accessibilityAction(generation - 1, 0, 0, ""))
            val review = (0 until nodes.length()).map { nodes.getJSONObject(it) }
                .single { it.optString("label") == "Review settings" }
            assertTrue(NativeBridge.accessibilityAction(generation, review.getString("id").toULong().toLong(), 1, ""))
            NativeBridge.frame(generation)
            assertTrue(view.onKeyDown(KeyEvent.KEYCODE_ENTER, KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER)))
            assertTrue(view.onKeyUp(KeyEvent.KEYCODE_ENTER, KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER)))
            NativeBridge.frame(generation)
            nodes = JSONArray(NativeBridge.accessibilitySnapshot(generation, true))
            assertFalse("Modal removes background controls from the tree", (0 until nodes.length()).any {
                nodes.getJSONObject(it).optString("label") == "Dark appearance"
            })
            assertTrue("Modal exposes its close action", (0 until nodes.length()).any {
                nodes.getJSONObject(it).optString("label") == "Close"
            })
            assertTrue(view.onKeyDown(KeyEvent.KEYCODE_ESCAPE, KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ESCAPE)))
            NativeBridge.frame(generation)
            nodes = JSONArray(NativeBridge.accessibilitySnapshot(generation, true))
            assertTrue("Dismiss restores trigger focus", (0 until nodes.length()).any {
                val node = nodes.getJSONObject(it)
                node.optString("label") == "Review settings" && node.optBoolean("focused")
            })
            val home = (0 until nodes.length()).map { nodes.getJSONObject(it) }
                .single { it.optString("label") == "Back to platform checks" }
            assertTrue(NativeBridge.accessibilityAction(generation, home.getString("id").toULong().toLong(), 0, ""))
            NativeBridge.frame(generation)
            NativeBridge.accessibilitySnapshot(generation, false)
        }
    }
}
