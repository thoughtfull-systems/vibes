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
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalMinimumInteractiveComponentSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Locale
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionDates
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.ActionStatus
import systems.thoughtfull.commonplace.core.Dated
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

internal fun toggleStar(model: NotebookModel, note: MobileNote) {
  model.perform { it.changeAction(note.id, ActionChange.Starred(note.action?.starred == null)) }
}

/** A toggle that stars an action, or takes its star away. Starred actions lead Execute. */
@Composable
internal fun StarButton(starred: Boolean, onClick: () -> Unit) {
  IconButton(onClick, Modifier.testTag("star")) {
    if (starred) {
      // Adwaita's yellow_4, the color the Linux app gives a star.
      Icon(
        painterResource(R.drawable.starred),
        "Remove the star",
        Modifier.size(24.dp),
        tint = Color(0xFFF5C211)
      )
    } else {
      AppIcon(R.drawable.starred, "Star", Modifier.alpha(0.55f))
    }
  }
}

/** Which of a note's times a row shows. */
internal enum class Stamp(val word: String) {
  ARCHIVED("Archived"),
  CREATED("Created"),
  MODIFIED("Modified"),
  TRASHED("Trashed");

  fun time(note: MobileNote): Long = when (this) {
    ARCHIVED -> note.archived ?: note.modified
    CREATED -> note.created
    MODIFIED -> note.modified
    TRASHED -> note.trashed ?: note.modified
  }
}

/** The rest of the library that a row looks up to describe a note. */
internal class RowContext(
  notes: List<MobileNote>,
  private val actionDates: Map<String, ActionDates> = emptyMap(),
  private val actionPaths: Map<String, List<String>> = emptyMap()
) {
  private val notes = notes.associateBy { it.id }

  fun note(id: String) = notes[id]

  fun label(id: String) = notes[id]?.let(::displayLabel)

  fun dates(id: String) = actionDates[id]

  fun parents(id: String) = actionPaths[id].orEmpty()

  fun entryColor(note: MobileNote) = note.entry?.let { notes[it.journal]?.journal?.color } ?: "blue"
}

@Composable
internal fun rememberRowContext(state: NotebookState) = remember(state.revision) {
  RowContext(state.notes, state.actionDates, state.actionPaths)
}

internal fun emptyLabel(note: MobileNote) = when {
  note.action?.project == true -> "Empty project"
  note.action != null -> "Empty task"
  note.entry != null -> "Empty entry"
  note.journal != null -> "Untitled journal"
  else -> "Empty memo"
}

internal fun displayLabel(note: MobileNote) = note.title.ifBlank { emptyLabel(note) }

internal fun formatDate(
  date: LocalDate,
  today: LocalDate = LocalDate.now(),
  locale: Locale = Locale.getDefault()
): String = date.format(
  DateTimeFormatter.ofPattern(if (date.year == today.year) "EEE MMM d" else "MMM d, yyyy", locale)
)

internal fun formatDate(
  date: String,
  today: LocalDate = LocalDate.now(),
  locale: Locale = Locale.getDefault()
): String = runCatching { formatDate(LocalDate.parse(date), today, locale) }.getOrDefault(date)

/** Describes a date that applies to the action `id`, naming the project it comes from if any. */
internal fun describeDate(
  word: String,
  dated: Dated,
  id: String,
  label: (String) -> String?,
  today: LocalDate = LocalDate.now()
): String {
  val date = "$word ${formatDate(dated.date, today)}"
  if (dated.from == id) return date
  return label(dated.from)?.let { "$date from $it" } ?: date
}

/** The dates that apply to an action and come from a project above it, as words. */
internal fun inheritedDates(
  id: String,
  dates: ActionDates,
  label: (String) -> String?,
  today: LocalDate = LocalDate.now()
): String = listOf(
  "Starts" to dates.start?.takeIf { LocalDate.parse(it.date) > today },
  "Due" to dates.due
).mapNotNull { (word, dated) ->
  dated?.takeIf { it.from != id }?.let {
    "$word ${formatDate(it.date, today)} from ${label(it.from).orEmpty()}"
  }
}.joinToString(" · ")

/** A line that sums up an action's list, dates, and project state. */
internal fun actionSummary(
  note: MobileNote,
  dates: ActionDates?,
  stalled: Boolean,
  label: (String) -> String?,
  today: LocalDate = LocalDate.now()
): String {
  val action = note.action ?: return ""
  val open = action.state == ActionState.OPEN
  return listOfNotNull(
    "Archived".takeIf { note.archived != null },
    (if (stalled) "Stalled project" else "Project").takeIf { action.project },
    when (action.list) {
      ActionList.ACTIVE -> null
      ActionList.INBOX -> "Inbox"
      ActionList.MAYBE -> "Maybe"
      ActionList.SOMEDAY -> "Someday"
      ActionList.WAITING -> action.waitingFor?.let { "Waiting for $it" } ?: "Waiting"
    },
    dates?.due?.let {
      val overdue = open && LocalDate.parse(it.date) < today
      describeDate(if (overdue) "Overdue since" else "Due", it, note.id, label, today)
    },
    dates?.start?.takeIf { LocalDate.parse(it.date) > today }?.let {
      describeDate("Starts", it, note.id, label, today)
    }
  ).joinToString(" · ")
}

/** The line under a note's title, as the Linux rows write it. */
internal fun rowSummary(
  note: MobileNote,
  stamp: Stamp?,
  dates: ActionDates?,
  stalled: Boolean,
  context: RowContext,
  today: LocalDate = LocalDate.now()
): String {
  val stampText = stamp?.let { "${it.word} ${listTimestamp(it.time(note))}" }
  var summary = if (note.action != null) {
    listOfNotNull(actionSummary(note, dates, stalled, context::label, today), stampText)
      .filter { it.isNotEmpty() }.joinToString(" · ")
  } else {
    stampText ?: listTimestamp(note.modified)
  }
  if (note.action == null && note.archived != null && stamp != Stamp.ARCHIVED) {
    summary = "Archived · $summary"
  }
  val journal = note.entry?.let { context.note(it.journal) }
  return journal?.let { "${displayLabel(it)} · $summary" } ?: summary
}

@Composable
internal fun NoteRow(
  note: MobileNote,
  context: RowContext,
  open: (String) -> Unit,
  stamp: Stamp? = null,
  dates: ActionDates? = context.dates(note.id),
  parents: List<String> = context.parents(note.id),
  showPath: Boolean = true,
  stalled: Boolean = false,
  plan: (() -> Unit)? = null,
  star: (() -> Unit)? = null,
  complete: (() -> Unit)? = null
) {
  val titleCenter = with(LocalDensity.current) {
    MaterialTheme.typography.bodyLarge.lineHeight.roundToPx() / 2
  }
  val urgency = dates?.status
  val dimmed = note.action?.state == ActionState.DONE || urgency == ActionStatus.UPCOMING
  Row(
    Modifier.fillMaxWidth().testTag("note-row-${note.id}").clickable { open(note.id) }
      .alpha(if (dimmed) 0.55f else 1f)
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
        JournalDot(context.entryColor(note))
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
      val pressing = urgency == ActionStatus.OVERDUE || urgency == ActionStatus.DUE_SOON
      Text(
        formattedTitle(displayLabel(note)),
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        color = when (urgency) {
          ActionStatus.OVERDUE -> MaterialTheme.colorScheme.error
          ActionStatus.DUE_SOON -> Color(0xFFB26314)
          else -> MaterialTheme.colorScheme.onSurface
        },
        style = MaterialTheme.typography.bodyLarge.copy(
          fontStyle = if (note.action?.project == true) FontStyle.Italic else FontStyle.Normal,
          fontWeight = if (pressing) FontWeight.Bold else FontWeight.Normal
        )
      )
      val path = if (showPath) parents.mapNotNull(context::label).joinToString(" › ") else ""
      listOf(
        AnnotatedString(rowSummary(note, stamp, dates, stalled, context)),
        formattedTitle(path)
      ).filter { it.isNotEmpty() }.forEach { line ->
        Text(
          line,
          maxLines = 1,
          overflow = TextOverflow.Ellipsis,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          style = MaterialTheme.typography.bodySmall
        )
      }
    }
    if (plan != null && note.action?.state == ActionState.OPEN) {
      // A filled button stands in for the Linux toggle's pressed state.
      if (note.action.planning != null) {
        FilledTonalButton(onClick = plan) { Text("Plan") }
      } else {
        TextButton(onClick = plan) { Text("Plan") }
      }
    }
    if (star != null && note.action != null) {
      // Aligning the IconButton itself left the star centered on the row, so it copies the
      // checkbox's box instead.
      Box(
        Modifier.size(32.dp).alignBy { it.measuredHeight / 2 },
        contentAlignment = Alignment.Center
      ) {
        CompositionLocalProvider(LocalMinimumInteractiveComponentSize provides 32.dp) {
          StarButton(note.action.starred != null, star)
        }
      }
    }
  }
  HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
}

/** What a list shows when it has nothing to show, as the Linux status pages say it. */
@Composable
internal fun EmptyNotes(icon: Int, title: String, description: String) {
  Column(
    Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 48.dp).testTag("empty"),
    horizontalAlignment = Alignment.CenterHorizontally,
    verticalArrangement = Arrangement.spacedBy(12.dp)
  ) {
    AppIcon(
      icon,
      modifier = Modifier.size(64.dp).alpha(0.55f)
    )
    Text(title, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
    Text(
      description,
      color = MaterialTheme.colorScheme.onSurfaceVariant,
      textAlign = TextAlign.Center
    )
  }
}
