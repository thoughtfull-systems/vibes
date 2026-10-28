// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import systems.thoughtfull.commonplace.core.MobileNote

@Composable
internal fun NotePicker(
  notes: List<MobileNote>,
  dismiss: () -> Unit,
  create: ((String, Boolean) -> Unit)? = null,
  topLevel: (() -> Unit)? = null,
  pick: (String) -> Unit
) {
  var query by rememberSaveable { mutableStateOf("") }
  val focus = remember { FocusRequester() }
  val matches = notes.filter { it.matches(query) }
  fun accept() {
    if (query.isBlank() &&
      topLevel != null
    ) {
      topLevel()
    } else {
      matches.firstOrNull()?.let { pick(it.id) }
    }
  }
  AlertDialog(
    onDismissRequest = dismiss,
    title = { Text(if (topLevel == null) "Link to…" else "Move to…") },
    text = {
      val focused = LocalWindowInfo.current.isWindowFocused
      LaunchedEffect(focused) { if (focused) focus.requestFocus() }
      Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
          query,
          { query = it },
          label = { Text("Search") },
          singleLine = true,
          keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
          keyboardActions = KeyboardActions(onDone = { accept() }),
          modifier = Modifier.fillMaxWidth().focusRequester(focus).testTag("note-picker-search")
        )
        if (topLevel != null) {
          TextButton(
            onClick = topLevel,
            modifier = Modifier.fillMaxWidth().testTag("note-picker-top-level").background(
              if (query.isBlank()) {
                MaterialTheme.colorScheme.secondaryContainer
              } else {
                MaterialTheme.colorScheme.surface
              }
            )
          ) { Text("Top level") }
        }
        LazyColumn(Modifier.heightIn(max = 300.dp)) {
          items(matches, key = { it.id }) { note ->
            TextButton(
              onClick = { pick(note.id) },
              modifier = Modifier.fillMaxWidth().testTag(
                "note-picker-result-${note.id}"
              ).background(
                if (query.isNotBlank() && note == matches.firstOrNull()) {
                  MaterialTheme.colorScheme.secondaryContainer
                } else {
                  MaterialTheme.colorScheme.surface
                }
              )
            ) {
              Text(formattedTitle(note.title.ifBlank { "Untitled" }))
            }
          }
        }
      }
    },
    confirmButton = {
      if (create != null) {
        Row {
          TextButton(enabled = query.isNotBlank(), onClick = { create(query, true) }) {
            Text("Create action")
          }
          TextButton(enabled = query.isNotBlank(), onClick = { create(query, false) }) {
            Text("Create memo")
          }
        }
      }
    },
    dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } }
  )
}

@Composable
internal fun MoveActionPicker(
  model: NotebookModel,
  note: MobileNote,
  notes: List<MobileNote>,
  dismiss: () -> Unit
) {
  var target by remember { mutableStateOf<MobileNote?>(null) }
  fun move(parent: String?) {
    model.perform(complete = { if (it) dismiss() }) { it.moveAction(note.id, parent, null) }
  }
  NotePicker(
    parentChoices(notes, note.id).filter { !it.readOnly },
    dismiss,
    topLevel = { move(null) }
  ) { id ->
    val parent = notes.first { it.id == id }
    if (parent.space != note.space) target = parent else move(id)
  }
  target?.let { parent ->
    ConfirmDialog(
      "Move to ${model.repository.name(parent.space)}?",
      "Edits to the note on another device that haven't synced yet are lost.",
      { target = null },
      confirmLabel = "Move"
    ) {
      target = null
      move(parent.id)
    }
  }
}

@Composable
internal fun JournalEntryMenu(model: NotebookModel, note: MobileNote, opened: (String) -> Unit) {
  val state by model.state.collectAsState()
  var expanded by remember { mutableStateOf(false) }
  val journals = state.journals.filter {
    it.space == note.space && !it.inTrash && it.archived == null &&
      state.spaces.any { space -> space.key == it.space && space.shown }
  }
  if (journals.isEmpty()) return
  Box {
    OutlinedButton(onClick = { expanded = true }, colors = neutralButtonColors()) {
      Text("Make journal entry")
      AppIcon(R.drawable.dropdown)
    }
    DropdownMenu(expanded, onDismissRequest = { expanded = false }) {
      journals.forEach { journal ->
        DropdownMenuItem(text = { Text(formattedTitle(journal.title)) }, onClick = {
          expanded = false
          var entry: String? = null
          model.perform(complete = { if (it) entry?.let(opened) }) {
            entry = it.convertMemo(note.id, journal.id)
          }
        }, leadingIcon = { JournalDot(journal.journal!!.color) })
      }
    }
  }
}
