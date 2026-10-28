// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.app.Application
import android.content.Context
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.File
import java.time.LocalDate
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import systems.thoughtfull.commonplace.core.ActionDates
import systems.thoughtfull.commonplace.core.ActionView
import systems.thoughtfull.commonplace.core.MobileAction
import systems.thoughtfull.commonplace.core.MobileException
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.MobileNotebook
import systems.thoughtfull.commonplace.core.MobileSpace

class CommonplaceApplication : Application() {
  val repository by lazy { NotebookRepository(this) }
}

class NotebookRepository(context: Context) {
  @Volatile
  var answeringJournal: String? = null
  private val directory = File(context.filesDir, "commonplace")

  @OptIn(ExperimentalCoroutinesApi::class)
  private val dispatcher = Dispatchers.IO.limitedParallelism(1)
  private var notebook: MobileNotebook? = null

  // Body saves outlive the ViewModel that started them, so closing the app keeps the last edits.
  val saves = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
  val preferences = context.getSharedPreferences("device", Context.MODE_PRIVATE)
  val sync = DeviceSync(context, this)

  suspend fun <T> use(block: (MobileNotebook) -> T): T = withContext(dispatcher) {
    val core = notebook ?: MobileNotebook.open(directory.path).also { notebook = it }
    block(core)
  }

  fun name(key: String) = preferences.getString("name.$key", null) ?: "Unnamed"

  fun named(key: String) = preferences.contains("name.$key")

  fun prompts(id: String) = preferences.getBoolean("prompts.$id", false)

  fun setPrompts(id: String, enabled: Boolean) {
    preferences.edit().putBoolean("prompts.$id", enabled).apply()
  }
}

data class NotebookState(
  val actionDates: Map<String, ActionDates> = emptyMap(),
  val actionPaths: Map<String, List<String>> = emptyMap(),
  val actions: Map<ActionView, List<MobileAction>> = emptyMap(),
  val busy: Boolean = false,
  val error: String? = null,
  val journals: List<MobileNote> = emptyList(),
  val loaded: Boolean = false,
  val notes: List<MobileNote> = emptyList(),
  val revision: Long = 0,
  val spaces: List<MobileSpace> = emptyList()
)

class NotebookModel(application: Application) : AndroidViewModel(application) {
  val repository = (application as CommonplaceApplication).repository
  private val drafts = mutableMapOf<String, Pair<Long, String>>()
  private val failedSaves = mutableSetOf<String>()
  private var nextSave = 0L
  private val pendingSaves = mutableMapOf<String, Pair<Job, (MobileNote?) -> Unit>>()
  private val mutable = MutableStateFlow(NotebookState())
  val state = mutable.asStateFlow()

  init {
    refresh()
    viewModelScope.launch { repository.sync.merges.drop(1).collect { refresh() } }
  }

  fun clearError() {
    mutable.value = mutable.value.copy(error = null)
  }

  fun report(error: Throwable) {
    if (error is CancellationException) throw error
    val message = (error as? MobileException.Failed)?.reason
      ?: error.message ?: "Couldn't open the library"
    mutable.value = mutable.value.copy(error = message)
  }

  fun refresh() {
    perform { }
  }

  fun loadTestLibrary() {
    perform { core ->
      if (repository.preferences.getBoolean("samples.loaded", false)) return@perform
      val names = core.loadTestLibrary(LocalDate.now().toString())
      val settings = repository.preferences.edit().putBoolean("samples.loaded", true)
      names.forEach { (key, name) ->
        settings.putString("name.$key", name).putBoolean("shown.$key", true)
      }
      settings.apply()
    }
  }

  fun perform(complete: (Boolean) -> Unit = {}, block: (MobileNotebook) -> Unit) {
    // The library runs one operation at a time in order, so the writes go first.
    flushSaves()
    viewModelScope.launch {
      mutable.value = mutable.value.copy(busy = true)
      try {
        val snapshot = repository.use { core ->
          block(core)
          val spaces = core.spaces()
          spaces.forEach { space ->
            core.showSpace(
              space.key,
              repository.preferences.getBoolean("shown.${space.key}", true)
            )
          }
          val today = LocalDate.now().toString()
          NotebookState(
            actionDates = core.allActionDates(today),
            actionPaths = core.actionPaths(),
            actions = ActionView.entries.associateWith { core.actions(it, today) },
            journals = core.journals(),
            loaded = true,
            notes = core.notes(""),
            spaces = spaces
          )
        }
        val previous = mutable.value
        mutable.value = snapshot.copy(revision = previous.revision + 1)
        if (!previous.loaded || snapshot.journals != previous.journals) {
          PromptScheduler.schedule(getApplication())
        }
        complete(true)
      } catch (cancelled: CancellationException) {
        throw cancelled
      } catch (error: Exception) {
        mutable.value = mutable.value.copy(busy = false)
        report(error)
        complete(false)
      }
    }
  }

  // Text that failed to save stays in preferences too, so closing the app doesn't lose it.
  fun draft(id: String) = drafts[id]?.second
    ?: repository.preferences.getString(UNSAVED + id, null)

  fun saveFailed(id: String) =
    id in failedSaves || (id !in drafts && repository.preferences.contains(UNSAVED + id))

  /** Saves `text` once typing pauses, so each keystroke doesn't write the library. */
  fun saveBody(id: String, text: String, complete: (MobileNote?) -> Unit) {
    val sequence = ++nextSave
    drafts[id] = sequence to text
    failedSaves.remove(id)
    pendingSaves.remove(id)?.first?.cancel()
    val job = repository.saves.launch {
      delay(SAVE_DELAY_MILLIS)
      pendingSaves.remove(id)
      write(id, sequence, text, complete)
    }
    pendingSaves[id] = job to complete
  }

  /** Saves every waiting draft now. */
  fun flushSaves() {
    for ((id, pending) in pendingSaves.toMap()) {
      pendingSaves.remove(id)
      pending.first.cancel()
      val (sequence, text) = drafts[id] ?: continue
      repository.saves.launch { write(id, sequence, text, pending.second) }
    }
  }

  private suspend fun write(
    id: String,
    sequence: Long,
    text: String,
    complete: (MobileNote?) -> Unit
  ) {
    try {
      var merged = false
      val note = repository.use {
        merged = it.setBody(id, text)
        it.note(id)
      }
      // The editor shows its own text, which lacks the changes that merged into it.
      if (merged) refresh()
      if (drafts[id]?.first == sequence) {
        drafts.remove(id)
        failedSaves.remove(id)
        repository.preferences.edit().remove(UNSAVED + id).apply()
        complete(note)
      }
    } catch (cancelled: CancellationException) {
      throw cancelled
    } catch (error: Exception) {
      if (drafts[id]?.first == sequence) {
        failedSaves.add(id)
        repository.preferences.edit().putString(UNSAVED + id, text).apply()
        complete(null)
        report(error)
      }
    }
  }

  suspend fun <T> read(block: (MobileNotebook) -> T): T? = try {
    repository.use(block)
  } catch (cancelled: CancellationException) {
    throw cancelled
  } catch (error: Exception) {
    report(error)
    null
  }

  private companion object {
    const val SAVE_DELAY_MILLIS = 500L
    const val UNSAVED = "unsaved."
  }
}
