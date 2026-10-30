// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.runtime.saveable.listSaver
import systems.thoughtfull.commonplace.core.MobileNote

internal data class Location(val tab: String, val note: String? = null)

internal data class Navigation(val current: Location, val history: List<Location> = emptyList()) {
  val canGoBack: Boolean get() = current.note != null || history.isNotEmpty()

  fun go(location: Location): Navigation = if (location == current) {
    this
  } else {
    Navigation(location, history + current)
  }

  fun back(exists: (String) -> Boolean): Navigation {
    val index = history.indexOfLast { it.note == null || exists(it.note) }
    return if (index >= 0) {
      Navigation(history[index], history.take(index))
    } else {
      Navigation(Location(current.tab))
    }
  }
}

/** The tab that `note` belongs to. */
internal fun ownTab(note: MobileNote): String = when {
  note.inTrash -> "Trash"
  note.action != null -> "Actions"
  note.journal != null || note.entry != null -> "Journals"
  else -> "Memos"
}

/**
 * The tab to open a note under from `from`: `from` if it lists notes like it, and otherwise `own`,
 * the note's own, so the tabs say what's open, as the Linux app does.
 */
internal fun tabFor(own: String, from: String): String =
  if (from == own || (from == "All notes" && own != "Trash")) from else own

internal val navigationSaver = listSaver<Navigation, String>(
  save = { navigation ->
    (listOf(navigation.current) + navigation.history).flatMap { listOf(it.tab, it.note.orEmpty()) }
  },
  restore = { saved ->
    val locations = saved.chunked(2).map { Location(it[0], it[1].ifEmpty { null }) }
    Navigation(locations.first(), locations.drop(1))
  }
)
