package dev.gpui.box

import android.os.Looper
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.rule.ActivityTestRule
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/** Executes the example's real Rust AppData operations on the test worker. */
@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class StorageHostTest {
    @get:Rule val activityRule = ActivityTestRule(MainActivity::class.java)

    private external fun nativeStorageProbe(): String

    @Test fun privateRootsReopenSettingsAndCachePreservesDurableContent() {
        assertNotEquals(Looper.getMainLooper(), Looper.myLooper())
        val first = JSONObject(nativeStorageProbe())
        assertTrue(first.toString(), first.getBoolean("success"))
        val second = JSONObject(nativeStorageProbe())
        assertTrue(second.toString(), second.getBoolean("success"))
        assertEquals(first.getLong("visits") + 1, second.getLong("visits"))
        assertEquals(second.getLong("visits"), second.getJSONObject("reloaded").getLong("visits"))
        assertEquals("Private 你好🙂", second.getJSONObject("reloaded").getString("text"))
        assertEquals(1, second.getInt("cleared"))
        assertTrue(second.getBoolean("sandboxed"))
        assertTrue(second.getBoolean("export_removed"))
        val context = activityRule.activity.applicationContext
        for (name in listOf("control", "config", "data", "logs", "temporary")) {
            assertEquals(File(context.noBackupFilesDir, "gpui-box/$name").canonicalPath,
                File(second.getString(name)).canonicalPath)
            assertTrue(File(second.getString(name)).isDirectory)
        }
        assertEquals(File(context.cacheDir, "gpui-box").canonicalPath,
            File(second.getString("cache")).canonicalPath)
        assertFalse(context.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_ALLOW_BACKUP != 0)
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val rejected = JSONObject(nativeStorageProbe())
            assertFalse("Rust discovery must reject the UI thread", rejected.getBoolean("success"))
            try {
                NativeBridge.storageDirectories()
                fail("Storage discovery must reject the UI thread")
            } catch (_: IllegalStateException) { }
        }
        val recovered = JSONObject(nativeStorageProbe())
        assertTrue("Rejected UI discovery must leave no pending JNI exception", recovered.getBoolean("success"))
        assertEquals(second.getLong("visits") + 1, recovered.getLong("visits"))
    }
}
