// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.StatusBarManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.drawable.Icon
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat

internal const val CAPTURE_ACTION = "systems.thoughtfull.commonplace.CAPTURE"

internal fun captureIntent(context: Context) = Intent(context, CaptureActivity::class.java)
  .setAction(CAPTURE_ACTION)
  .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)

private fun quickCaptureShortcut(context: Context) = ShortcutInfo.Builder(context, "quick-capture")
  .setIcon(Icon.createWithResource(context, R.drawable.quick_capture))
  .setIntent(captureIntent(context))
  .setLongLabel("Capture in Commonplace")
  .setShortLabel("Quick capture")
  .build()

internal fun publishQuickCapture(context: Context) {
  val shortcuts = context.getSystemService(ShortcutManager::class.java)
  val shortcut = quickCaptureShortcut(context)
  shortcuts.addDynamicShortcuts(listOf(shortcut))
  shortcuts.updateShortcuts(listOf(shortcut))
  publishCaptureNotification(context)
}

internal fun pinQuickCapture(context: Context): Boolean {
  val shortcuts = context.getSystemService(ShortcutManager::class.java)
  if (!shortcuts.isRequestPinShortcutSupported) return false
  return shortcuts.requestPinShortcut(quickCaptureShortcut(context), null)
}

internal fun publishCaptureNotification(context: Context) {
  val manager = context.getSystemService(NotificationManager::class.java)
  val enabled = context.getSharedPreferences("device", Context.MODE_PRIVATE)
    .getBoolean("capture.notification", false)
  if (!enabled || !NotificationManagerCompat.from(context).areNotificationsEnabled()) {
    manager.cancel(2000)
    return
  }
  if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(
      context,
      Manifest.permission.POST_NOTIFICATIONS
    ) != PackageManager.PERMISSION_GRANTED
  ) {
    return
  }
  manager.createNotificationChannel(
    NotificationChannel("quick-capture", "Quick capture", NotificationManager.IMPORTANCE_LOW)
  )
  val intent = PendingIntent.getActivity(
    context,
    2000,
    captureIntent(context),
    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
  )
  val notification = NotificationCompat.Builder(context, "quick-capture")
    .setSmallIcon(R.drawable.add).setContentTitle("Quick capture")
    .setContentText("Capture a thought without leaving your task")
    .setContentIntent(intent).setOngoing(true).setOnlyAlertOnce(true).setSilent(true).build()
  runCatching { manager.notify(2000, notification) }
}

internal fun addCaptureTile(context: Context, failed: (String) -> Unit) {
  if (android.os.Build.VERSION.SDK_INT >= 33) {
    context.getSystemService(StatusBarManager::class.java).requestAddTileService(
      ComponentName(context, CaptureTileService::class.java),
      context.getString(R.string.quick_capture),
      Icon.createWithResource(context, R.drawable.add),
      context.mainExecutor
    ) { result ->
      if (result !in listOf(
          StatusBarManager.TILE_ADD_REQUEST_RESULT_TILE_ADDED,
          StatusBarManager.TILE_ADD_REQUEST_RESULT_TILE_ALREADY_ADDED,
          StatusBarManager.TILE_ADD_REQUEST_RESULT_TILE_NOT_ADDED
        )
      ) {
        failed("Couldn't add the tile. Add Quick capture from Quick Settings → Edit.")
      }
    }
  } else {
    failed("Add Quick capture from Quick Settings → Edit.")
  }
}
