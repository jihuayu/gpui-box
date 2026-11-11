package dev.gpui.box

import android.app.Activity
import android.content.Intent
import android.net.Uri
import org.json.JSONObject

/** Generated declarations describe application intent. Runtime support is probed separately. */
class GpuiServices(private val activity: Activity) {
    private val configuration = try {
        activity.assets.open("gpui-box-services.json").bufferedReader().use { JSONObject(it.readText()) }
    } catch (_: java.io.FileNotFoundException) { JSONObject() }

    fun accepts(intent: Intent): Boolean {
        if (intent.action != Intent.ACTION_VIEW) return false
        val uri = intent.data ?: return false
        val scheme = uri.scheme ?: return false
        val schemes = configuration.optJSONArray("url_schemes") ?: return false
        return (0 until schemes.length()).any { schemes.getString(it) == scheme } &&
            uri.toString().toByteArray(Charsets.UTF_8).size <= 8192 && uri.toString().none { it.isISOControl() } && uri.userInfo == null
    }
    fun openUrl(value: String): Boolean {
        if (value.length > 8192 || value.any { it.isISOControl() }) return false
        val uri = Uri.parse(value)
        if (uri.userInfo != null || uri.scheme !in listOf("http", "https", "mailto", "tel")) return false
        if (uri.scheme in listOf("http", "https") && uri.host.isNullOrEmpty()) return false
        return try {
            activity.startActivity(Intent(Intent.ACTION_VIEW, uri).addCategory(Intent.CATEGORY_BROWSABLE))
            true
        } catch (_: android.content.ActivityNotFoundException) { false }
          catch (_: SecurityException) { false }
    }
}
