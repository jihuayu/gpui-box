package dev.gpui.box

import android.graphics.Rect
import android.os.Bundle
import android.view.View
import android.view.MotionEvent
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
    private var hovered = -1
    private val manager = view.context.getSystemService(AccessibilityManager::class.java)
    private var previous = ""
    private var generation = 0L
    private val listener = AccessibilityManager.AccessibilityStateChangeListener { view.post { refresh() } }
    fun attach() { manager.addAccessibilityStateChangeListener(listener); previous = "" }
    fun detach() { manager.removeAccessibilityStateChangeListener(listener); nodes = emptyMap(); focused = -1; hovered = -1 }

    fun refresh() {
        NativeBridge.checkMain()
        if (generation != view.generation) {
            generation = view.generation
            previous = ""
            ids.clear()
            nodes = emptyMap()
            root = -1
            focused = -1
            hovered = -1
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
        val old = nodes
        if (focused !in updated && focused != -1) {
            event(focused, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED)
            focused = -1
        }
        if (hovered !in updated) hovered = -1
        nodes = updated
        if (manager.isEnabled) {
            event(root, AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED)
            for ((id, node) in nodes) {
                val prior = old[id] ?: continue
                if (node.optBoolean("focused") && !prior.optBoolean("focused")) event(id, AccessibilityEvent.TYPE_VIEW_FOCUSED)
                if (node.optBoolean("selected") && !prior.optBoolean("selected")) event(id, AccessibilityEvent.TYPE_VIEW_SELECTED)
                if (node.optString("selection") != prior.optString("selection")) event(id, AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED)
                if (node.optString("value") != prior.optString("value")) {
                    val type = if (node.optBoolean("editable")) AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED
                        else if (node.optBoolean("live")) AccessibilityEvent.TYPE_ANNOUNCEMENT else continue
                    event(id, type, prior)
                }
            }
        }
    }
    private fun bounds(node: JSONObject): Rect {
        val bounds = node.optJSONArray("bounds")
        return if (bounds != null) Rect(
            kotlin.math.floor(bounds.getDouble(0)).toInt(), kotlin.math.floor(bounds.getDouble(1)).toInt(),
            kotlin.math.ceil(bounds.getDouble(2)).toInt(), kotlin.math.ceil(bounds.getDouble(3)).toInt(),
        ) else Rect(0, 0, view.width, view.height)
    }
    private fun exposed(node: JSONObject) = !node.optBoolean("hidden") && !node.optBoolean("textRun")
    private fun hitTest(id: Int, x: Int, y: Int, visited: MutableSet<Int>): Int {
        if (!visited.add(id)) return -1
        val node = nodes[id] ?: return -1
        if (!exposed(node)) return -1
        val children = node.getJSONArray("children")
        // Later siblings paint above earlier siblings. Descendants precede containers.
        for (index in children.length() - 1 downTo 0) {
            val child = ids[children.getString(index)] ?: continue
            val target = hitTest(child, x, y, visited)
            if (target != -1) return target
        }
        return if (node.optJSONArray("bounds") != null && bounds(node).contains(x, y) &&
            (node.optBoolean("focusable") || node.optBoolean("clickable") ||
                !node.optString("label").let { it.isEmpty() || it == "null" } ||
                !node.optString("value").let { it.isEmpty() || it == "null" })) id else -1
    }
    fun hover(motion: MotionEvent): Boolean {
        if (!manager.isEnabled || !manager.isTouchExplorationEnabled) return false
        val target = if (motion.actionMasked == MotionEvent.ACTION_HOVER_EXIT) -1 else
            hitTest(root, motion.x.toInt(), motion.y.toInt(), mutableSetOf())
        if (hovered != target) {
            if (hovered != -1) event(hovered, AccessibilityEvent.TYPE_VIEW_HOVER_EXIT)
            hovered = target
            if (target != -1) event(target, AccessibilityEvent.TYPE_VIEW_HOVER_ENTER)
        }
        return target != -1 || motion.actionMasked == MotionEvent.ACTION_HOVER_EXIT
    }
    override fun createAccessibilityNodeInfo(id: Int): AccessibilityNodeInfo? {
        if (id == HOST_VIEW_ID) return AccessibilityNodeInfo.obtain(view).apply {
            view.onInitializeAccessibilityNodeInfo(this)
            if (root in nodes) addChild(view, root)
        }
        val node = nodes[id] ?: return null
        if (!exposed(node)) return null
        return AccessibilityNodeInfo.obtain().apply {
            setSource(view, id)
            packageName = view.context.packageName
            className = node.getString("class")
            isEnabled = node.optBoolean("enabled", true)
            val visible = bounds(node).intersect(0, 0, view.width, view.height)
            isVisibleToUser = view.isShown && visible
            isFocusable = node.optBoolean("focusable")
            isFocused = node.optBoolean("focused")
            isSelected = node.optBoolean("selected")
            isAccessibilityFocused = focused == id
            contentDescription = node.optString("label").takeUnless { it == "null" }
            text = node.optString("value").takeUnless { it == "null" }
            viewIdResourceName = node.optString("authorId").takeUnless { it == "null" }
            val parent = ids[node.optString("parent")]
            if (parent != null) setParent(view, parent) else setParent(view)
            val children = node.getJSONArray("children")
            for (index in 0 until children.length()) ids[children.getString(index)]?.let { child ->
                if (nodes[child]?.let(::exposed) == true) addChild(view, child)
            }
            val rect = bounds(node)
            val parentBounds = parent?.let { nodes[it]?.let(::bounds) }
            val local = Rect(rect)
            if (parentBounds != null) local.offset(-parentBounds.left, -parentBounds.top)
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
            if (node.optBoolean("selectable")) {
                addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_SELECTION)
                node.optJSONArray("selection")?.let { setTextSelection(it.getInt(0), it.getInt(1)) }
            }
            liveRegion = if (node.optBoolean("live")) View.ACCESSIBILITY_LIVE_REGION_POLITE else View.ACCESSIBILITY_LIVE_REGION_NONE
            if (!node.isNull("checked")) { isCheckable = true; isChecked = node.getBoolean("checked") }
        }
    }
    override fun findFocus(focus: Int): AccessibilityNodeInfo? = when (focus) {
        AccessibilityNodeInfo.FOCUS_ACCESSIBILITY -> createAccessibilityNodeInfo(focused)
        AccessibilityNodeInfo.FOCUS_INPUT -> nodes.entries.firstOrNull { it.value.optBoolean("focused") }?.key?.let(::createAccessibilityNodeInfo)
        else -> null
    }
    override fun findAccessibilityNodeInfosByText(text: String, id: Int): List<AccessibilityNodeInfo> {
        val pending = java.util.ArrayDeque<Int>()
        pending.add(if (id == HOST_VIEW_ID) root else id)
        val result = mutableListOf<AccessibilityNodeInfo>()
        val visited = mutableSetOf<Int>()
        while (pending.isNotEmpty()) {
            val current = pending.removeFirst()
            if (!visited.add(current)) continue
            val node = nodes[current] ?: continue
            if (node.optString("label").contains(text, true) || node.optString("value").contains(text, true)) createAccessibilityNodeInfo(current)?.let(result::add)
            val children = node.getJSONArray("children")
            for (index in 0 until children.length()) ids[children.getString(index)]?.let(pending::add)
        }
        return result
    }

    override fun performAction(id: Int, action: Int, arguments: Bundle?): Boolean {
        val node = nodes[id] ?: return false
        if (!manager.isEnabled || !view.isShown || !node.optBoolean("enabled", true) || !exposed(node)) return false
        when (action) {
            AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS -> {
                if (focused == id) return false
                if (focused != -1) event(focused, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED)
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
            AccessibilityNodeInfo.ACTION_SET_SELECTION -> 3
            else -> return false
        }
        val text = if (operation == 3) {
            val start = arguments?.getInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, -1) ?: -1
            val end = arguments?.getInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, -1) ?: -1
            if (start < 0 || end < 0) return false
            "[$start,$end]"
        } else arguments?.getCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE)?.toString() ?: ""
        return NativeBridge.accessibilityAction(view.generation, node.getString("id").toULong().toLong(), operation, text)
    }
    private fun event(id: Int, type: Int, prior: JSONObject? = null) {
        if (!manager.isEnabled) return
        val event = AccessibilityEvent.obtain(type)
        event.setSource(view, id)
        event.packageName = view.context.packageName
        event.className = nodes[id]?.optString("class") ?: "android.view.View"
        nodes[id]?.let { node ->
            val value = node.optString("value").takeUnless { it == "null" } ?: node.optString("label").takeUnless { it == "null" }
            if (value != null) event.text.add(value)
            if (type == AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED && prior != null) {
                val before = prior.optString("value").takeUnless { it == "null" } ?: ""
                val after = value ?: ""
                val prefix = before.commonPrefixWith(after).length
                val suffix = before.substring(prefix).commonSuffixWith(after.substring(prefix)).length
                event.beforeText = before
                event.fromIndex = prefix
                event.removedCount = before.length - prefix - suffix
                event.addedCount = after.length - prefix - suffix
            }
            node.optJSONArray("selection")?.let { selection ->
                if (type == AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED) {
                    event.fromIndex = selection.getInt(0)
                    event.toIndex = selection.getInt(1)
                    event.itemCount = value?.length ?: 0
                }
            }
        }
        view.parent?.requestSendAccessibilityEvent(view, event)
    }
}
