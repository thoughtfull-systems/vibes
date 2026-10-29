// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionStatus
import systems.thoughtfull.commonplace.core.ActionView
import systems.thoughtfull.commonplace.core.MobileAction

internal data class ActionSection(
  val title: String,
  val entries: List<MobileAction>,
  val plan: Boolean = false,
  val star: Boolean = false
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
        ActionSection("Planning queue", queue, true),
        ActionSection(
          "Stalled projects",
          view(ActionView.STALLED).filter {
            it.note.id !in queued
          },
          true
        )
      )
    }

    "Review" -> listOf(
      ActionSection("Projects", unqueued(ActionView.PROJECTS), true),
      ActionSection("Waiting for", unqueued(ActionView.WAITING), true),
      ActionSection("Scheduled", unqueued(ActionView.SCHEDULED), true),
      ActionSection("Someday", unqueued(ActionView.SOMEDAY), true),
      ActionSection("Maybe", unqueued(ActionView.MAYBE), true),
      ActionSection(
        "Recently done",
        view(ActionView.CLOSED).filter {
          (it.note.action?.closed ?: 0) >= now - 7 * 86_400_000L
        }
      )
    )

    else -> {
      val next = view(ActionView.NEXT)
      listOf(
        ActionSection("Starred", view(ActionView.STARRED), star = true),
        ActionSection("Overdue", next.filter { it.dates.status == ActionStatus.OVERDUE }),
        ActionSection("Due soon", next.filter { it.dates.status == ActionStatus.DUE_SOON }),
        ActionSection("Active", next.filter { it.dates.status == ActionStatus.NORMAL }),
        ActionSection("Starting soon", next.filter { it.dates.status == ActionStatus.UPCOMING })
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
  Box(Modifier.fillMaxWidth().padding(vertical = 4.dp), contentAlignment = Alignment.Center) {
    // The mode order follows the Linux mode menu.
    MenuControl(mode, listOf("Execute", "Plan", "Review", "Inbox"), { it }) { mode = it }
  }
  HorizontalDivider()
  if (mode == "Inbox" && query.isBlank()) {
    InboxScreen(model, state, editingChanged, open)
    return
  }
  val sections = actionSections(mode, state.actions, query)
  val context = rememberRowContext(state)
  LazyColumn(Modifier.fillMaxSize().testTag("actions")) {
    if (sections.all { it.entries.isEmpty() }) item { ActionsEmpty(mode, query) }
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
        Column(Modifier.padding(start = (entry.depth.toInt().coerceAtMost(6) * 12).dp)) {
          NoteRow(
            entry.note,
            context,
            open,
            complete = { toggleAction(model, entry.note) },
            dates = entry.dates,
            parents = entry.parents,
            stalled = entry.stalled,
            star = if (section.star) {
              { toggleStar(model, entry.note) }
            } else {
              null
            },
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

@Composable
private fun ActionsEmpty(mode: String, query: String) {
  when {
    query.isNotBlank() -> EmptyNotes(R.drawable.search, "No actions found", "Try other words")

    mode == "Plan" -> EmptyNotes(
      R.drawable.plan,
      "Nothing to plan",
      "Actions queued for planning and stalled projects show up here"
    )

    mode == "Review" -> EmptyNotes(
      R.drawable.review,
      "Nothing to review",
      "Projects, and waiting, scheduled, someday, and maybe actions show up here"
    )

    else -> EmptyNotes(
      R.drawable.execute,
      "Nothing to do next",
      "Starred actions and tasks on the active list show up here"
    )
  }
}
