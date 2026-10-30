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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
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
import java.time.temporal.ChronoUnit
import systems.thoughtfull.commonplace.core.DayOffEffect
import systems.thoughtfull.commonplace.core.DaysOffKind
import systems.thoughtfull.commonplace.core.DaysOffUse
import systems.thoughtfull.commonplace.core.MobileDaysOff
import systems.thoughtfull.commonplace.core.daysOffHidden
import systems.thoughtfull.commonplace.core.daysOffKindName
import systems.thoughtfull.commonplace.core.describeDaysOff
import systems.thoughtfull.commonplace.core.newDaysOff

/**
 * The Days off screen, as the Linux app's Days off dialog shows it: holidays and vacations, each a
 * list of schedules that recurring actions and journal schedules can skip or move past.
 */
@Composable
internal fun DaysOffDialog(model: NotebookModel, dismiss: () -> Unit) {
  val state by model.state.collectAsState()
  var kind by remember { mutableStateOf(DaysOffKind.HOLIDAYS) }
  var all by remember { mutableStateOf<List<MobileDaysOff>>(emptyList()) }
  var showHidden by remember { mutableStateOf(false) }
  var editing by remember { mutableStateOf<MobileDaysOff?>(null) }
  var adding by remember { mutableStateOf(false) }
  val today = LocalDate.now().toString()
  LaunchedEffect(state.revision) { all = model.read { it.daysOff() }.orEmpty() }
  val (hidden, shown) = all.filter { it.days.kind == kind }
    .partition { daysOffHidden(it.days, today) }
  val several = state.spaces.size > 1
  Dialog(dismiss, DialogProperties(usePlatformDefaultWidth = false)) {
    Scaffold(
      topBar = {
        TopAppBar(
          title = { Text("Days off") },
          navigationIcon = { IconButton(onClick = dismiss) { AppIcon(R.drawable.back, "Back") } }
        )
      }
    ) { padding ->
      Column(
        Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
          .padding(horizontal = 16.dp, vertical = 8.dp)
          .testTag("days-off"),
        verticalArrangement = Arrangement.spacedBy(4.dp)
      ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
          DaysOffKind.entries.forEach { option ->
            FilterChip(kind == option, onClick = { kind = option }, label = {
              Text(daysOffKindName(option))
            })
          }
        }
        shown.forEach { item ->
          DaysOffRow(item, several, model) { editing = item }
        }
        if (shown.isEmpty()) {
          Text(
            if (kind == DaysOffKind.HOLIDAYS) "No holidays" else "No vacations",
            Modifier.padding(vertical = 8.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant
          )
        }
        if (hidden.isNotEmpty()) {
          TextButton(onClick = { showHidden = !showHidden }) {
            Text("${if (showHidden) "▾" else "▸"} Show hidden (${hidden.size})")
          }
          if (showHidden) {
            hidden.forEach { item -> DaysOffRow(item, several, model) { editing = item } }
          }
        }
        TextButton(onClick = { adding = true }) {
          AppIcon(R.drawable.add)
          Text("Add schedule")
        }
      }
    }
  }
  if (adding || editing != null) {
    DaysOffEditor(model, state.spaces.map { it.key }, kind, editing, {
      adding = false
      editing = null
    })
  }
}

/** A row for one schedule of days off, with a menu that edits or archives it. */
@Composable
private fun DaysOffRow(
  item: MobileDaysOff,
  several: Boolean,
  model: NotebookModel,
  edit: () -> Unit
) {
  var menu by remember { mutableStateOf(false) }
  val days = item.days
  val subtitle = listOfNotNull(
    "Archived".takeIf { days.archived != null },
    model.repository.name(item.space).takeIf { several },
    describeDaysOff(days)
  ).joinToString(" · ")
  Row(
    Modifier.fillMaxWidth().padding(vertical = 4.dp),
    verticalAlignment = Alignment.CenterVertically
  ) {
    Column(Modifier.weight(1f)) {
      Text(days.name)
      Text(
        subtitle,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        style = MaterialTheme.typography.bodySmall
      )
    }
    Box {
      IconButton(onClick = { menu = true }) { AppIcon(R.drawable.more, "Schedule menu") }
      DropdownMenu(menu, onDismissRequest = { menu = false }) {
        DropdownMenuItem(text = { Text("Edit") }, onClick = {
          menu = false
          edit()
        })
        DropdownMenuItem(
          text = { Text(if (days.archived != null) "Unarchive" else "Archive") },
          onClick = {
            menu = false
            val today = LocalDate.now().toString()
            model.perform {
              it.archiveDaysOff(item.space, days.kind, days.id, days.archived == null, today)
            }
          }
        )
      }
    }
  }
  HorizontalDivider()
}

/**
 * The dialog that adds a schedule of days off of `kind`, or edits `existing` in its library, with
 * the Linux dialog's fields: name, first and last day, how it repeats, and, when adding with more
 * than one library, which library.
 */
@Composable
private fun DaysOffEditor(
  model: NotebookModel,
  spaces: List<String>,
  kind: DaysOffKind,
  existing: MobileDaysOff?,
  dismiss: () -> Unit
) {
  val today = LocalDate.now().toString()
  var days by remember {
    mutableStateOf(existing?.days ?: newDaysOff(kind, "", today))
  }
  var name by remember { mutableStateOf(days.name) }
  var space by remember {
    mutableStateOf(
      existing?.space ?: model.repository.preferences.getString("capture.space", null)
        ?.takeIf { it in spaces } ?: spaces.first()
    )
  }
  var picking by remember { mutableStateOf<String?>(null) }
  var custom by remember { mutableStateOf(false) }
  AlertDialog(
    onDismissRequest = dismiss,
    title = { Text(if (existing == null) "Add schedule" else "Edit schedule") },
    text = {
      Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
          name,
          { name = it },
          label = { Text("Name") },
          placeholder = { Text("Christmas") },
          singleLine = true
        )
        EditorRow("First day") {
          TextButton(onClick = { picking = "first" }) { Text(formatDate(days.first)) }
        }
        EditorRow("Last day") {
          TextButton(onClick = { picking = "last" }) {
            Text(if (days.last == days.first) "Same day" else formatDate(days.last))
          }
        }
        EditorRow("Repeats") {
          RepeatMenu(days.rule, days.first, allowNone = true) { choice ->
            when (choice) {
              RepeatChoice.None -> days = days.copy(rule = null)
              is RepeatChoice.Preset -> days = days.copy(rule = choice.rule)
              RepeatChoice.Custom -> custom = true
            }
          }
        }
        if (existing == null && spaces.size > 1) {
          EditorRow("Library") {
            MenuControl(space, spaces, { model.repository.name(it) }) { space = it }
          }
        }
      }
    },
    confirmButton = {
      TextButton(onClick = {
        val saved = days.copy(name = name.trim().ifEmpty { formatDate(days.first) })
        model.perform { it.setDaysOff(space, saved) }
        dismiss()
      }) { Text(if (existing == null) "Add" else "Save") }
    },
    dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } }
  )
  picking?.let { field ->
    val initial = LocalDate.parse(if (field == "first") days.first else days.last)
    CalendarDialog(
      initial,
      { picking = null },
      clearLabel = if (field == "first") "Today" else "Same day"
    ) { picked ->
      days = if (field == "first") {
        val first = picked ?: LocalDate.now()
        val span = ChronoUnit.DAYS.between(LocalDate.parse(days.first), LocalDate.parse(days.last))
        days.copy(first = first.toString(), last = first.plusDays(span).toString())
      } else {
        val last = picked?.toString()?.takeIf { it >= days.first } ?: days.first
        days.copy(last = last)
      }
      picking = null
    }
  }
  if (custom) {
    RepeatEditorDialog(
      model,
      null,
      days.rule,
      days.first,
      showStart = false,
      daysOff = DaysOffUse(DayOffEffect.IGNORE, DayOffEffect.IGNORE),
      dismiss = { custom = false }
    ) { values ->
      days = days.copy(rule = values.rule)
      custom = false
    }
  }
}
