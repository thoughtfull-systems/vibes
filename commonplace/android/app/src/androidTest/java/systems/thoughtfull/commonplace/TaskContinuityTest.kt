// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.content.Intent
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class TaskContinuityTest {
  @get:Rule val compose = createComposeRule()
  private val context = InstrumentationRegistry.getInstrumentation().targetContext
  private val application = context.applicationContext as CommonplaceApplication
  private val repository = application.repository
  private val spaces = mutableListOf<String>()

  private fun space(): String {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    return runBlocking { repository.use { it.createSpace() } }.also { spaces.add(it) }
  }

  @After fun cleanup() {
    runBlocking { repository.use { core -> spaces.forEach { core.removeSpace(it) } } }
  }

  @Test fun addingSeveralStepsKeepsTheParentOpenAndTheEntryFocused() {
    val library = space()
    val parent = runBlocking {
      repository.use { core ->
        core.createAction(library, "Keep working here").also { project ->
          repeat(12) { core.createStep(project, "Existing step $it") }
        }
      }
    }
    val model = NotebookModel(application)
    var opened: String? = null
    compose.setContent { CommonplaceTheme { NoteScreen(model, parent, {}, { opened = it }) } }
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithTag("step-entry").assertExists() }.isSuccess
    }
    listOf("First inline step", "Second inline step").forEach { text ->
      compose.onNodeWithTag("step-entry").performClick().performTextInput(text)
      compose.onNodeWithTag("step-entry").performImeAction()
      compose.waitUntil(10_000) { model.state.value.notes.any { it.title == text } }
      val added = model.state.value.notes.single { it.title == text }
      compose.waitUntil(10_000) {
        runCatching {
          val row = compose.onNodeWithTag("step-row-${added.id}")
            .fetchSemanticsNode().boundsInRoot
          val viewport = compose.onNodeWithTag("step-scroll").fetchSemanticsNode().boundsInRoot
          row.top >= viewport.top && row.bottom <= viewport.bottom
        }.getOrDefault(false)
      }
      compose.onNodeWithTag("step-row-${added.id}").assertIsDisplayed()
      compose.onNodeWithContentDescription("Add step").assertDoesNotExist()
      compose.onNodeWithTag("step-entry").assertIsFocused()
      compose.onNodeWithTag("note-editor").assertExists()
      assertEquals(null, opened)
    }
    Espresso.closeSoftKeyboard()
    compose.onNodeWithText("Move…").performClick()
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithTag("note-picker-search").assertIsFocused() }.isSuccess
    }
    compose.onNodeWithTag("note-picker-search").performClick()
    compose.onNodeWithTag("note-picker-top-level").assertExists()
    compose.onNodeWithTag("note-picker-search").performTextInput("First inline")
    compose.onNodeWithText("Cancel").performClick()
    assertEquals(null, runBlocking { repository.use { it.note(parent)!!.parent } })
  }

  @Test fun quickCapturePreservesTheListingAndSearch() {
    val library = space()
    repository.preferences.edit().putString(
      "capture.space",
      library
    ).putString("capture.kind", "Memo")
      .putString("tab", "Memos")
      .remove("selected").putString("query", "current task")
      .putBoolean("ascending.Memos", false).commit()
    val model = NotebookModel(application)
    val incoming = mutableStateOf<Intent?>(null)
    compose.setContent {
      CommonplaceTheme { CommonplaceApp(model, incoming.value) { incoming.value = null } }
    }
    compose.waitUntil(10_000) { model.state.value.loaded }
    compose.onNodeWithContentDescription("New memo").assertDoesNotExist()
    compose.runOnIdle { incoming.value = Intent(CAPTURE_ACTION) }
    compose.onNodeWithText("Write in Markdown").performTextInput("A quick unrelated thought")
    compose.onNodeWithText("Save").performClick()
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithText("Write in Markdown").assertDoesNotExist() }.isSuccess
    }
    compose.onNodeWithTag("note-editor").assertDoesNotExist()
    compose.onNodeWithTag("search").assertExists()
    assertEquals("current task", repository.preferences.getString("query", null))
    compose.onNodeWithContentDescription("Clear search").performClick()
    compose.waitUntil(10_000) { repository.preferences.getString("query", null) == "" }
    compose.onNodeWithContentDescription("Latest first").performClick()
    compose.onNodeWithContentDescription("Earliest first").assertExists()
  }

  @Test fun movingAcrossLibrariesWarnsBeforeChangingTheParent() {
    val source = space()
    val target = space()
    val (note, parent) = runBlocking {
      repository.use { core ->
        core.createAction(source, "Move this") to core.createAction(target, "Other library parent")
      }
    }
    val model = NotebookModel(application)
    val picker = mutableStateOf(true)
    val current = runBlocking { repository.use { it.note(note)!! } }
    compose.setContent {
      CommonplaceTheme {
        val state by model.state.collectAsState()
        if (state.loaded && picker.value) {
          MoveActionPicker(model, current, state.notes) {
            picker.value = false
          }
        }
      }
    }
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithTag("note-picker-search").assertExists() }.isSuccess
    }
    compose.onNodeWithTag("note-picker-search").performTextInput("Other library parent")
    compose.onNodeWithTag("note-picker-result-$parent").performClick()
    compose.onNodeWithText("Edits to the note on another device that haven't synced yet are lost.")
      .assertExists()
    assertEquals(source, runBlocking { repository.use { it.note(note)!!.space } })
    compose.onNodeWithText("Move").performClick()
    compose.waitUntil(10_000) {
      model.state.value.notes.find { it.id == note }?.parent == parent
    }
    assertEquals(target, runBlocking { repository.use { it.note(note)!!.space } })
  }
}
