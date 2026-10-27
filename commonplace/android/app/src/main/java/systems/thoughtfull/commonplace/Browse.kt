// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import systems.thoughtfull.commonplace.core.MobileNote

internal enum class BrowseOrder(val label: String) {
  ARCHIVED("Archived"),
  CREATED("Created"),
  DUE("Due"),
  MODIFIED("Modified"),
  SCHEDULED("Scheduled"),
  TRASHED("Trashed");

  fun timestamp(note: MobileNote, zone: ZoneId): Long? = when (this) {
    ARCHIVED -> note.archived

    CREATED -> note.created

    DUE -> note.action?.due?.let {
      LocalDate.parse(it).atStartOfDay(zone).toInstant().toEpochMilli()
    }

    MODIFIED -> note.modified

    SCHEDULED -> note.action?.defer?.let {
      LocalDate.parse(it).atStartOfDay(zone).toInstant().toEpochMilli()
    }

    TRASHED -> note.trashed ?: note.modified
  }
}

internal fun browseNotes(
  notes: List<MobileNote>,
  tab: String,
  query: String,
  order: BrowseOrder,
  ascending: Boolean,
  from: LocalDate?,
  zone: ZoneId = ZoneId.systemDefault()
): List<MobileNote> {
  val eligible = notes.filter { note ->
    note.matches(query) && when (tab) {
      "Memos" ->
        !note.inTrash && note.archived == null && note.action == null &&
          note.journal == null && note.entry == null

      "Trash" -> note.trashed != null

      else -> !note.inTrash
    }
  }
  val (dated, undated) = eligible.partition { order.timestamp(it, zone) != null }
  val matching = dated.filter { note ->
    val day = Instant.ofEpochMilli(order.timestamp(note, zone)!!).atZone(zone).toLocalDate()
    from == null || if (ascending) day >= from else day <= from
  }.sortedBy { order.timestamp(it, zone) }.let { if (ascending) it else it.reversed() }
  return matching + if (from == null && order != BrowseOrder.ARCHIVED) {
    undated.sortedByDescending { it.created }
  } else {
    emptyList()
  }
}

@Composable
internal fun BrowseScreen(
  model: NotebookModel,
  state: NotebookState,
  tab: String,
  query: String,
  open: (String) -> Unit
) {
  val preferences = model.repository.preferences
  val colors = remember(state.notes) {
    state.notes.mapNotNull { note -> note.journal?.let { note.id to it.color } }.toMap()
  }
  val defaultOrder = if (tab == "Trash") BrowseOrder.TRASHED else BrowseOrder.CREATED
  var order by rememberSaveable(tab) {
    val saved = preferences.getString("order.$tab", defaultOrder.label)!!
    mutableStateOf(
      BrowseOrder.entries.find { it.label == saved } ?: if (saved == "Start") {
        BrowseOrder.SCHEDULED
      } else {
        defaultOrder
      }
    )
  }
  var ascending by rememberSaveable(tab) {
    mutableStateOf(preferences.getBoolean("ascending.$tab", false))
  }
  var from by rememberSaveable(tab) {
    mutableStateOf(preferences.getString("from.$tab", "")!!)
  }
  var calendar by remember { mutableStateOf(false) }
  val parsed = runCatching { LocalDate.parse(from) }.getOrNull()
  val notes = remember(state.notes, tab, query, order, ascending, from) {
    browseNotes(state.notes, tab, query, order, ascending, parsed)
  }
  LaunchedEffect(order, ascending, from, tab) {
    preferences.edit().putString("order.$tab", order.label).putString("from.$tab", from)
      .putBoolean("ascending.$tab", ascending).apply()
  }
  // The menus follow the Linux browser's order.
  val orders = when (tab) {
    "All Notes" -> listOf(
      BrowseOrder.CREATED,
      BrowseOrder.MODIFIED,
      BrowseOrder.DUE,
      BrowseOrder.SCHEDULED,
      BrowseOrder.ARCHIVED
    )

    "Trash" -> listOf(BrowseOrder.TRASHED, BrowseOrder.CREATED, BrowseOrder.MODIFIED)

    else -> listOf(BrowseOrder.CREATED, BrowseOrder.MODIFIED)
  }
  Surface(color = MaterialTheme.colorScheme.surfaceContainerLow) {
    FlowRow(
      Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp)
        .testTag("browse-controls"),
      horizontalArrangement = Arrangement.spacedBy(4.dp),
      verticalArrangement = Arrangement.spacedBy(0.dp)
    ) {
      Row(verticalAlignment = Alignment.CenterVertically) {
        MenuControl(
          order,
          orders,
          { it.label },
          Modifier.testTag("browse-order"),
          compact = true
        ) { order = it }
        IconButton(
          onClick = { ascending = !ascending },
          modifier = Modifier.testTag("sort-direction")
        ) {
          AppIcon(
            if (ascending) R.drawable.sort_ascending else R.drawable.sort_descending,
            if (ascending) "Earliest first" else "Latest first"
          )
        }
      }
      Row(verticalAlignment = Alignment.CenterVertically) {
        OutlinedButton(
          onClick = { calendar = true },
          shape = RoundedCornerShape(6.dp),
          contentPadding = PaddingValues(horizontal = 8.dp),
          modifier = Modifier.testTag("browse-date")
        ) {
          Text(parsed?.format(DateTimeFormatter.ofPattern("MMM d, yyyy")) ?: "Any day")
          AppIcon(R.drawable.dropdown)
        }
        TextButton(
          enabled = order != defaultOrder || ascending || parsed != null,
          onClick = {
            order = defaultOrder
            from = ""
            ascending = false
          },
          contentPadding = PaddingValues(horizontal = 8.dp)
        ) { Text("Reset") }
      }
    }
  }
  HorizontalDivider()
  LazyColumn(Modifier.fillMaxSize()) {
    if (notes.isEmpty()) {
      item {
        EmptyNotes(query)
      }
    }
    items(notes, key = { it.id }) { note ->
      NoteRow(
        note,
        open,
        complete = if (note.action != null && !note.inTrash && !note.readOnly) {
          { toggleAction(model, note) }
        } else {
          null
        },
        stamp = order.label.takeUnless {
          order == BrowseOrder.DUE || order == BrowseOrder.SCHEDULED
        },
        entryColor = colors[note.entry?.journal] ?: "blue"
      )
    }
  }
  if (calendar) {
    CalendarDialog(parsed, { calendar = false }, "Any day") {
      from = it?.toString().orEmpty()
      calendar = false
    }
  }
}
