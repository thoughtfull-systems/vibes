// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionStatus
import systems.thoughtfull.commonplace.core.ActionView
import systems.thoughtfull.commonplace.core.MobileAction

internal data class ActionSection(
  val title: String,
  val entries: List<MobileAction>,
  val plan: Boolean = false
)

internal fun actionSections(
  mode: String,
  views: Map<ActionView, List<MobileAction>>,
  query: String,
  now: Long = System.currentTimeMillis()
): List<ActionSection> {
  fun view(view: ActionView) = views[view].orEmpty()
  fun unqueued(view: ActionView) = view(view).filter { it.note.action?.planning == null }
  if (query.isNotBlank()) {
    return listOf(
      ActionSection(
        "",
        view(ActionView.ALL).filter {
          it.note.matches(query)
        }.map { it.copy(depth = 0u) }
      )
    )
  }
  // The sections and modes follow the Linux actions view's workflow.
  return when (mode) {
    "Inbox" -> listOf(ActionSection("Inbox", view(ActionView.INBOX)))

    "Plan" -> {
      val queue = view(ActionView.PLANNING)
      val queued = queue.map { it.note.id }.toSet()
      listOf(
        ActionSection("Planning Queue", queue, true),
        ActionSection(
          "Stalled Projects",
          view(ActionView.STALLED).filter {
            it.note.id !in queued
          },
          true
        )
      )
    }

    "Review" -> listOf(
      ActionSection("Projects", unqueued(ActionView.PROJECTS), true),
      ActionSection("Waiting For", unqueued(ActionView.WAITING), true),
      ActionSection("Scheduled", unqueued(ActionView.SCHEDULED), true),
      ActionSection("Someday", unqueued(ActionView.SOMEDAY), true),
      ActionSection("Maybe", unqueued(ActionView.MAYBE), true),
      ActionSection(
        "Recently Done",
        view(ActionView.CLOSED).filter {
          (it.note.action?.closed ?: 0) >= now - 7 * 86_400_000L
        }
      )
    )

    else -> {
      val next = view(ActionView.NEXT)
      listOf(
        ActionSection("Overdue", next.filter { it.dates.status == ActionStatus.OVERDUE }),
        ActionSection("Due Soon", next.filter { it.dates.status == ActionStatus.DUE_SOON }),
        ActionSection("Active", next.filter { it.dates.status == ActionStatus.NORMAL }),
        ActionSection("Starting Soon", next.filter { it.dates.status == ActionStatus.UPCOMING })
      )
    }
  }
}

@Composable
internal fun ActionsScreen(
  model: NotebookModel,
  state: NotebookState,
  query: String,
  open: (String) -> Unit,
  editingChanged: (Boolean) -> Unit
) {
  var mode by rememberSaveable {
    mutableStateOf(model.repository.preferences.getString("action.mode", "Execute")!!)
  }
  LaunchedEffect(mode) {
    model.repository.preferences.edit().putString("action.mode", mode).apply()
  }
  MenuControl(
    mode,
    // The mode order follows the Linux sidebar.
    listOf("Execute", "Plan", "Review", "Inbox"),
    { it },
    Modifier.padding(horizontal = 12.dp, vertical = 4.dp)
  ) { mode = it }
  if (mode == "Inbox" && query.isBlank()) {
    InboxScreen(model, state, editingChanged, open)
    return
  }
  val sections = actionSections(mode, state.actions, query)
  LazyColumn(Modifier.fillMaxSize().testTag("actions")) {
    if (sections.all { it.entries.isEmpty() }) item { EmptyNotes(query) }
    sections.filter { it.entries.isNotEmpty() }.forEach { section ->
      if (section.title.isNotEmpty()) {
        item {
          Text(
            section.title,
            Modifier.fillMaxWidth().padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.titleSmall
          )
        }
      }
      items(section.entries, key = { "${section.title}.${it.note.id}" }) { entry ->
        Column(
          Modifier.padding(start = (entry.depth.toInt().coerceAtMost(6) * 12).dp)
            .alpha(if (entry.dates.status == ActionStatus.UPCOMING) 0.65f else 1f)
        ) {
          val context = entry.parents.mapNotNull { parent ->
            state.notes.find { it.id == parent }?.title
          }.joinToString(" › ")
          val dates = listOfNotNull(
            entry.dates.due?.let { "Due ${it.date}" },
            entry.dates.start?.let { "Starts ${it.date}" },
            "No next step".takeIf { entry.stalled }
          ).joinToString(" · ")
          NoteRow(
            entry.note,
            open,
            complete = { toggleAction(model, entry.note) },
            stamp = null,
            metadata = dates,
            parentPath = context,
            urgency = entry.dates.status,
            plan = if (section.plan) {
              {
                model.perform {
                  it.changeAction(
                    entry.note.id,
                    ActionChange.Planning(entry.note.action?.planning == null)
                  )
                }
              }
            } else {
              null
            }
          )
        }
      }
    }
  }
}
