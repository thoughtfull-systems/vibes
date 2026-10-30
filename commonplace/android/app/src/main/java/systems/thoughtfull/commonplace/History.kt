// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import java.time.LocalDate
import systems.thoughtfull.commonplace.core.Occurrence
import systems.thoughtfull.commonplace.core.OccurrenceState
import systems.thoughtfull.commonplace.core.OccurrenceStatus

/** How many past occurrences the history shows at first, and how many more Show earlier adds. */
private const val PAST = 10

/** How many occurrences after the current one the history shows. */
private const val UPCOMING = 5

/**
 * The history of a recurring action, as the Linux app's History dialog shows it: its upcoming and
 * past occurrences, each with a menu that closes, skips, moves, or reopens it, or writes a note.
 */
@Composable
internal fun HistoryDialog(
  model: NotebookModel,
  id: String,
  dismiss: () -> Unit,
  open: (String) -> Unit
) {
  val state by model.state.collectAsState()
  var past by remember { mutableIntStateOf(PAST) }
  var occurrences by remember { mutableStateOf<List<Occurrence>>(emptyList()) }
  var moving by remember { mutableStateOf<Occurrence?>(null) }
  val today = LocalDate.now().toString()
  LaunchedEffect(state.revision, past) {
    occurrences =
      model.read { it.occurrences(id, today, past.toUInt(), UPCOMING.toUInt()) }.orEmpty()
  }
  val (upcoming, earlier) = occurrences.partition {
    it.status == OccurrenceStatus.CURRENT || it.status == OccurrenceStatus.UPCOMING
  }
  Dialog(dismiss, DialogProperties(usePlatformDefaultWidth = false)) {
    Scaffold(
      topBar = {
        TopAppBar(
          title = { Text("History") },
          navigationIcon = { IconButton(onClick = dismiss) { AppIcon(R.drawable.back, "Back") } }
        )
      }
    ) { padding ->
      Column(
        Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
          .padding(horizontal = 16.dp, vertical = 8.dp)
          .testTag("history"),
        verticalArrangement = Arrangement.spacedBy(4.dp)
      ) {
        listOf("Upcoming" to upcoming, "Past" to earlier).filter { it.second.isNotEmpty() }
          .forEach { (title, items) ->
            Text(title, Modifier.padding(top = 8.dp), style = MaterialTheme.typography.titleSmall)
            items.forEach { occurrence ->
              OccurrenceRow(occurrence) { choice ->
                when (choice) {
                  "done" -> model.perform {
                    it.handleOccurrence(id, occurrence.date, OccurrenceState.DONE, today)
                  }

                  "skip" -> model.perform {
                    it.handleOccurrence(id, occurrence.date, OccurrenceState.SKIPPED, today)
                  }

                  "reopen" -> model.perform {
                    it.handleOccurrence(id, occurrence.date, null, today)
                  }

                  "move" -> moving = occurrence

                  "put-back" -> model.perform { it.moveOccurrence(id, occurrence.date, null) }

                  "open-note" -> occurrence.note?.let {
                    dismiss()
                    open(it)
                  }

                  "write-note" -> {
                    var note: String? = null
                    model.perform({ written ->
                      note?.takeIf { written }?.let {
                        dismiss()
                        open(it)
                      }
                    }) { core -> note = core.writeOccurrenceNote(id, occurrence.date) }
                  }
                }
              }
            }
          }
        if (earlier.size >= past) {
          TextButton(onClick = { past += PAST }) { Text("Show earlier") }
        }
        if (occurrences.isEmpty()) {
          Text("No occurrences", color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
      }
    }
  }
  moving?.let { occurrence ->
    CalendarDialog(LocalDate.parse(occurrence.on), { moving = null }, clearLabel = "Put back") {
      val to = it?.toString()
      model.perform { core -> core.moveOccurrence(id, occurrence.date, to) }
      moving = null
    }
  }
}

/** A row for one occurrence, with its menu, as the Linux History dialog writes it. */
@Composable
private fun OccurrenceRow(occurrence: Occurrence, choose: (String) -> Unit) {
  var menu by remember { mutableStateOf(false) }
  val (icon, word) = when (occurrence.status) {
    OccurrenceStatus.CURRENT -> R.drawable.repeat to "this time"

    OccurrenceStatus.UPCOMING ->
      R.drawable.plan to "waiting".takeIf { occurrence.on <= LocalDate.now().toString() }

    OccurrenceStatus.DONE ->
      R.drawable.actions to
        occurrence.handled?.let { "done ${formatDate(it.day)}" }

    OccurrenceStatus.SKIPPED -> R.drawable.remove to "skipped"

    OccurrenceStatus.MISSED -> R.drawable.clear_search to "missed"
  }
  val details = listOfNotNull(
    word,
    "moved from ${formatDate(occurrence.date)}".takeIf { occurrence.on != occurrence.date },
    "has a note".takeIf { occurrence.note != null }
  ).joinToString(" · ")
  Row(
    Modifier.fillMaxWidth().padding(vertical = 4.dp),
    verticalAlignment = Alignment.CenterVertically,
    horizontalArrangement = Arrangement.spacedBy(12.dp)
  ) {
    AppIcon(icon)
    Column(Modifier.weight(1f)) {
      Text(formatDate(occurrence.on))
      if (details.isNotEmpty()) {
        Text(
          details,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          style = MaterialTheme.typography.bodySmall
        )
      }
    }
    Box {
      IconButton(onClick = { menu = true }) { AppIcon(R.drawable.more, "Occurrence menu") }
      DropdownMenu(menu, onDismissRequest = { menu = false }) {
        fun item(label: String, choice: String): @Composable () -> Unit = {
          DropdownMenuItem(text = { Text(label) }, onClick = {
            menu = false
            choose(choice)
          })
        }
        val items = buildList {
          if (occurrence.status == OccurrenceStatus.DONE ||
            occurrence.status == OccurrenceStatus.SKIPPED
          ) {
            add(item("Reopen", "reopen"))
          } else {
            add(item("Done", "done"))
            add(item("Skip", "skip"))
          }
          if (occurrence.status == OccurrenceStatus.CURRENT ||
            occurrence.status == OccurrenceStatus.UPCOMING
          ) {
            add(item("Move…", "move"))
            if (occurrence.moved) add(item("Put back on its date", "put-back"))
          }
          add(
            if (occurrence.note !=
              null
            ) {
              item("Open note", "open-note")
            } else {
              item("Write a note", "write-note")
            }
          )
        }
        items.forEach { it() }
      }
    }
  }
  HorizontalDivider()
}
