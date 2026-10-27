// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalMinimumInteractiveComponentSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.ActionStatus
import systems.thoughtfull.commonplace.core.MobileNote

internal fun toggleAction(model: NotebookModel, note: MobileNote) {
  model.perform {
    it.changeAction(
      note.id,
      ActionChange.State(
        if (note.action?.state == ActionState.DONE) ActionState.OPEN else ActionState.DONE
      )
    )
  }
}

@Composable
internal fun NoteRow(
  note: MobileNote,
  open: (String) -> Unit,
  stamp: String? = "Modified",
  plan: (() -> Unit)? = null,
  metadata: String? = null,
  urgency: ActionStatus? = null,
  complete: (() -> Unit)? = null,
  parentPath: String? = null,
  entryColor: String = "blue"
) {
  val titleCenter = with(LocalDensity.current) {
    MaterialTheme.typography.bodyLarge.lineHeight.roundToPx() / 2
  }
  Row(
    Modifier.fillMaxWidth().testTag("note-row-${note.id}").clickable { open(note.id) }
      .padding(start = 12.dp, end = 16.dp)
      .padding(vertical = 12.dp),
    verticalAlignment = Alignment.CenterVertically
  ) {
    Box(
      Modifier.size(32.dp).alignBy { it.measuredHeight / 2 }.testTag(
        if (note.action != null) "note-checkbox" else "note-kind-icon"
      ),
      contentAlignment = Alignment.Center
    ) {
      if (note.action != null) {
        CompositionLocalProvider(LocalMinimumInteractiveComponentSize provides 32.dp) {
          Checkbox(
            note.action.state == ActionState.DONE,
            onCheckedChange = complete?.let {
              { it() }
            }
          )
        }
      } else if (note.entry != null) {
        JournalDot(entryColor)
      } else {
        AppIcon(
          if (note.journal != null) R.drawable.journal else R.drawable.memo
        )
      }
    }
    Spacer(Modifier.width(8.dp))
    Column(
      Modifier.weight(1f).alignBy { titleCenter },
      verticalArrangement = Arrangement.spacedBy(4.dp)
    ) {
      Text(
        formattedTitle(note.title.ifBlank { "Empty ${note.kind}" }),
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        color = when (urgency) {
          ActionStatus.OVERDUE -> MaterialTheme.colorScheme.error
          ActionStatus.DUE_SOON -> Color(0xFFB26314)
          else -> MaterialTheme.colorScheme.onSurface
        },
        style = MaterialTheme.typography.bodyLarge.copy(
          fontWeight = if (urgency in listOf(ActionStatus.OVERDUE, ActionStatus.DUE_SOON)) {
            FontWeight.Bold
          } else {
            FontWeight.Normal
          }
        )
      )
      val summary = listOfNotNull(
        "Archived".takeIf { note.action != null && note.archived != null },
        "Project".takeIf { note.action?.project == true },
        note.action?.let { action ->
          when (action.list) {
            ActionList.INBOX -> "Inbox"
            ActionList.WAITING -> action.waitingFor?.let { "Waiting for $it" } ?: "Waiting"
            ActionList.MAYBE -> "Maybe"
            ActionList.SOMEDAY -> "Someday"
            else -> null
          }
        },
        metadata?.takeIf { it.isNotBlank() },
        stamp?.let {
          val time = when (it) {
            "Archived" -> note.archived ?: note.modified
            "Created" -> note.created
            "Trashed" -> note.trashed ?: note.modified
            else -> note.modified
          }
          "$it ${listTimestamp(time)}"
        }
      ).joinToString(" · ")
      if (summary.isNotEmpty()) {
        Text(
          summary,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          style = MaterialTheme.typography.bodySmall
        )
      }
      if (!parentPath.isNullOrBlank()) {
        Text(
          formattedTitle(parentPath),
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          style = MaterialTheme.typography.bodySmall
        )
      }
    }
    if (plan != null) {
      TextButton(onClick = plan) {
        Text(if (note.action?.planning != null) "Queued" else "Plan")
      }
    }
  }
  HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
}

@Composable
internal fun EmptyNotes(query: String) {
  Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
    Text(if (query.isBlank()) "No notes here yet." else "No notes match your search.")
  }
}
