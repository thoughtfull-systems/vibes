// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.MobileAction
import systems.thoughtfull.commonplace.core.MobileNote

internal fun visibleSteps(steps: List<MobileAction>, showDone: Boolean): List<MobileAction> {
  if (showDone) return steps
  var hiddenBelow: UInt? = null
  return steps.filter { step ->
    if (hiddenBelow?.let { step.depth > it } == true) {
      false
    } else {
      val done = step.note.action?.state == ActionState.DONE
      hiddenBelow = step.depth.takeIf { done }
      !done
    }
  }
}

@Composable
internal fun NoteBottomPanel(
  model: NotebookModel,
  note: MobileNote,
  backlinks: List<MobileNote>,
  open: (String) -> Unit,
  modifier: Modifier = Modifier
) {
  if (note.action == null && backlinks.isEmpty()) return
  val state by model.state.collectAsState()
  val colors = remember(state.notes) {
    state.notes.mapNotNull { item -> item.journal?.let { item.id to it.color } }.toMap()
  }
  val settings = model.repository.preferences
  var showDone by rememberSaveable(note.id) {
    mutableStateOf(settings.getBoolean("steps.showDone.${note.id}", false))
  }
  var steps by remember(note.id) { mutableStateOf(emptyList<MobileAction>()) }
  var stepText by rememberSaveable(note.id) { mutableStateOf("") }
  var adding by remember(note.id) { mutableStateOf(false) }
  var revealStep by remember(note.id) { mutableStateOf<String?>(null) }
  fun addStep() {
    if (adding || stepText.isBlank()) return
    adding = true
    val text = stepText.trim()
    var created: String? = null
    model.perform(complete = { success ->
      adding = false
      if (success) {
        revealStep = created
        if (stepText.trim() == text) stepText = ""
      }
    }) { created = it.createStep(note.id, text) }
  }
  var parentPicker by remember(note.id) { mutableStateOf(false) }
  val parents = remember(note.id, note.parent, state.notes) {
    val notes = state.notes.associateBy { it.id }
    val seen = mutableSetOf(note.id)
    buildList {
      var parent = note.parent
      while (parent != null && seen.add(parent)) {
        val ancestor = notes[parent] ?: break
        add(ancestor)
        parent = ancestor.parent
      }
    }.asReversed()
  }
  LaunchedEffect(note.id, state.revision) {
    if (note.action != null) {
      steps = model.read { it.actionSteps(note.id, LocalDate.now().toString()) }.orEmpty()
    }
  }
  val scroll = rememberScrollState()
  var viewport by remember { mutableStateOf(Rect.Zero) }
  val editable = !note.readOnly && !note.inTrash
  Column(modifier.testTag("note-bottom-panel")) {
    HorizontalDivider()
    if (note.action != null) {
      Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("action-parent"),
        verticalAlignment = Alignment.CenterVertically
      ) {
        Text("In", style = MaterialTheme.typography.bodySmall)
        Row(
          Modifier.weight(1f).horizontalScroll(rememberScrollState())
            .padding(horizontal = 8.dp),
          verticalAlignment = Alignment.CenterVertically
        ) {
          if (parents.isEmpty()) Text("Top level")
          parents.forEachIndexed { index, parent ->
            if (index > 0) Text(" › ")
            TextButton(onClick = { open(parent.id) }) { Text(formattedTitle(parent.title)) }
          }
        }
        if (editable) {
          TextButton(onClick = { parentPicker = true }) { Text("Move…") }
        }
      }
      HorizontalDivider()
    }
    Column(
      Modifier.weight(1f, fill = false).fillMaxWidth().verticalScroll(scroll)
        .testTag("step-scroll").onGloballyPositioned { viewport = it.boundsInRoot() }
    ) {
      if (note.action != null) {
        Row(
          Modifier.fillMaxWidth().padding(horizontal = 16.dp),
          verticalAlignment = Alignment.CenterVertically
        ) {
          Text("Steps", Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
          Text("Show done", style = MaterialTheme.typography.labelSmall)
          Checkbox(showDone, {
            showDone = it
            settings.edit().putBoolean("steps.showDone.${note.id}", it).apply()
          }, modifier = Modifier.testTag("steps-show-done"))
        }
        StepRows(
          model,
          visibleSteps(steps, showDone),
          editable,
          scroll,
          viewport,
          open,
          revealStep,
          { revealStep = null }
        )
      }
      if (backlinks.isNotEmpty()) {
        Text(
          "Linked From",
          Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
          style = MaterialTheme.typography.titleSmall
        )
        backlinks.forEach { linked ->
          NoteRow(
            linked,
            open,
            entryColor = colors[linked.entry?.journal] ?: "blue"
          )
        }
      }
    }
    if (editable && note.action != null) {
      OutlinedTextField(
        stepText,
        { stepText = it },
        placeholder = { Text("Add a step…") },
        singleLine = true,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { addStep() }),
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)
          .testTag("step-entry")
      )
    }
  }
  if (parentPicker) {
    MoveActionPicker(model, note, state.notes) { parentPicker = false }
  }
}
