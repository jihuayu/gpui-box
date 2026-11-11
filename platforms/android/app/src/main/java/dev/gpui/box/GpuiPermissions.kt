package dev.gpui.box

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import org.json.JSONObject

/** API 35 permission observations and the single native prompt lane. */
class GpuiPermissions(private val activity: MainActivity, state: Bundle?) {
    private val names = arrayOf(
        arrayOf(Manifest.permission.POST_NOTIFICATIONS),
        arrayOf(Manifest.permission.CAMERA),
        arrayOf(Manifest.permission.RECORD_AUDIO),
        arrayOf(Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION),
        arrayOf(Manifest.permission.READ_CONTACTS)
    )
    private val keys = arrayOf("notifications", "camera", "microphone", "location_when_in_use", "contacts")
    private val history = activity.getSharedPreferences("gpui-box.permissions", 0)
    private val declared = try {
        val config = activity.assets.open("gpui-box-services.json").bufferedReader().use { JSONObject(it.readText()) }
        val permissions = config.optJSONArray("permissions")
        (0 until (permissions?.length() ?: 0)).mapNotNull { index ->
            permissions!!.getJSONObject(index).takeIf { it.optString("purpose").isNotBlank() }?.optString("kind")
        }.toSet()
    } catch (_: java.io.IOException) { emptySet() }
      catch (_: org.json.JSONException) { emptySet() }
    @Suppress("DEPRECATION")
    private val manifest = activity.packageManager.getPackageInfo(activity.packageName, PackageManager.GET_PERMISSIONS)
        .requestedPermissions?.toSet() ?: emptySet()
    private val restoringPrompt = state?.getString("gpui-box.permission.process") == NativeBridge.permissionProcessToken &&
        state.getBoolean("gpui-box.permission.started", false)
    private var pendingId = if (restoringPrompt) state!!.getLong("gpui-box.permission.id", 0) else 0
    private var pendingKind = if (restoringPrompt) state!!.getInt("gpui-box.permission.kind", -1) else -1
    private var promptStarted = restoringPrompt

    fun save(state: Bundle) {
        state.putLong("gpui-box.permission.id", pendingId)
        state.putInt("gpui-box.permission.kind", pendingKind)
        state.putString("gpui-box.permission.process", NativeBridge.permissionProcessToken)
        state.putBoolean("gpui-box.permission.started", promptStarted)
    }

    /** 0 undetermined, 1 granted, 2 denied/retry, 3 denied/settings, 4 limited.
     * Negative values distinguish unsupported/configuration/host/busy/context. */
    fun status(kind: Int): Int {
        NativeBridge.checkMain()
        if (kind !in keys.indices) return -1
        if (keys[kind] !in declared || !manifest.containsAll(names[kind].toList())) return -2
        val granted = names[kind].map { activity.checkSelfPermission(it) == PackageManager.PERMISSION_GRANTED }
        if (granted.all { it }) {
            if (history.getBoolean("${keys[kind]}.blocked", false)) {
                history.edit().putBoolean("${keys[kind]}.blocked", false).apply()
            }
            return 1
        }
        if (kind == 3 && granted[0]) return 4 // Android approximate location.
        if (names[kind].any { activity.packageManager.isPermissionRevokedByPolicy(it, activity.packageName) }) return 5
        if (names[kind].any(activity::shouldShowRequestPermissionRationale)) return 2
        // Remember actual denied callbacks, not merely attempts. This avoids
        // treating an interrupted first prompt as a permanent denial.
        if (history.getBoolean("${keys[kind]}.blocked", false)) return 3
        return if (history.getBoolean("${keys[kind]}.seen", false)) 2 else 0
    }

    fun request(kind: Int, id: Long): Int {
        NativeBridge.checkMain()
        val status = status(kind)
        if (status < 0) return status
        if (!activity.canPresentPermissionUi()) return -5
        if (pendingId != 0L) return -4
        pendingId = id
        pendingKind = kind
        // Always unwind the Rust Entity borrow before invoking Android UI.
        Handler(Looper.getMainLooper()).post {
            if (pendingId != id) return@post
            if (!NativeBridge.permissionPending(id)) { finish(-5); return@post }
            if (!activity.canPresentPermissionUi()) { finish(-5); return@post }
            try { promptStarted = true; activity.requestPermissions(names[kind], REQUEST_CODE) }
            catch (_: RuntimeException) { finish(-3) }
        }
        return 1
    }

    fun result(code: Int, permissions: Array<out String>, results: IntArray): Boolean {
        if (code != REQUEST_CODE) return false
        if (pendingId == 0L || pendingKind !in keys.indices) return true
        val kind = pendingKind
        if (results.isNotEmpty() && permissions.toSet() == names[kind].toSet()) {
            val denied = names[kind].filter { activity.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
            history.edit().putBoolean("${keys[kind]}.seen", true)
                .putBoolean("${keys[kind]}.blocked", denied.isNotEmpty() &&
                    denied.none(activity::shouldShowRequestPermissionRationale)).apply()
        }
        finish(0)
        return true
    }

    private fun finish(error: Int) {
        val id = pendingId
        pendingId = 0
        pendingKind = -1
        promptStarted = false
        if (id != 0L) NativeBridge.permissionResult(id, error)
    }

    fun settings(target: Int): Int {
        NativeBridge.checkMain()
        val intent = when (target) {
            0 -> Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${activity.packageName}"))
            1 -> Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName)
            else -> return -1
        }
        if (!activity.canPresentPermissionUi()) return -5
        return try { activity.startActivity(intent); 1 }
        catch (_: android.content.ActivityNotFoundException) { 0 }
        catch (_: SecurityException) { 0 }
    }

    companion object { private const val REQUEST_CODE = 6031 }
}
