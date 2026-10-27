// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.content.Intent
import android.content.pm.ShortcutManager
import android.os.ParcelFileDescriptor
import android.provider.Settings
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class QuickCaptureTest {
  @get:Rule
  val compose = createAndroidComposeRule<MainActivity>()

  @Test
  fun externalCaptureSavesAndReturnsToThePreviousTask() {
    val context = compose.activity
    val original = Intent(context.intent)
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val instrumentation = InstrumentationRegistry.getInstrumentation()
    val repository = (context.applicationContext as CommonplaceApplication).repository
    val space = runBlocking { repository.use { it.createSpace() } }
    val previous = repository.preferences.getString("capture.space", null)
    val previousKind = repository.preferences.getString("capture.kind", null)
    repository.preferences.edit().putString("capture.space", space)
      .putString("capture.kind", "Memo").commit()
    fun activityStack(): String = ParcelFileDescriptor.AutoCloseInputStream(
      instrumentation.uiAutomation.executeShellCommand("dumpsys activity activities")
    ).bufferedReader().use { it.readText() }
    try {
      compose.waitUntil(10_000) {
        context.getSystemService(ShortcutManager::class.java)
          .dynamicShortcuts.any { it.id == "quick-capture" }
      }
      val shortcut = context.getSystemService(ShortcutManager::class.java)
        .dynamicShortcuts.single { it.id == "quick-capture" }
      assertEquals(CaptureActivity::class.java.name, shortcut.intent!!.component!!.className)
      assertEquals(CAPTURE_ACTION, shortcut.intent!!.action)
      assertTrue(
        context.getSystemService(ShortcutManager::class.java).isRequestPinShortcutSupported
      )
      context.startActivity(
        Intent(Settings.ACTION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      )
      compose.waitUntil(10_000) {
        activityStack().lineSequence().any { "topResumedActivity" in it && "settings" in it }
      }
      context.startActivity(captureIntent(context))
      compose.waitUntil(10_000) {
        runCatching { compose.onNodeWithText("Write in Markdown").assertExists() }.isSuccess
      }
      compose.onNodeWithText("Write in Markdown").performTextInput("A thought from another app")
      compose.onNodeWithText("Save").performClick()
      compose.waitUntil(10_000) {
        activityStack().lineSequence().any { "topResumedActivity" in it && "settings" in it }
      }
      instrumentation.uiAutomation.grantRuntimePermission(
        context.packageName,
        Manifest.permission.POST_NOTIFICATIONS
      )
      repository.preferences.edit().putBoolean("capture.notification", true).commit()
      compose.waitUntil(10_000) {
        publishCaptureNotification(context)
        context.getSystemService(NotificationManager::class.java)
          .activeNotifications.any { it.id == 2000 }
      }
      val notification = context.getSystemService(NotificationManager::class.java)
        .activeNotifications.single { it.id == 2000 }.notification
      assertTrue(notification.flags and Notification.FLAG_ONGOING_EVENT != 0)
      notification.contentIntent.send()
      compose.waitUntil(10_000) {
        runCatching { compose.onNodeWithText("Write in Markdown").assertExists() }.isSuccess
      }
      compose.onNodeWithText("Cancel").performClick()
      compose.waitUntil(10_000) {
        activityStack().lineSequence().any { "topResumedActivity" in it && "settings" in it }
      }
      assertEquals(
        1,
        runBlocking {
          repository.use { core ->
            core.notes("").count {
              it.space == space && it.body == "A thought from another app"
            }
          }
        }
      )
    } finally {
      repository.preferences.edit().putString("capture.space", previous)
        .putString("capture.kind", previousKind)
        .putBoolean("capture.notification", false).commit()
      publishCaptureNotification(context)
      runBlocking { repository.use { it.removeSpace(space) } }
      context.startActivity(
        Intent(original)
          .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
      )
    }
  }

  @Test
  fun quickSettingsTileOpensCaptureAndReturnsToThePreviousApp() {
    val context = compose.activity
    val original = Intent(context.intent)
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val instrumentation = InstrumentationRegistry.getInstrumentation()
    fun shell(command: String): String = ParcelFileDescriptor.AutoCloseInputStream(
      instrumentation.uiAutomation.executeShellCommand(command)
    ).bufferedReader().use { it.readText() }
    val component = "${context.packageName}/${CaptureTileService::class.java.name}"
    try {
      context.startActivity(
        Intent(Settings.ACTION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      )
      compose.waitUntil(10_000) {
        shell("dumpsys activity activities").lineSequence().any {
          "topResumedActivity" in it && "settings" in it
        }
      }
      shell("cmd statusbar add-tile $component")
      shell("cmd statusbar expand-settings")
      android.os.SystemClock.sleep(1000)
      shell("cmd statusbar click-tile $component")
      compose.waitUntil(15_000) {
        runCatching { compose.onNodeWithText("Write in Markdown").assertExists() }.isSuccess
      }
      compose.onNodeWithText("Cancel").performClick()
      compose.waitUntil(10_000) {
        shell("dumpsys activity activities").lineSequence().any {
          "topResumedActivity" in it && "settings" in it
        }
      }
    } finally {
      shell("cmd statusbar remove-tile $component")
      shell("cmd statusbar collapse")
      context.startActivity(
        Intent(original)
          .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
      )
    }
  }
}
