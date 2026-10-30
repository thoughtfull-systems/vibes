// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package systems.thoughtfull.commonplace

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import kotlinx.coroutines.launch
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionDates
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.MobileLink
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.noteUri
import systems.thoughtfull.commonplace.core.wordCount

@Composable
internal fun NoteScreen(
  model: NotebookModel,
  id: String,
  back: () -> Unit,
  open: (String) -> Unit,
  tabs: @Composable () -> Unit = {}
) {
  val state by model.state.collectAsState()
  val scope = rememberCoroutineScope()
  val context = LocalContext.current
  val focusManager = LocalFocusManager.current
  val keyboard = LocalSoftwareKeyboardController.current
  val editorFocus = remember { FocusRequester() }
  var note by remember(id) { mutableStateOf<MobileNote?>(null) }
  var loaded by remember(id) { mutableStateOf(false) }
  var value by remember(id) { mutableStateOf(TextFieldValue("")) }
  var selectionStart by rememberSaveable(id) { mutableStateOf(0) }
  var selectionEnd by rememberSaveable(id) { mutableStateOf(0) }
  var initialized by remember(id) { mutableStateOf(false) }
  var saveError by remember(id) { mutableStateOf(false) }
  var backlinks by remember(id) { mutableStateOf<List<MobileNote>>(emptyList()) }
  var picker by remember { mutableStateOf(false) }
  var linkStart by remember { mutableStateOf<Int?>(null) }
  var deleting by remember { mutableStateOf(false) }
  var moving by remember { mutableStateOf<String?>(null) }
  var details by rememberSaveable(id) { mutableStateOf(false) }
  var parentPicker by rememberSaveable(id) { mutableStateOf(false) }
  var menu by remember { mutableStateOf(false) }
  var editorActive by remember { mutableStateOf(false) }
  LaunchedEffect(id, state.revision) {
    note = model.read { it.note(id) }
    loaded = true
    backlinks = model.read { it.backlinks(id) }.orEmpty()
    if (!initialized && note != null) {
      model.read { it.editorBody(id) }?.let {
        val body = model.draft(id) ?: it
        value = TextFieldValue(
          body,
          TextRange(selectionStart.coerceIn(0, body.length), selectionEnd.coerceIn(0, body.length))
        )
        saveError = model.saveFailed(id)
        initialized = true
      }
    } else if (initialized && model.draft(id) == null) {
      // Another device's changes can arrive while the note is open, and saving the text from
      // before them would undo them.
      model.read { it.editorBody(id) }?.takeIf { it != value.text }?.let { body ->
        if (model.draft(id) == null) {
          value = TextFieldValue(
            body,
            TextRange(
              value.selection.start.coerceAtMost(body.length),
              value.selection.end.coerceAtMost(body.length)
            )
          )
        }
      }
    }
  }
  fun update(next: TextFieldValue) {
    val changed = value.text != next.text
    value = next
    selectionStart = next.selection.start
    selectionEnd = next.selection.end
    if (changed && next.selection.collapsed && next.selection.start >= 2 &&
      next.text.substring(0, next.selection.start).endsWith("[[")
    ) {
      linkStart = next.selection.start - 2
      picker = true
    }
    if (changed) {
      saveError = false
      model.saveBody(id, next.text) { updated ->
        if (value.text == next.text) {
          note = updated ?: note
          saveError = updated == null
        }
      }
    }
  }
  fun leave() {
    if (!saveError) {
      keyboard?.hide()
      model.refresh()
      back()
    }
  }
  fun follow(link: MobileLink) {
    if (link.note != null) {
      model.refresh()
      open(link.note)
    } else if (link.uri != null) {
      runCatching {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(link.uri)))
      }.onFailure(model::report)
    }
  }
  BackHandler { leave() }
  val current = note
  val editable = initialized && current != null && !current.readOnly && !current.inTrash
  val keyboardVisible = WindowInsets.ime.getBottom(LocalDensity.current) > 0
  val toolsVisible = editorActive && editable && keyboardVisible
  Scaffold(
    modifier = Modifier.imePadding(),
    topBar = {
      TopAppBar(
        title = {
          Text(
            formattedTitle(current?.title?.ifBlank { "Note" } ?: "Note").text,
            maxLines = 1,
            overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
            style = MaterialTheme.typography.titleMedium
          )
        },
        expandedHeight = BAR_HEIGHT,
        navigationIcon = {
          IconButton(onClick = ::leave) { AppIcon(R.drawable.back, "Back") }
        },
        actions = {
          TextButton(onClick = {
            focusManager.clearFocus()
            keyboard?.hide()
            editorActive = false
            details = true
          }, enabled = current != null) { Text("Details") }
          Box {
            IconButton(onClick = { menu = true }) { AppIcon(R.drawable.more, "Note menu") }
            DropdownMenu(menu, onDismissRequest = { menu = false }) {
              DropdownMenuItem(text = { Text("Copy link") }, onClick = {
                context.getSystemService(ClipboardManager::class.java)
                  .setPrimaryClip(ClipData.newPlainText("Commonplace note", noteUri(id)))
                menu = false
              })
              if (current != null && !current.readOnly && !current.inTrash) {
                DropdownMenuItem(text = { Text("Move to trash") }, onClick = {
                  model.perform { it.trash(id) }
                  menu = false
                })
              }
              if (current?.trashed != null && !current.readOnly) {
                DropdownMenuItem(text = { Text("Delete permanently") }, onClick = {
                  deleting = true
                  menu = false
                })
              }
            }
          }
        }
      )
    },
    bottomBar = {
      if (toolsVisible) {
        EditorToolbar(value, {
          update(it)
          editorFocus.requestFocus()
          keyboard?.show()
        }, editable) {
          linkStart = null
          picker = true
        }
      } else if (!keyboardVisible) {
        tabs()
      }
    }
  ) { padding ->
    Column(Modifier.padding(padding).fillMaxSize()) {
      if (current == null) {
        Text(
          if (loaded) "This note isn't on this device, or it was deleted." else "Loading note…",
          Modifier.padding(20.dp)
        )
      } else {
        if (current.readOnly) ReadOnlyBanner()
        if (current.inTrash && !current.readOnly) {
          Row(
            Modifier.padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically
          ) {
            Text(
              if (current.trashed != null) {
                "This note is in the trash"
              } else {
                "In the trash with a project above it, which restoring brings back too"
              },
              Modifier.weight(1f),
              color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            TextButton(onClick = { model.perform { it.restore(id) } }) { Text("Restore") }
          }
        }
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
          val automaticLimit = maxHeight * 0.4f
          val panelLimit = maxHeight - 120.dp
          val divider = remember(id) { PanelDivider() }
          Column(Modifier.fillMaxSize().panelDividerDrag(divider)) {
            MarkdownEditor(
              value,
              editable,
              ::update,
              editorFocus,
              { editorActive = it },
              Modifier.weight(1f).fillMaxWidth(),
              ::follow
            )
            if (saveError) {
              TextButton(onClick = {
                model.saveBody(id, value.text) { updated ->
                  note = updated ?: note
                  saveError = updated == null
                }
              }) { Text("Retry save") }
            } else {
              Text(
                "${wordCount(value.text)} words",
                Modifier.align(Alignment.End).padding(horizontal = 16.dp, vertical = 4.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.labelSmall
              )
            }
            if (!toolsVisible) {
              NoteBottomPanel(
                model,
                current,
                backlinks,
                open,
                automaticLimit,
                panelLimit,
                {
                  focusManager.clearFocus()
                  keyboard?.hide()
                  editorActive = false
                  parentPicker = true
                },
                Modifier.fillMaxWidth(),
                divider
              )
            }
          }
          PanelUnpin(divider)
        }
      }
    }
  }
  if (details && current != null) {
    ModalBottomSheet(
      onDismissRequest = { details = false },
      sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ) {
      Column(
        Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(20.dp)
          .testTag("note-details"),
        verticalArrangement = Arrangement.spacedBy(12.dp)
      ) {
        Text("Details", style = MaterialTheme.typography.headlineSmall)
        if (current.entry == null && state.spaces.size > 1 && !current.readOnly) {
          Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
          ) {
            state.spaces.sortedBy { model.repository.name(it.key) }.forEach { space ->
              FilterChip(
                selected = current.space == space.key,
                onClick = { if (current.space != space.key) moving = space.key },
                enabled = !current.inTrash && !space.readOnly,
                label = { Text(model.repository.name(space.key)) },
                modifier = Modifier.testTag("detail-library-${space.key}")
              )
            }
          }
        }
        if (!current.inTrash && !current.readOnly) {
          if (current.journal == null && current.entry == null) {
            ActionField("Starred?", "detail-starred") {
              StarButton(current.starred != null) { toggleStar(model, current) }
            }
          }
          current.action?.let { ActionDetails(model, current) }
          current.journal?.let { JournalDetails(model, current) }
          current.entry?.let {
            EntryDetails(model, current) { journal ->
              details = false
              open(journal)
            }
          }
          if (current.action == null && current.journal == null && current.entry == null &&
            state.journals.any { it.space == current.space && it.archived == null && !it.inTrash }
          ) {
            JournalEntryMenu(model, current) { entry ->
              details = false
              open(entry)
            }
          }
        }
        Text(
          "Created ${dateLabel(current.created)}\nModified ${dateLabel(current.modified)}",
          Modifier.testTag("detail-timestamps"),
          style = MaterialTheme.typography.bodySmall
        )
        if (!current.inTrash && !current.readOnly) {
          Button(
            onClick = { model.perform { it.setArchived(id, current.archived == null) } },
            modifier = Modifier.fillMaxWidth().testTag("detail-archive"),
            colors = ButtonDefaults.buttonColors(
              containerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
              contentColor = MaterialTheme.colorScheme.onSurface
            )
          ) { Text(if (current.archived == null) "Archive" else "Unarchive") }
        }
      }
    }
  }
  if (picker) {
    fun insertLink(target: String) {
      scope.launch {
        model.read { it.linkText(id, target) }?.let { insert ->
          val start = linkStart ?: value.selection.min
          update(
            TextFieldValue(
              value.text.replaceRange(start, value.selection.max, insert),
              TextRange(start + insert.length)
            )
          )
          picker = false
        }
      }
    }
    NotePicker(state.notes.filter { !it.inTrash && it.id != id }, {
      picker = false
      linkStart = null
    }, create = { title, action ->
      scope.launch {
        model.read {
          if (action) it.createAction(note!!.space, title) else it.createMemo(note!!.space, title)
        }?.let { target ->
          insertLink(target)
          model.refresh()
        }
      }
    }, pick = ::insertLink)
  }
  if (parentPicker && current != null) {
    MoveActionPicker(model, current, state.notes) { parentPicker = false }
  }
  if (deleting) {
    ConfirmDialog(
      "Delete this note permanently?",
      "Commonplace deletes this note permanently, including its steps or journal entries, " +
        "on every device it syncs with.",
      { deleting = false },
      confirmLabel = "Delete permanently"
    ) {
      scope.launch {
        if (model.read {
            it.delete(id)
            true
          } == true
        ) {
          deleting = false
          leave()
        }
      }
    }
  }
  moving?.let { target ->
    ConfirmDialog(
      "Move to ${model.repository.name(target)}?",
      "Edits to the note on another device that haven't synced yet are lost.",
      { moving = null },
      confirmLabel = "Move"
    ) {
      model.perform { it.moveToSpace(id, target) }
      moving = null
    }
  }
}

@Composable
internal fun ActionDetails(model: NotebookModel, note: MobileNote) {
  val state by model.state.collectAsState()
  val action = note.action!!
  val today = LocalDate.now().toString()
  var dates by remember(note.id) { mutableStateOf<ActionDates?>(null) }
  LaunchedEffect(note.id, state.revision, today) {
    dates = model.read { it.actionDates(note.id, today) }
  }
  var dateField by remember(note.id) { mutableStateOf<String?>(null) }
  var waitingText by remember(note.id) { mutableStateOf(action.waitingFor.orEmpty()) }
  fun change(value: ActionChange) {
    model.perform { it.changeAction(note.id, value) }
  }
  Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
    // Match the field order and control column in the Linux details panel.
    ActionField("Done?", "detail-done") {
      Switch(action.state == ActionState.DONE, {
        change(ActionChange.State(if (it) ActionState.DONE else ActionState.OPEN))
      })
    }
    ActionField("List", "detail-list") {
      // Keep the choices in the same order as the Linux list dropdown.
      val lists = listOf(
        ActionList.INBOX,
        ActionList.ACTIVE,
        ActionList.WAITING,
        ActionList.SOMEDAY,
        ActionList.MAYBE
      )
      MenuControl(
        action.list,
        lists.filter { it != ActionList.INBOX || action.list == ActionList.INBOX },
        { it.name.lowercase().replaceFirstChar { letter -> letter.uppercase() } }
      ) { change(ActionChange.List(it)) }
    }
    if (action.list == ActionList.WAITING) {
      ActionField("Waiting for", "detail-waiting") {
        OutlinedTextField(
          waitingText,
          {
            waitingText = it
            change(ActionChange.WaitingFor(it.trim().ifBlank { null }))
          },
          placeholder = { Text("Who or what") },
          singleLine = true,
          modifier = Modifier.testTag("waiting-for")
        )
      }
    }
    ActionField("Project?", "detail-project") {
      Switch(action.project, { change(ActionChange.Project(it)) })
    }
    ActionField("Starts", "detail-starts") {
      TextButton(onClick = { dateField = "Starts" }) {
        Text(action.defer?.let { formatDate(it) } ?: "None")
      }
    }
    ActionField("Due", "detail-due") {
      TextButton(onClick = { dateField = "Due" }) {
        Text(action.due?.let { formatDate(it) } ?: "None")
      }
    }
    val context = rememberRowContext(state)
    val inherited = dates?.let { inheritedDates(note.id, it, context::label) }.orEmpty()
    if (inherited.isNotEmpty()) {
      Text(
        inherited,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        style = MaterialTheme.typography.bodySmall
      )
    }
    if (action.state == ActionState.OPEN) {
      ActionField("Unclear?", "detail-unclear") {
        Switch(action.planning != null, { change(ActionChange.Planning(it)) })
      }
    }
  }
  dateField?.let { field ->
    CalendarDialog(
      (if (field == "Due") action.due else action.defer)?.let(LocalDate::parse),
      { dateField = null }
    ) {
      val date = it?.toString()
      change(if (field == "Due") ActionChange.Due(date) else ActionChange.Defer(date))
      dateField = null
    }
  }
}

@Composable
private fun ActionField(label: String, tag: String, control: @Composable () -> Unit) {
  Row(
    Modifier.fillMaxWidth().testTag(tag),
    horizontalArrangement = Arrangement.spacedBy(12.dp),
    verticalAlignment = Alignment.CenterVertically
  ) {
    Text(label, Modifier.width(88.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
    Box(Modifier.weight(1f)) { control() }
  }
}

@Composable
internal fun ToggleRow(
  label: String,
  checked: Boolean,
  enabled: Boolean = true,
  change: (Boolean) -> Unit
) {
  Row(
    Modifier.fillMaxWidth(),
    horizontalArrangement = Arrangement.SpaceBetween,
    verticalAlignment = Alignment.CenterVertically
  ) {
    Text(label, Modifier.weight(1f).padding(end = 12.dp))
    Switch(checked, change, enabled = enabled)
  }
}

@Composable
internal fun TextInputDialog(
  title: String,
  initial: String,
  hint: String,
  dismiss: () -> Unit,
  valid: (String) -> Boolean = { true },
  message: String? = null,
  save: (String) -> Unit
) {
  var text by rememberSaveable { mutableStateOf(initial) }
  AlertDialog(onDismissRequest = dismiss, title = { Text(title) }, text = {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
      message?.let { Text(it) }
      OutlinedTextField(text, { text = it }, label = { Text(hint) }, isError = !valid(text))
    }
  }, confirmButton = {
    TextButton(enabled = valid(text), onClick = { save(text) }) { Text("Save") }
  }, dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } })
}

@Composable
internal fun ConfirmDialog(
  title: String,
  body: String,
  dismiss: () -> Unit,
  confirmLabel: String = "Confirm",
  confirm: () -> Unit
) {
  AlertDialog(
    onDismissRequest = dismiss,
    title = { Text(title) },
    text = { Text(body) },
    confirmButton = { TextButton(onClick = confirm) { Text(confirmLabel) } },
    dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } }
  )
}
