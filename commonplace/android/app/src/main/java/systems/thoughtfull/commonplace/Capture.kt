// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import kotlinx.coroutines.launch
import systems.thoughtfull.commonplace.core.Answer
import systems.thoughtfull.commonplace.core.CapturedPrompt
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.QuestionKind

internal data class CaptureRequest(
  val body: String = "",
  val journal: String? = null,
  val kind: String? = null,
  val prompt: String? = null,
  val prompted: Long? = null
)

internal val captureSaver = listSaver<CaptureRequest?, String>(
  save = { request ->
    request?.let {
      listOf(
        it.body,
        it.journal.orEmpty(),
        it.kind.orEmpty(),
        it.prompt.orEmpty(),
        it.prompted?.toString().orEmpty()
      )
    }.orEmpty()
  },
  restore = { values ->
    values.takeIf { it.size == 5 }?.let {
      CaptureRequest(
        it[0],
        it[1].ifEmpty { null },
        it[2].ifEmpty { null },
        it[3].ifEmpty { null },
        it[4].toLongOrNull()
      )
    }
  }
)

private val answersSaver = listSaver<Map<String, Answer>, String>(
  save = { answers ->
    answers.toSortedMap().flatMap { (id, answer) ->
      val (type, value) = when (answer) {
        is Answer.Bool -> "bool" to answer.v1.toString()
        is Answer.Choices -> "choices" to answer.v1.joinToString(",")
        is Answer.Number -> "number" to answer.v1.toString()
        is Answer.Text -> "text" to answer.v1
      }
      listOf(id, type, value)
    }
  },
  restore = { values ->
    values.chunked(3).associate { (id, type, value) ->
      id to when (type) {
        "bool" -> Answer.Bool(value.toBoolean())
        "choices" -> Answer.Choices(value.split(','))
        "number" -> Answer.Number(value.toDouble())
        else -> Answer.Text(value)
      }
    }
  }
)

@Composable
internal fun CaptureDialog(
  model: NotebookModel,
  state: NotebookState,
  request: CaptureRequest,
  dismiss: () -> Unit,
  saved: (String) -> Unit
) {
  val preferences = model.repository.preferences
  val context = LocalContext.current
  DisposableEffect(request.prompt, request.journal) {
    if (request.prompt != null) {
      model.repository.answeringJournal = request.journal
      PromptScheduler.schedule(context)
    }
    onDispose {
      if (request.prompt != null) {
        model.repository.answeringJournal = null
        PromptScheduler.schedule(context)
      }
    }
  }
  val scope = rememberCoroutineScope()
  val focus = remember { FocusRequester() }
  LaunchedEffect(Unit) { focus.requestFocus() }
  val writableJournals = state.journals.filter { !it.readOnly }
  var kind by rememberSaveable {
    val savedKind = preferences.getString("capture.kind", "Memo")
    val available = listOfNotNull(
      "Action",
      "Entry".takeIf { writableJournals.isNotEmpty() },
      "Memo"
    )
    mutableStateOf(request.kind ?: savedKind?.takeIf { it in available } ?: "Memo")
  }
  var space by rememberSaveable {
    mutableStateOf(
      preferences.getString("capture.space", null)
        ?.takeIf { key -> state.spaces.any { it.key == key && !it.readOnly } }
        ?: state.spaces.firstOrNull { !it.readOnly }?.key.orEmpty()
    )
  }
  var journalId by rememberSaveable {
    mutableStateOf(
      request.journal ?: preferences.getString("capture.journal", null)
        ?.takeIf { id -> writableJournals.any { it.id == id } }
        ?: writableJournals.firstOrNull()?.id
    )
  }
  var requestedJournal by remember { mutableStateOf<MobileNote?>(null) }
  LaunchedEffect(request.journal) {
    request.journal?.let { id -> requestedJournal = model.read { it.note(id) } }
  }
  val journals = (state.journals + listOfNotNull(requestedJournal)).distinctBy { it.id }
  val journal = journals.find { it.id == journalId }
  val openedDate = rememberSaveable { LocalDate.now().toString() }
  fun template(note: MobileNote?) = "$openedDate\n\n" + note?.journal?.questions.orEmpty()
    .filter { it.retired == null && it.kind == QuestionKind.Text }
    .joinToString("") { "## ${it.prompt.trim()}\n\n" }
  var body by rememberSaveable {
    mutableStateOf(request.body.ifEmpty { if (kind == "Entry") template(journal) else "" })
  }
  var previousTemplate by rememberSaveable { mutableStateOf(template(journal)) }
  LaunchedEffect(journal?.id, journal?.journal?.questions) {
    val next = template(journal)
    if (kind == "Entry" && body == previousTemplate) body = next
    previousTemplate = next
  }
  var answers by rememberSaveable(stateSaver = answersSaver) {
    mutableStateOf<Map<String, Answer>>(emptyMap())
  }
  var invalid by remember { mutableStateOf<Set<String>>(emptySet()) }
  var saving by remember { mutableStateOf(false) }
  val valid = when (kind) {
    "Entry" ->
      journal != null && !journal.readOnly && !journal.inTrash &&
        journal.archived == null && invalid.isEmpty() &&
        (answers.isNotEmpty() || (body.isNotBlank() && body.trim() != template(journal).trim()))

    "Action", "Memo" -> space.isNotEmpty() && body.isNotBlank()

    else -> false
  }
  AlertDialog(
    onDismissRequest = { if (!saving) dismiss() },
    title = { Text("Capture") },
    text = {
      Column(Modifier.verticalScroll(rememberScrollState())) {
        if (request.prompt == null) {
          ChipRow {
            // Follow the navigation tab order.
            listOf("Memo", "Action", "Entry").forEach { option ->
              FilterChip(
                kind == option,
                onClick = {
                  if (option == "Entry" && body.isBlank()) body = template(journal)
                  kind = option
                },
                label = { Text(option) }
              )
            }
          }
        }
        if (kind == "Entry") {
          if (request.prompt != null) {
            Text(journal?.title ?: "Loading journal…")
          } else {
            ChipRow {
              journals.filter { !it.readOnly }.forEach { option ->
                FilterChip(journalId == option.id, onClick = {
                  if (body == template(journal)) body = template(option)
                  journalId = option.id
                  answers = emptyMap()
                  invalid = emptySet()
                }, label = { Text(option.title) })
              }
            }
          }
          if (journal == null) Text("Create a journal first.")
          journal?.journal?.questions?.filter {
            it.retired == null
          }?.forEach { question ->
            AnswerField(question, answers[question.id], validity = { valid ->
              invalid = if (valid) invalid - question.id else invalid + question.id
            }) { answer ->
              answers = if (answer == null) {
                answers - question.id
              } else {
                answers + (question.id to answer)
              }
            }
          }
        } else if (state.spaces.size > 1) {
          ChipRow {
            state.spaces.filter { !it.readOnly }
              .sortedBy { model.repository.name(it.key) }.forEach { option ->
                FilterChip(
                  space == option.key,
                  onClick = { space = option.key },
                  label = { Text(model.repository.name(option.key)) }
                )
              }
          }
        }
        OutlinedTextField(
          body,
          { body = it },
          label = { Text("Write in Markdown") },
          minLines = 5,
          modifier = Modifier.fillMaxWidth().padding(top = 8.dp).focusRequester(focus)
        )
      }
    },
    confirmButton = {
      TextButton(enabled = valid && !saving, onClick = {
        saving = true
        scope.launch {
          val id = model.read { core ->
            when (kind) {
              "Action" -> core.createAction(space, body)

              "Entry" -> core.createEntry(
                journalId!!,
                body,
                System.currentTimeMillis(),
                request.prompt?.let {
                  CapturedPrompt(
                    it,
                    request.prompted ?: System.currentTimeMillis()
                  )
                },
                answers,
                LocalDate.now().toString()
              )

              else -> core.createMemo(space, body)
            }
          }
          saving = false
          if (id != null) {
            preferences.edit().putString("capture.kind", kind)
              .putString("capture.space", if (kind == "Entry") journal!!.space else space)
              .putString("capture.journal", journalId).apply()
            model.refresh()
            saved(id)
          }
        }
      }) { Text(if (saving) "Saving…" else "Save") }
    },
    dismissButton = { TextButton(enabled = !saving, onClick = dismiss) { Text("Cancel") } }
  )
}
