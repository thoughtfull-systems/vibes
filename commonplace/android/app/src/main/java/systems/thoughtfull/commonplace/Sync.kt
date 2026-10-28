// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.content.Context
import android.os.Build
import android.provider.Settings
import android.util.Log
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.io.File
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import systems.thoughtfull.commonplace.core.MobileException
import systems.thoughtfull.commonplace.core.MobileNotebook
import systems.thoughtfull.commonplace.core.SyncListener

/** Runs the core's syncer while the app is in the foreground or a sync job runs. */
class DeviceSync(context: Context, private val repository: NotebookRepository) {
  private val context = context.applicationContext

  // Android never backs up this directory, so the device's key stays on the device.
  private val directory = File(context.noBackupFilesDir, "sync")
  private val gate = Mutex()
  private var holds = 0
  private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
  private var stopping: Job? = null
  private val mutableError = MutableStateFlow<String?>(null)
  private val mutableMerges = MutableStateFlow(0L)
  private val mutableRevision = MutableStateFlow(0L)

  /** Why sync last failed to start, if it did. */
  val error = mutableError.asStateFlow()

  /** Counts merges from other devices. */
  val merges = mutableMerges.asStateFlow()

  /** Counts changes to the configuration and to each device's status. */
  val revision = mutableRevision.asStateFlow()

  private val listener = object : SyncListener {
    override fun syncChanged() = mutableRevision.update { it + 1 }

    override fun librariesChanged() = mutableMerges.update { it + 1 }
  }

  /** Keeps sync running until a matching [release]. */
  fun hold() {
    synchronized(this) {
      holds++
      stopping?.cancel()
      stopping = null
    }
    scope.launch {
      try {
        start()
      } catch (cancelled: CancellationException) {
        throw cancelled
      } catch (error: Exception) {
        Log.e("Commonplace", "Couldn't start sync", error)
      }
    }
  }

  fun release() {
    synchronized(this) {
      holds--
      if (holds > 0) return
      // Leaving the app briefly, or rotating it, shouldn't drop connections or a pending sync.
      stopping = scope.launch {
        delay(STOP_DELAY_MILLIS)
        gate.withLock {
          if (synchronized(this@DeviceSync) { holds } == 0) repository.use { it }.stopSync()
        }
      }
    }
  }

  /** Starts sync unless it runs, and returns the notebook that it syncs. */
  suspend fun start(): MobileNotebook = gate.withLock { startLocked() }

  private suspend fun startLocked(): MobileNotebook {
    val core = repository.use { it }
    withContext(Dispatchers.IO) {
      try {
        core.startSync(directory.path, listener)
        mutableError.value = null
      } catch (error: MobileException) {
        mutableError.value = (error as? MobileException.Failed)?.reason ?: error.message
        throw error
      }
      if (core.syncName().isBlank()) core.setSyncName(deviceName())
      SyncWork.schedule(context, core.syncConfig().automatic)
    }
    mutableRevision.update { it + 1 }
    return core
  }

  /** The name that the owner gave the phone in Android's settings, or its model. */
  private fun deviceName(): String =
    Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)
      ?.takeIf { it.isNotBlank() } ?: Build.MODEL

  /** Starts sync again, for a change that takes effect only then. */
  suspend fun restart() {
    gate.withLock {
      withContext(Dispatchers.IO) { repository.use { it }.stopSync() }
      startLocked()
    }
  }

  /** Stops announcing this device after its Sync screen closes, which can outlive the screen. */
  fun stopNearby() {
    scope.launch {
      gate.withLock {
        runCatching {
          val core = repository.use { it }
          if (core.syncRunning()) {
            core.stopNearby()
            core.clearOutgoingPairing()
          }
        }
      }
    }
  }

  /** Calls the syncer on a thread where it may block. */
  suspend fun <T> call(block: (MobileNotebook) -> T): T {
    val core = start()
    return withContext(Dispatchers.IO) { block(core) }
  }

  private companion object {
    const val STOP_DELAY_MILLIS = 60_000L
  }
}

internal object SyncWork {
  private const val NAME = "sync"

  fun schedule(context: Context, automatic: Boolean) {
    val work = WorkManager.getInstance(context)
    if (automatic) {
      val request = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
        .setConstraints(
          Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
        )
        .build()
      work.enqueueUniquePeriodicWork(NAME, ExistingPeriodicWorkPolicy.KEEP, request)
    } else {
      work.cancelUniqueWork(NAME)
    }
  }
}

class SyncWorker(context: Context, parameters: WorkerParameters) :
  CoroutineWorker(context, parameters) {
  override suspend fun doWork(): Result {
    val sync = (applicationContext as CommonplaceApplication).repository.sync
    sync.hold()
    return try {
      val core = sync.start()
      if (core.syncConfig().automatic) {
        core.syncNow()
        // WorkManager stops a job after ten minutes.
        withTimeoutOrNull(WAIT_MILLIS) {
          while (core.syncing()) delay(POLL_MILLIS)
        }
      }
      Result.success()
    } catch (cancelled: CancellationException) {
      throw cancelled
    } catch (error: Exception) {
      Log.e("Commonplace", "Couldn't sync in the background", error)
      Result.retry()
    } finally {
      sync.release()
    }
  }

  private companion object {
    const val POLL_MILLIS = 1_000L
    const val WAIT_MILLIS = 8 * 60_000L
  }
}
