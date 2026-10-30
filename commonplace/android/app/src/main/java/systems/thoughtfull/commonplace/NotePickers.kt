// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.view.Gravity
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import systems.thoughtfull.commonplace.core.MobileNote

/**
 * Picks a note from a search, in a card at the top of the screen that the keyboard can't cover,
 * like the Linux app's popover. Choices to create a note follow the matches, as on Linux.
 */
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
  val creating = create != null && query.isNotBlank()
  fun accept() {
    when {
      query.isBlank() && topLevel != null -> topLevel()
      matches.isNotEmpty() -> pick(matches.first().id)
      creating -> create?.invoke(query, false)
    }
  }
  Dialog(dismiss, DialogProperties(usePlatformDefaultWidth = false)) {
    (LocalView.current.parent as? DialogWindowProvider)?.window?.setGravity(Gravity.TOP)
    val focused = LocalWindowInfo.current.isWindowFocused
    LaunchedEffect(focused) { if (focused) focus.requestFocus() }
    Surface(
      Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 16.dp),
      shape = RoundedCornerShape(20.dp),
      color = MaterialTheme.colorScheme.surfaceContainerHigh
    ) {
      Column(Modifier.padding(10.dp)) {
        CompactTextField(
          query,
          { query = it },
          if (topLevel == null) "Link to…" else "Move to…",
          Modifier.fillMaxWidth().focusRequester(focus).testTag("note-picker-search"),
          keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
          keyboardActions = KeyboardActions(onDone = { accept() })
        )
        LazyColumn(Modifier.heightIn(max = 300.dp).padding(top = 6.dp)) {
          if (topLevel != null) {
            item(key = "top-level") {
              PickerRow(
                AnnotatedString("Top level"),
                query.isBlank(),
                Modifier.testTag("note-picker-top-level")
              ) { topLevel() }
            }
          }
          items(matches, key = { it.id }) { note ->
            PickerRow(
              formattedTitle(note.title.ifBlank { "Untitled" }),
              query.isNotBlank() && note == matches.first(),
              Modifier.testTag("note-picker-result-${note.id}")
            ) { pick(note.id) }
          }
          if (creating) {
            item(key = "create-memo") {
              PickerRow(
                AnnotatedString("New memo: $query"),
                matches.isEmpty(),
                Modifier.testTag("note-picker-create-memo"),
                accent = true
              ) { create?.invoke(query, false) }
            }
            item(key = "create-action") {
              PickerRow(
                AnnotatedString("New action: $query"),
                false,
                Modifier.testTag("note-picker-create-action"),
                accent = true
              ) { create?.invoke(query, true) }
            }
          }
        }
      }
    }
  }
}

@Composable
private fun PickerRow(
  text: AnnotatedString,
  selected: Boolean,
  modifier: Modifier = Modifier,
  accent: Boolean = false,
  choose: () -> Unit
) {
  Text(
    text,
    modifier.fillMaxWidth().heightIn(min = 40.dp)
      .background(
        if (selected) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent,
        RoundedCornerShape(8.dp)
      )
      .clickable(onClick = choose).wrapContentHeight().padding(horizontal = 12.dp),
    color = if (accent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
    maxLines = 1,
    overflow = TextOverflow.Ellipsis
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
