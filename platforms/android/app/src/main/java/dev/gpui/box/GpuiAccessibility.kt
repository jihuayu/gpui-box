package dev.gpui.box

import android.graphics.Rect
import android.os.Bundle
import android.view.View
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityManager
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityNodeProvider
import org.json.JSONArray
import org.json.JSONObject

/** Stable virtual IDs adapt the coordinated AccessKit tree to Android. */
class GpuiAccessibility(private val view: GpuiView) : AccessibilityNodeProvider() {
    private val ids = mutableMapOf<String, Int>()
    private var nextId = 1
    private var nodes = emptyMap<Int, JSONObject>()
    private var root = -1
    private var focused = -1
    private val manager = view.context.getSystemService(AccessibilityManager::class.java)
    private var previous = ""
    private var generation = 0L
    private val listener = AccessibilityManager.AccessibilityStateChangeListener { view.post { refresh() } }
    fun attach() { manager.addAccessibilityStateChangeListener(listener); previous = "" }
    fun detach() { manager.removeAccessibilityStateChangeListener(listener); nodes = emptyMap(); focused = -1 }

    fun refresh() {
        NativeBridge.checkMain()
        if (generation != view.generation) {
            generation = view.generation
            previous = ""
            ids.clear()
            nodes = emptyMap()
            root = -1
            focused = -1
        }
        val snapshot = NativeBridge.accessibilitySnapshot(view.generation, manager.isEnabled)
        if (snapshot == previous) return
        previous = snapshot
        val array = JSONArray(snapshot)
        val updated = mutableMapOf<Int, JSONObject>()
        root = -1
        for (index in 0 until array.length()) {
            val node = array.getJSONObject(index)
            val id = ids.getOrPut(node.getString("id")) { nextId++ }
            updated[id] = node
            if (node.optBoolean("root")) root = id
        }
        nodes = updated
        if (focused !in nodes) focused = -1
        if (manager.isEnabled) view.sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED)
    }
    override fun createAccessibilityNodeInfo(id: Int): AccessibilityNodeInfo? {
        if (id == HOST_VIEW_ID) return AccessibilityNodeInfo.obtain(view).apply {
            view.onInitializeAccessibilityNodeInfo(this)
            if (root in nodes) addChild(view, root)
        }
        val node = nodes[id] ?: return null
        return AccessibilityNodeInfo.obtain().apply {
            setSource(view, id)
            packageName = view.context.packageName
            className = node.getString("class")
            isEnabled = node.optBoolean("enabled", true)
            isVisibleToUser = view.isShown
            isFocusable = node.optBoolean("focusable")
            isFocused = node.optBoolean("focused")
            isAccessibilityFocused = focused == id
            contentDescription = node.optString("label").takeUnless { it == "null" }
            text = node.optString("value").takeUnless { it == "null" }
            viewIdResourceName = node.optString("authorId").takeUnless { it == "null" }
            val parent = ids[node.optString("parent")]
            if (parent != null) setParent(view, parent) else setParent(view)
            val children = node.getJSONArray("children")
            for (index in 0 until children.length()) ids[children.getString(index)]?.let { addChild(view, it) }
            val scale = view.resources.displayMetrics.density
            val bounds = node.optJSONArray("bounds")
            val rect = if (bounds != null) Rect(
                (bounds.getDouble(0) * scale).toInt(), (bounds.getDouble(1) * scale).toInt(),
                (bounds.getDouble(2) * scale).toInt(), (bounds.getDouble(3) * scale).toInt(),
            ) else Rect(0, 0, view.width, view.height)
            val parentBounds = parent?.let { nodes[it]?.optJSONArray("bounds") }
            val local = Rect(rect)
            if (parentBounds != null) local.offset(-(parentBounds.getDouble(0) * scale).toInt(), -(parentBounds.getDouble(1) * scale).toInt())
            setBoundsInParent(local)
            val location = IntArray(2)
            view.getLocationOnScreen(location)
            rect.offset(location[0], location[1])
            setBoundsInScreen(rect)
            addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_ACCESSIBILITY_FOCUS)
            addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLEAR_ACCESSIBILITY_FOCUS)
            if (node.optBoolean("focusable")) addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_FOCUS)
            if (node.optBoolean("clickable")) { isClickable = true; addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK) }
            if (node.optBoolean("editable")) { isEditable = true; addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_TEXT) }
            if (!node.isNull("checked")) { isCheckable = true; isChecked = node.getBoolean("checked") }
        }
    }
    override fun findAccessibilityNodeInfosByText(text: String, id: Int): List<AccessibilityNodeInfo> =
        nodes.filterValues { it.optString("label").contains(text, true) || it.optString("value").contains(text, true) }
            .keys.mapNotNull(::createAccessibilityNodeInfo)

    override fun performAction(id: Int, action: Int, arguments: Bundle?): Boolean {
        val node = nodes[id] ?: return false
        when (action) {
            AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS -> {
                focused = id
                event(id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED)
                return true
            }
            AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS -> {
                if (focused != id) return false
                focused = -1
                event(id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED)
                return true
            }
        }
        val operation = when (action) {
            AccessibilityNodeInfo.ACTION_CLICK -> 0
            AccessibilityNodeInfo.ACTION_FOCUS -> 1
            AccessibilityNodeInfo.ACTION_SET_TEXT -> 2
            else -> return false
        }
        val text = arguments?.getCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE)?.toString() ?: ""
        return NativeBridge.accessibilityAction(view.generation, node.getString("id").toULong().toLong(), operation, text)
    }
    private fun event(id: Int, type: Int) {
        if (!manager.isEnabled) return
        val event = AccessibilityEvent.obtain(type)
        event.setSource(view, id)
        event.packageName = view.context.packageName
        event.className = nodes[id]?.optString("class") ?: "android.view.View"
        view.parent?.requestSendAccessibilityEvent(view, event)
    }
}
