// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.ime
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextInputSelection
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.test.espresso.Espresso
import androidx.test.platform.app.InstrumentationRegistry
import java.time.LocalDate
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionList

class EditingTest {
  @get:Rule
  val compose = createComposeRule()
  private val context = InstrumentationRegistry.getInstrumentation().targetContext
  private var space = ""

  @After
  fun removeFixtures() {
    if (space.isEmpty()) return
    runBlocking {
      (context.applicationContext as CommonplaceApplication).repository.use {
        it.removeSpace(space)
      }
    }
  }

  @Test
  fun theSingleScrollingToolbarKeepsTheCaretVisibleAndCoversTheBottomPanel() {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val repository = (context.applicationContext as CommonplaceApplication).repository
    val source = runBlocking {
      repository.use { core ->
        space = core.createSpace()
        val body = "A long action\n\n" + (1..80).joinToString("\n") { "Line $it" } +
          "\nCursor destination"
        val id = core.createAction(space, body)
        core.changeAction(id, ActionChange.List(ActionList.ACTIVE))
        core.createStep(id, "A child step")
        val referring = core.createMemo(space, "Referring memo")
        core.setBody(referring, "Referring memo\n\n" + core.linkText(referring, id))
        id
      }
    }
    repository.preferences.edit().putString("selected", source).putString("tab", "All notes")
      .putString("query", "").commit()
    val model = NotebookModel(context.applicationContext as CommonplaceApplication)
    val keyboardVisible = AtomicBoolean(false)
    compose.setContent {
      val shown = WindowInsets.ime.getBottom(LocalDensity.current) > 0
      SideEffect { keyboardVisible.set(shown) }
      CommonplaceTheme { CommonplaceApp(model, null) {} }
    }
    val editor = compose.onNodeWithTag("note-editor")
    compose.waitUntil(10_000) {
      runCatching {
        editor.fetchSemanticsNode().config[SemanticsProperties.EditableText]
          .contains("Cursor destination")
      }.getOrDefault(false)
    }
    compose.onNodeWithTag("note-bottom-panel").assertExists()
    editor.performClick()
    val length = editor.fetchSemanticsNode().config[SemanticsProperties.EditableText].length
    editor.performTextInputSelection(TextRange(length), relativeToOriginalText = false)
    editor.performTextInput(" at the end")
    compose.waitUntil(5_000) { keyboardVisible.get() }
    compose.onNodeWithTag("note-bottom-panel").assertDoesNotExist()
    val density = context.resources.displayMetrics.density
    val toolbar = compose.onNodeWithTag("editor-toolbar").fetchSemanticsNode().boundsInRoot
    assertEquals(48f * density, toolbar.height, 1f)
    compose.onNodeWithContentDescription("Checklist").performScrollTo().performClick()
    compose.waitUntil(5_000) {
      val layouts = mutableListOf<TextLayoutResult>()
      editor.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
      val layout = layouts.single()
      val caret = layout.getCursorRect(layout.layoutInput.text.length)
        .translate(editor.fetchSemanticsNode().positionInRoot)
      val viewport = compose.onNodeWithTag("editor-viewport").fetchSemanticsNode().boundsInRoot
      caret.top >= viewport.top && caret.bottom <= viewport.bottom && caret.bottom <= toolbar.top
    }
    assertTrue(keyboardVisible.get())
    Espresso.pressBack()
    compose.waitUntil(5_000) { !keyboardVisible.get() }
    compose.onNodeWithTag("editor-toolbar").assertDoesNotExist()
    compose.onNodeWithTag("note-bottom-panel").assertExists()
  }

  @Test
  fun actionTitlesAlignWithOtherNotesIncludingInTrash() {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val repository = (context.applicationContext as CommonplaceApplication).repository
    val notes = runBlocking {
      repository.use { core ->
        space = core.createSpace()
        val action = core.createAction(space, "An active action")
        val trashed = core.createAction(space, "A trashed action")
        core.trash(trashed)
        val memo = core.createMemo(space, "A memo")
        val journal = core.createJournal(space, "A journal")
        val entry = core.createEntry(
          journal,
          "An entry",
          System.currentTimeMillis(),
          null,
          emptyMap(),
          LocalDate.now().toString()
        )
        listOf(action, trashed, memo, journal, entry).map { core.note(it)!! }
      }
    }
    compose.setContent {
      CommonplaceTheme {
        Column {
          notes.forEach { note ->
            NoteRow(
              note,
              RowContext(notes),
              {},
              complete = if (note.trashed == null) ({}) else null
            )
          }
        }
      }
    }
    val titles = notes.map { note ->
      compose.onNodeWithText(note.title, useUnmergedTree = true)
        .fetchSemanticsNode().boundsInRoot
    }
    titles.forEach { assertEquals(titles.last().left, it.left, 1f) }
    val boxes = compose.onAllNodesWithTag("note-checkbox", useUnmergedTree = true)
      .fetchSemanticsNodes().map { it.boundsInRoot }
    compose.onAllNodesWithTag("note-kind-icon", useUnmergedTree = true)
      .fetchSemanticsNodes().forEachIndexed { index, icon ->
        assertEquals(titles[index + 2].center.y, icon.boundsInRoot.center.y, 1f)
      }
    boxes.forEachIndexed { index, box ->
      assertEquals(titles[index].center.y, box.center.y, 1f)
      assertEquals(
        8f * context.resources.displayMetrics.density,
        titles[index].left - box.right,
        1f
      )
    }
  }
}
