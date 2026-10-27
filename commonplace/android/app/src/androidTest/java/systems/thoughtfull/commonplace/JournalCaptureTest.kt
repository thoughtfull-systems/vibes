// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.runtime.getValue
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.test.platform.app.InstrumentationRegistry
import java.time.LocalDate
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind

class JournalCaptureTest {
  @get:Rule
  val compose = createComposeRule()

  @Test
  fun newJournalsAskForANameAndLibraryThenOpenOutsideCapture() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val application = context.applicationContext as CommonplaceApplication
    val repository = application.repository
    val space = runBlocking { repository.use { it.createSpace() } }
    repository.preferences.edit().putString("name.$space", "Journal dialog test")
      .putString("tab", "Journals").remove("selected").putString("query", "").commit()
    val model = NotebookModel(application)
    var created: String? = null
    try {
      compose.setContent { CommonplaceTheme { CommonplaceApp(model, null) {} } }
      compose.waitUntil(10_000) { model.state.value.loaded }
      compose.onNodeWithTag("journals-menu").performClick()
      compose.onNodeWithText("New journal…").performClick()
      compose.onNodeWithText("Create").assertIsNotEnabled()
      compose.onNodeWithText("Write in Markdown").assertDoesNotExist()
      compose.onNodeWithText("Cancel").performClick()
      assertEquals(
        0,
        runBlocking {
          repository.use { core ->
            core.notes("").count { it.space == space && it.journal != null }
          }
        }
      )
      compose.onNodeWithTag("journals-menu").performClick()
      compose.onNodeWithText("New journal…").performClick()
      compose.onNodeWithTag("new-journal-name").performTextInput("A named journal")
      compose.onNodeWithTag("new-journal-library").performTouchInput { click(center) }
      compose.onAllNodesWithText("Journal dialog test").onLast().performClick()
      compose.onNodeWithText("Create").performClick()
      compose.waitUntil(10_000) {
        model.state.value.notes.any { it.space == space && it.title == "A named journal" }
      }
      val note = model.state.value.notes.single {
        it.space == space && it.title == "A named journal"
      }
      created = note.id
      assertNotNull(note.journal)
      compose.waitUntil(5_000) {
        runCatching { compose.onNodeWithTag("note-editor").assertExists() }.isSuccess
      }
      val body = compose.onNodeWithTag("note-editor").fetchSemanticsNode()
        .config[SemanticsProperties.EditableText].text
      assertEquals("A named journal", body.trim())
      compose.onNodeWithTag("new-journal-name").assertDoesNotExist()
      compose.onNodeWithContentDescription("Back").performClick()
      compose.onNodeWithContentDescription("New journal entry").assertDoesNotExist()
      compose.waitUntil(5_000) {
        runCatching { compose.onNodeWithTag("journals-menu").assertExists() }.isSuccess
      }
      assertEquals(
        1,
        runBlocking {
          repository.use { core ->
            core.notes("").count { it.space == space && it.journal != null }
          }
        }
      )
    } finally {
      runBlocking { repository.use { it.removeSpace(space) } }
      repository.preferences.edit().remove("name.$space").apply {
        created?.let { remove("prompts.$it") }
      }.apply()
    }
  }

  @Test
  fun anEntryWithOnlyStructuredAnswersHasAnEditableDateTitle() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val model = NotebookModel(context.applicationContext as CommonplaceApplication)
    val journal = runBlocking {
      model.repository.use { core ->
        core.createJournal(core.spaces().first().key, "Date title test").also { id ->
          core.setQuestion(id, Question("slept", "Slept well?", QuestionKind.YesNo, null), true)
        }
      }
    }
    var saved: String? = null
    try {
      compose.setContent {
        CommonplaceTheme {
          val state by model.state.collectAsStateWithLifecycle()
          CaptureDialog(
            model,
            state,
            CaptureRequest(kind = "Entry", journal = journal),
            {},
            { saved = it }
          )
        }
      }
      compose.waitUntil(10_000) {
        runCatching { compose.onNodeWithText("Write in Markdown").assertExists() }.isSuccess
      }
      val text = compose.onNodeWithText("Write in Markdown").fetchSemanticsNode()
        .config[SemanticsProperties.EditableText].text
      assertEquals(LocalDate.now().toString(), text.trim())
      compose.onNodeWithText("Save").assertIsNotEnabled()
      compose.onNodeWithText("Yes").performClick()
      compose.onNodeWithText("Save").performClick()
      compose.waitUntil(10_000) { saved != null }
      val title = runBlocking { model.repository.use { it.note(saved!!)!!.title } }
      assertEquals(LocalDate.now().toString(), title)
    } finally {
      runBlocking {
        model.repository.use { core ->
          core.trash(journal)
          core.delete(journal)
        }
      }
    }
  }
}
