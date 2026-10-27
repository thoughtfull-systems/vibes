// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import kotlinx.coroutines.launch
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.ActionView
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.MobileNotebook

@Composable
internal fun InboxScreen(
  model: NotebookModel,
  state: NotebookState,
  editingChanged: (Boolean) -> Unit,
  open: (String) -> Unit = {}
) {
  var skipped by rememberSaveable { mutableStateOf(emptyList<String>()) }
  var currentId by rememberSaveable { mutableStateOf<String?>(null) }
  val queue = state.actions[ActionView.INBOX].orEmpty().filter { it.note.id !in skipped }
  val current = queue.find { it.note.id == currentId } ?: queue.firstOrNull()
  LaunchedEffect(current?.note?.id) { currentId = current?.note?.id }
  DisposableEffect(Unit) { onDispose { editingChanged(false) } }
  if (current == null) {
    Column(
      Modifier.fillMaxSize().testTag("inbox-empty"),
      horizontalAlignment = Alignment.CenterHorizontally
    ) {
      if (skipped.isEmpty()) {
        EmptyNotes(
          R.drawable.inbox,
          "Inbox Is Empty",
          "Tasks you capture wait here until you sort them"
        )
      } else {
        EmptyNotes(
          R.drawable.inbox,
          "Nothing Left to Sort",
          "Start over to sort the actions you skipped"
        )
        Button(onClick = { skipped = emptyList() }) { Text("Start over") }
      }
    }
  } else {
    key(current.note.id) {
      InboxItem(model, state, current.note, queue.size, editingChanged, open) {
        skipped = skipped + current.note.id
      }
    }
  }
}

internal fun parentChoices(notes: List<MobileNote>, id: String): List<MobileNote> {
  val excluded = mutableSetOf(id)
  var previous: Int
  do {
    previous = excluded.size
    notes.filter { it.parent in excluded }.forEach { excluded.add(it.id) }
  } while (excluded.size != previous)
  return notes.filter { it.action != null && !it.inTrash && it.id !in excluded }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun InboxItem(
  model: NotebookModel,
  state: NotebookState,
  note: MobileNote,
  remaining: Int,
  editingChanged: (Boolean) -> Unit,
  open: (String) -> Unit,
  skip: () -> Unit
) {
  var value by rememberSaveable(stateSaver = TextFieldValue.Saver) {
    mutableStateOf(TextFieldValue(""))
  }
  var loaded by rememberSaveable { mutableStateOf(false) }
  var pending by remember { mutableStateOf(false) }
  var dateField by remember { mutableStateOf<String?>(null) }
  var waiting by remember { mutableStateOf(false) }
  var reason by rememberSaveable { mutableStateOf("") }
  val waitingFocus = remember { FocusRequester() }
  var parentPicker by remember { mutableStateOf(false) }
  var linkPicker by remember { mutableStateOf(false) }
  val context = LocalContext.current
  val focus = remember { FocusRequester() }
  val focusManager = LocalFocusManager.current
  val keyboard = LocalSoftwareKeyboardController.current
  val keyboardVisible = WindowInsets.ime.getBottom(LocalDensity.current) > 0
  var focused by remember { mutableStateOf(false) }
  val editing = focused && keyboardVisible
  LaunchedEffect(editing) { editingChanged(editing) }
  LaunchedEffect(note.id, state.revision) {
    if (!loaded) {
      model.read { it.editorBody(note.id) }?.let {
        value = TextFieldValue(model.draft(note.id) ?: it)
        loaded = true
      }
    }
  }
  val action = note.action!!
  val enabled = loaded && !note.readOnly && !pending
  fun update(next: TextFieldValue) {
    val changed = value.text != next.text
    value = next
    if (changed) model.saveBody(note.id, next.text) { }
  }
  fun process(after: () -> Unit = {}, operation: (MobileNotebook) -> Unit) {
    pending = true
    focusManager.clearFocus()
    keyboard?.hide()
    model.perform(complete = { success ->
      pending = false
      if (success) after()
    }) { core ->
      core.setBody(note.id, value.text)
      operation(core)
    }
  }
  fun sort(list: ActionList) {
    process { it.changeAction(note.id, ActionChange.List(list)) }
  }
  fun change(change: ActionChange) {
    model.perform { it.changeAction(note.id, change) }
  }
  Column(Modifier.fillMaxSize().testTag("inbox-processor")) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
      Text("Sort the Inbox", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
      Text(if (remaining == 1) "Last one" else "${remaining - 1} more")
    }
    Column(
      Modifier.weight(1f).verticalScroll(rememberScrollState())
    ) {
      MarkdownEditor(
        value,
        enabled,
        ::update,
        focus,
        { focused = it },
        Modifier.fillMaxWidth().height(if (editing) 300.dp else 220.dp),
        { link ->
          if (link.note != null) {
            open(link.note)
          } else if (link.uri != null) {
            runCatching {
              context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(link.uri)))
            }.onFailure(model::report)
          }
        }
      )
      if (!editing) {
        HorizontalDivider()
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
          FlowRow(
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            itemVerticalAlignment = Alignment.CenterVertically
          ) {
            Row(
              horizontalArrangement = Arrangement.spacedBy(8.dp),
              verticalAlignment = Alignment.CenterVertically
            ) {
              Text("Due", color = MaterialTheme.colorScheme.onSurfaceVariant)
              OutlinedButton(
                colors = neutralButtonColors(),
                enabled = enabled,
                onClick = { dateField = "Due" },
                contentPadding = PaddingValues(horizontal = 12.dp),
                modifier = Modifier.testTag("inbox-due")
              ) {
                Text(action.due?.let { formatDate(it) } ?: "None")
                AppIcon(R.drawable.dropdown)
              }
            }
            Row(
              horizontalArrangement = Arrangement.spacedBy(8.dp),
              verticalAlignment = Alignment.CenterVertically
            ) {
              Text("Project", color = MaterialTheme.colorScheme.onSurfaceVariant)
              Switch(action.project, { change(ActionChange.Project(it)) }, enabled = enabled)
            }
            Row(
              horizontalArrangement = Arrangement.spacedBy(8.dp),
              verticalAlignment = Alignment.CenterVertically
            ) {
              Text("In", color = MaterialTheme.colorScheme.onSurfaceVariant)
              Text(
                formattedTitle(
                  state.actionPaths[note.id].orEmpty()
                    .mapNotNull { id -> state.notes.find { it.id == id }?.let(::displayLabel) }
                    .joinToString(" › ").ifEmpty { "Top level" }
                ),
                Modifier.weight(1f, fill = false)
              )
              OutlinedButton(
                colors = neutralButtonColors(),
                enabled = enabled,
                onClick = { parentPicker = true }
              ) {
                Text("Move…")
              }
            }
          }
          // The button order follows the Linux inbox.
          FlowRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            itemVerticalAlignment = Alignment.CenterVertically
          ) {
            Text("Sort onto", color = MaterialTheme.colorScheme.onSurfaceVariant)
            Button(enabled = enabled, onClick = { sort(ActionList.ACTIVE) }) { Text("Active") }
            listOf(
              "Waiting…" to { waiting = true },
              "Schedule…" to { dateField = "Schedule" },
              "Someday" to { sort(ActionList.SOMEDAY) },
              "Maybe" to { sort(ActionList.MAYBE) }
            ).forEach { (label, choose) ->
              OutlinedButton(colors = neutralButtonColors(), enabled = enabled, onClick = choose) {
                Text(label)
              }
            }
          }
          if (waiting) {
            fun saveWaiting() {
              process {
                it.changeAction(note.id, ActionChange.WaitingFor(reason.trim().ifBlank { null }))
                it.changeAction(note.id, ActionChange.List(ActionList.WAITING))
              }
            }
            LaunchedEffect(Unit) { waitingFocus.requestFocus() }
            OutlinedTextField(
              reason,
              { reason = it },
              label = { Text("Who or what") },
              singleLine = true,
              keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
              keyboardActions = KeyboardActions(onDone = { saveWaiting() }),
              trailingIcon = {
                IconButton(enabled = enabled, onClick = ::saveWaiting) {
                  AppIcon(R.drawable.actions, "Save waiting reason")
                }
              },
              modifier = Modifier.fillMaxWidth().focusRequester(waitingFocus)
                .testTag("inbox-waiting")
            )
          }
        }
      }
    }
    HorizontalDivider()
    Row(
      Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp)
        .testTag("inbox-processing-actions"),
      horizontalArrangement = Arrangement.spacedBy(6.dp),
      verticalAlignment = Alignment.CenterVertically
    ) {
      Text("Or", color = MaterialTheme.colorScheme.onSurfaceVariant)
      OutlinedButton(
        colors = neutralButtonColors(),
        enabled = enabled,
        contentPadding = PaddingValues(horizontal = 8.dp),
        onClick = {
          process { it.changeAction(note.id, ActionChange.State(ActionState.DONE)) }
        }
      ) { Text("Done") }
      OutlinedButton(
        colors = neutralButtonColors(),
        enabled = enabled,
        contentPadding = PaddingValues(horizontal = 8.dp),
        onClick = { process { it.setArchived(note.id, true) } }
      ) { Text("Archive") }
      Button(
        colors = ButtonDefaults.buttonColors(
          containerColor = MaterialTheme.colorScheme.error,
          contentColor = MaterialTheme.colorScheme.onError
        ),
        enabled = enabled,
        contentPadding = PaddingValues(horizontal = 8.dp),
        onClick = { process { it.trash(note.id) } }
      ) { Text("Trash") }
      Spacer(Modifier.weight(1f))
      OutlinedButton(
        colors = neutralButtonColors(),
        enabled = enabled,
        contentPadding = PaddingValues(horizontal = 8.dp),
        onClick = { process(skip) { } }
      ) { Text("Skip") }
    }
    if (editing && enabled) {
      EditorToolbar(value, {
        update(it)
        focus.requestFocus()
        keyboard?.show()
      }) { linkPicker = true }
    }
  }
  dateField?.let { field ->
    CalendarDialog(
      (if (field == "Due") action.due else action.defer)?.let(LocalDate::parse),
      { dateField = null }
    ) { date ->
      if (field == "Due") {
        change(ActionChange.Due(date?.toString()))
      } else if (date != null) {
        process {
          it.changeAction(note.id, ActionChange.Defer(date.toString()))
          it.changeAction(note.id, ActionChange.List(ActionList.ACTIVE))
        }
      }
      dateField = null
    }
  }
  if (parentPicker) {
    MoveActionPicker(model, note, state.notes) { parentPicker = false }
  }
  if (linkPicker) {
    InboxLinkPicker(model, note, value, state.notes, { linkPicker = false }) {
      update(it)
      linkPicker = false
      focus.requestFocus()
      keyboard?.show()
    }
  }
}

@Composable
private fun InboxLinkPicker(
  model: NotebookModel,
  note: MobileNote,
  value: TextFieldValue,
  notes: List<MobileNote>,
  dismiss: () -> Unit,
  changed: (TextFieldValue) -> Unit
) {
  val scope = rememberCoroutineScope()
  NotePicker(notes.filter { !it.inTrash && it.id != note.id }, dismiss) { target ->
    scope.launch {
      model.read { it.linkText(note.id, target) }?.let { insert ->
        changed(
          TextFieldValue(
            value.text.replaceRange(value.selection.min, value.selection.max, insert),
            TextRange(value.selection.min + insert.length)
          )
        )
      }
    }
  }
}
