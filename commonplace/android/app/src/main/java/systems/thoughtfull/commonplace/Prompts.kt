// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.Manifest
import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import systems.thoughtfull.commonplace.core.Handled
import systems.thoughtfull.commonplace.core.MobilePrompt

internal object PromptScheduler {
  private const val CHANNEL = "journals"
  private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

  fun schedule(context: Context) {
    scope.launch {
      runCatching { check(context.applicationContext) }
        .onFailure { Log.e("Commonplace", "Couldn't schedule journal prompts", it) }
    }
  }

  suspend fun check(context: Context) {
    val repository = (context.applicationContext as CommonplaceApplication).repository
    val manager = context.getSystemService(NotificationManager::class.java)
    manager.createNotificationChannel(
      NotificationChannel(
        CHANNEL,
        "Journal prompts",
        NotificationManager.IMPORTANCE_DEFAULT
      )
    )
    val preferences = repository.preferences
    val enabled = preferences.all.filter { it.key.startsWith("prompts.") && it.value == true }
      .keys.map { it.removePrefix("prompts.") }
    val zone = ZoneId.systemDefault()
    val today = LocalDate.now(zone)
    val now = System.currentTimeMillis()
    val time = LocalTime.now(zone)
    fun local(date: LocalDate, minute: Int) = date.atTime(minute / 60, minute % 60)
      .atZone(zone).toInstant().toEpochMilli()
    var next = today.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli()
    val due = repository.use { core ->
      for (days in 0L..7L) {
        val date = today.plusDays(days)
        core.scheduledPrompts(enabled, date.toString()).forEach { prompt ->
          val at = when (val handled = prompt.handled) {
            is Handled.Snoozed -> handled.until
            null -> local(date, prompt.minute.toInt())
            else -> Long.MAX_VALUE
          }
          if (at > now) next = minOf(next, at)
        }
      }
      core.duePrompts(
        enabled,
        today.toString(),
        (time.hour * 60 + time.minute).toUShort(),
        now,
        (0..1439).map { local(today, it) }
      )
    }
    val visible = due.filter { it.journal != repository.answeringJournal }
    val active = visible.associateBy { it.journal }
    manager.activeNotifications.filter { it.notification.channelId == CHANNEL }.forEach {
      if (it.tag !in active) manager.cancel(it.tag, it.id)
    }
    if (NotificationManagerCompat.from(context).areNotificationsEnabled()) {
      visible.forEach { prompt ->
        val existing = manager.activeNotifications.any {
          it.tag == prompt.journal &&
            it.notification.extras.getString("prompt") == prompt.key
        }
        if (!existing) notify(context, manager, prompt, local(today, prompt.minute.toInt()))
      }
    }
    val alarm = context.getSystemService(AlarmManager::class.java)
    val operation = PendingIntent.getBroadcast(
      context,
      0,
      Intent(context, PromptReceiver::class.java).setAction("check"),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    if (enabled.isEmpty()) {
      alarm.cancel(operation)
    } else {
      // Android may defer inexact alarms in Doze; notifications still respect do-not-disturb.
      alarm.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, maxOf(next, now + 1000), operation)
    }
  }

  private fun notify(
    context: Context,
    manager: NotificationManager,
    prompt: MobilePrompt,
    prompted: Long
  ) {
    if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(
        context,
        Manifest.permission.POST_NOTIFICATIONS
      ) != PackageManager.PERMISSION_GRANTED
    ) {
      return
    }
    val answer = PendingIntent.getActivity(
      context,
      0,
      Intent(context, MainActivity::class.java)
        .setAction("answer").setData(Uri.parse("commonplace:prompt/${prompt.key}"))
        .putExtra("journal", prompt.journal).putExtra("prompt", prompt.key)
        .putExtra("prompted", prompted),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    fun action(name: String) = PendingIntent.getBroadcast(
      context,
      0,
      Intent(context, PromptReceiver::class.java).setAction(name)
        .setData(Uri.parse("commonplace:prompt/${prompt.journal}/${prompt.key}/$name"))
        .putExtra("journal", prompt.journal).putExtra("prompt", prompt.key),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    )
    val extras = android.os.Bundle().apply { putString("prompt", prompt.key) }
    val notification = NotificationCompat.Builder(context, CHANNEL)
      .addAction(0, "15 minutes", action("snooze15"))
      .addAction(0, "1 hour", action("snooze60"))
      .addAction(0, "Skip", action("skip"))
      .addExtras(extras)
      .setAutoCancel(true)
      .setContentIntent(answer)
      .setContentText("Tap to write a journal entry")
      .setContentTitle(prompt.title)
      .setOnlyAlertOnce(true)
      .setSmallIcon(R.drawable.commonplace)
      .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
      .build()
    manager.notify(prompt.journal, 0, notification)
  }
}

class PromptReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val pending = goAsync()
    CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
      try {
        val repository = (context.applicationContext as CommonplaceApplication).repository
        val journal = intent.getStringExtra("journal")
        val prompt = intent.getStringExtra("prompt")
        if (journal != null && prompt != null) {
          val handled = when (intent.action) {
            "skip" -> Handled.Skipped
            "snooze15" -> Handled.Snoozed(System.currentTimeMillis() + 15 * 60_000)
            "snooze60" -> Handled.Snoozed(System.currentTimeMillis() + 60 * 60_000)
            else -> null
          }
          if (handled != null) {
            repository.use {
              it.handlePrompt(journal, prompt, handled, LocalDate.now().toString())
            }
          }
          context.getSystemService(NotificationManager::class.java).cancel(journal, 0)
        }
        PromptScheduler.check(context)
      } catch (error: Exception) {
        Log.e("Commonplace", "Couldn't handle a journal prompt", error)
      } finally {
        pending.finish()
      }
    }
  }
}

class BootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action in listOf(
        Intent.ACTION_BOOT_COMPLETED,
        Intent.ACTION_MY_PACKAGE_REPLACED,
        Intent.ACTION_TIME_CHANGED,
        Intent.ACTION_TIMEZONE_CHANGED
      )
    ) {
      val pending = goAsync()
      CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
        try {
          publishCaptureNotification(context)
          PromptScheduler.check(context)
        } catch (
          error: Exception
        ) {
          Log.e("Commonplace", "Couldn't restore journal prompts", error)
        } finally {
          pending.finish()
        }
      }
    }
  }
}
