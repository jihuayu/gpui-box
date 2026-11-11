package dev.gpui.box

import android.app.Activity
import android.content.Intent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.IntentFilter
import android.app.KeyguardManager
import android.content.res.Configuration
import android.database.ContentObserver
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.os.Bundle
import android.content.ComponentCallbacks2
import android.util.Log
import android.view.WindowInsetsAnimation
import android.view.WindowInsets
import android.window.OnBackAnimationCallback
import android.window.BackEvent
import android.window.OnBackInvokedDispatcher
import android.widget.FrameLayout

class MainActivity : Activity() {
    private lateinit var gpui: GpuiView
    private lateinit var services: GpuiServices
    lateinit var permissions: GpuiPermissions
        private set
    private val pendingUrls = java.util.ArrayDeque<Pair<String, String>>()
    private var backRegistered = false
    private var predictiveClaimed = false
    private var predictiveStarted = false
    private var resumed = false
    private var started = false
    private val lockReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) { updateHostState() }
    }
    private val motionObserver = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) { updateHostState() }
    }
    fun updateHostState() {
        if (!::gpui.isInitialized || gpui.generation == 0L) return
        val config = resources.configuration
        NativeBridge.hostState(gpui.generation, started, resumed && hasWindowFocus(),
            getSystemService(KeyguardManager::class.java).isDeviceLocked,
            config.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES,
            config.fontScale, config.locales[0].toLanguageTag(),
            Settings.Global.getFloat(contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f)
    }
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        updateHostState()
    }
    override fun onConfigurationChanged(configuration: Configuration) {
        super.onConfigurationChanged(configuration)
        updateHostState()
    }
    private val backCallback = object : OnBackAnimationCallback {
        override fun onBackStarted(event: BackEvent) {
            // Android 15 ViewRootImpl sends a synthetic (0, 0) start for
            // KEYCODE_BACK. It has no gesture position and uses committed-back
            // dispatch. Real edge gestures retain their original hit test.
            predictiveStarted = event.touchX != 0f || event.touchY != 0f
            if (!predictiveStarted) return
            val location = IntArray(2)
            gpui.getLocationOnScreen(location)
            predictiveClaimed = NativeBridge.backGesture(gpui.generation, 0,
                event.touchX - location[0], event.touchY - location[1], 0f)
        }
        override fun onBackProgressed(event: BackEvent) {
            if (predictiveClaimed) NativeBridge.backGesture(gpui.generation, 1, 0f, 0f, event.progress)
        }
        override fun onBackCancelled() {
            NativeBridge.backGesture(gpui.generation, 3, 0f, 0f, 0f)
            predictiveClaimed = false
            predictiveStarted = false
        }
        override fun onBackInvoked() {
            if (gpui.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true) {
                onBackCancelled()
                Log.i("gpui-box", "System back dismissed the IME")
                gpui.hideKeyboard()
            } else {
                val handled = if (predictiveClaimed) NativeBridge.backGesture(gpui.generation, 2, 0f, 0f, 1f)
                    else if (!predictiveStarted) NativeBridge.back(gpui.generation) else false
                predictiveClaimed = false
                predictiveStarted = false
                Log.i("gpui-box", "System back handled=$handled generation=${gpui.generation}")
                if (!handled) finish()
            }
        }
    }
    fun updateBackRegistration() {
        val enabled = NativeBridge.backEnabled(gpui.generation)
        if (enabled == backRegistered) return
        if (enabled) onBackInvokedDispatcher.registerOnBackInvokedCallback(
            OnBackInvokedDispatcher.PRIORITY_DEFAULT, backCallback)
        else onBackInvokedDispatcher.unregisterOnBackInvokedCallback(backCallback)
        backRegistered = enabled
    }
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        NativeBridge.installStorageContext(this)
        state?.getByteArray("gpui-box.application-state")?.let(NativeBridge::restoreState)
        gpui = GpuiView(this)
        services = GpuiServices(this)
        permissions = GpuiPermissions(this, state)
        val saved = state?.getStringArrayList("gpui-box.pending-urls")
        if (saved != null) for (entry in saved) {
            val separator = entry.indexOf('\n')
            if (separator > 0) pendingUrls.add(entry.substring(0, separator) to entry.substring(separator + 1))
        }
        if (saved == null) enqueueUrl(intent)
        // Keep the first slice's drawable viewport clear of system UI and IME.
        // Full edge-to-edge GPUI WindowInsets animation is a later platform task.
        val content = FrameLayout(this)
        content.addView(gpui, FrameLayout.LayoutParams(-1, -1))
        fun applyInsets(insets: WindowInsets) {
            val view = content
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            val keyboard = insets.getInsets(WindowInsets.Type.ime())
            view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
            NativeBridge.insets(gpui.generation)
        }
        content.setOnApplyWindowInsetsListener { _, insets -> applyInsets(insets); insets }
        content.setWindowInsetsAnimationCallback(object : WindowInsetsAnimation.Callback(DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
            override fun onProgress(insets: WindowInsets, running: MutableList<WindowInsetsAnimation>): WindowInsets {
                applyInsets(insets)
                return insets
            }
        })
        setContentView(content)
        NativeBridge.bind(gpui)
        registerReceiver(lockReceiver, IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_USER_PRESENT)
        }, RECEIVER_NOT_EXPORTED)
        contentResolver.registerContentObserver(
            Settings.Global.getUriFor(Settings.Global.ANIMATOR_DURATION_SCALE), false, motionObserver)
    }
    private fun enqueueUrl(intent: Intent) {
        if (!services.accepts(intent)) return
        if (pendingUrls.size >= 16) {
            Log.w("gpui-box", "Activation queue is full; application must install and drain its activation handler")
            return
        }
        // Stable for Activity recreation/retry. A fresh incoming Intent gets its own ID.
        val id = java.util.UUID.randomUUID().toString()
        pendingUrls.add(id to intent.dataString!!)
    }
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        enqueueUrl(intent)
        deliverPendingUrls()
    }
    override fun onSaveInstanceState(state: Bundle) {
        permissions.save(state)
        state.putByteArray("gpui-box.application-state", NativeBridge.saveState())
        state.putStringArrayList("gpui-box.pending-urls", ArrayList(pendingUrls.map { "${it.first}\n${it.second}" }))
        super.onSaveInstanceState(state)
    }
    fun deliverPendingUrls() {
        while (pendingUrls.isNotEmpty()) {
            val (id, url) = pendingUrls.first
            if (!NativeBridge.openUrlEvent(gpui.generation, id, url)) return
            pendingUrls.removeFirst()
        }
    }
    fun openUrl(url: String) = services.openUrl(url)
    fun canPresentPermissionUi() = resumed && hasWindowFocus() && !isFinishing && !isDestroyed
    override fun onRequestPermissionsResult(code: Int, names: Array<out String>, results: IntArray) {
        super.onRequestPermissionsResult(code, names, results)
        permissions.result(code, names, results)
    }
    override fun onStart() { super.onStart(); started = true; updateHostState() }
    override fun onStop() { started = false; updateHostState(); super.onStop() }
    override fun onResume() { super.onResume(); resumed = true; gpui.setResumed(true); updateHostState(); updateBackRegistration() }
    override fun onPause() { resumed = false; backCallback.onBackCancelled(); gpui.setResumed(false); updateHostState(); super.onPause() }
    override fun onDestroy() {
        unregisterReceiver(lockReceiver)
        contentResolver.unregisterContentObserver(motionObserver)
        if (backRegistered) onBackInvokedDispatcher.unregisterOnBackInvokedCallback(backCallback)
        NativeBridge.unbind(gpui)
        super.onDestroy()
    }
    @Suppress("DEPRECATION")
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        // API 35 retains UI_HIDDEN and BACKGROUND notifications.
        if (level >= ComponentCallbacks2.TRIM_MEMORY_UI_HIDDEN) NativeBridge.memoryWarning()
    }
    override fun onLowMemory() { super.onLowMemory(); NativeBridge.memoryWarning() }
}
