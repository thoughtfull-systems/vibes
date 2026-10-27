// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.graphics.Bitmap
import android.os.SystemClock
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.espresso.Espresso
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.time.LocalDate
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.ActionView

class InboxTest {
  @get:Rule
  val compose = createComposeRule()
  private lateinit var model: NotebookModel
  private var space = ""
  private val visibility = mutableMapOf<String, Boolean>()
  private val skipped = mutableSetOf<String>()

  @Before
  fun openInbox() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val repository = (context.applicationContext as CommonplaceApplication).repository
    runBlocking {
      repository.use { core ->
        core.spaces().forEach {
          visibility[it.key] = repository.preferences.getBoolean("shown.${it.key}", true)
          repository.preferences.edit().putBoolean("shown.${it.key}", false).commit()
        }
        space = core.createSpace()
        val parent = core.createAction(space, "Inbox test project")
        core.changeAction(parent, ActionChange.List(ActionList.ACTIVE))
        repeat(8) { core.createAction(space, "Inbox test item $it") }
      }
    }
    repository.preferences.edit().putString("tab", "Actions").putString("query", "")
      .putString("action.mode", "Inbox").remove("selected").commit()
    model = NotebookModel(context.applicationContext as CommonplaceApplication)
    compose.setContent { CommonplaceTheme { CommonplaceApp(model, null) {} } }
    compose.waitUntil(10_000) { model.state.value.actions[ActionView.INBOX]?.size == 8 }
  }

  @After
  fun removeFixtures() {
    if (!::model.isInitialized) return
    runBlocking {
      model.repository.use { core ->
        if (space.isNotEmpty()) core.removeSpace(space)
        visibility.forEach { (key, shown) ->
          model.repository.preferences.edit().putBoolean("shown.$key", shown).commit()
          core.showSpace(key, shown)
        }
      }
    }
  }

  private fun current(): String {
    val note = model.state.value.actions[ActionView.INBOX]!!.first { it.note.id !in skipped }.note
    compose.waitUntil(10_000) {
      runCatching {
        compose.onNodeWithTag("note-editor").fetchSemanticsNode()
          .config[SemanticsProperties.EditableText].text == note.body
      }.getOrDefault(false)
    }
    return note.id
  }

  private fun click(label: String) {
    val button = compose.onNode(hasText(label) and hasAnyAncestor(hasTestTag("inbox-processor")))
    if (label in listOf("Done", "Archive", "Trash", "Skip")) {
      button.assertIsDisplayed().performClick()
    } else {
      button.performScrollTo().performClick()
    }
  }

  private fun awaitProcessed(id: String) {
    compose.waitUntil(10_000) {
      model.state.value.actions[ActionView.INBOX].orEmpty().none { it.note.id == id }
    }
  }

  private fun screenshot(name: String) {
    compose.waitForIdle()
    SystemClock.sleep(300)
    val instrumentation = InstrumentationRegistry.getInstrumentation()
    val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
    try {
      File(instrumentation.targetContext.cacheDir, "ui-$name.png").outputStream().use {
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
      }
    } finally {
      bitmap.recycle()
    }
  }

  @Test
  fun processingButtonsStayAboveNavigationWhileTheItemScrolls() {
    current()
    compose.onNodeWithText("Someday").performScrollTo()
    val navigation = compose.onNodeWithText("Memos").fetchSemanticsNode().boundsInRoot
    val footer = compose.onNodeWithTag("inbox-processing-actions").fetchSemanticsNode().boundsInRoot
    assertTrue(footer.bottom <= navigation.top)
    listOf("Done", "Archive", "Trash", "Skip").forEach { label ->
      compose.onNode(hasText(label) and hasAnyAncestor(hasTestTag("inbox-processing-actions")))
        .assertIsDisplayed()
    }
    val trash = compose.onNode(
      hasText("Trash") and hasAnyAncestor(hasTestTag("inbox-processing-actions"))
    ).fetchSemanticsNode().boundsInRoot
    val skip = compose.onNodeWithText("Skip").fetchSemanticsNode().boundsInRoot
    assertTrue(skip.left > trash.right)
    screenshot("inbox-pinned-actions")
  }

  @Test
  fun keepsTheCurrentItemVisibleWhenProcessingFails() {
    val id = current()
    runBlocking {
      model.repository.use {
        it.trash(id)
        it.delete(id)
      }
    }
    click("Active")
    compose.onNodeWithText("Commonplace couldn't save or load").assertExists()
    compose.onNodeWithText("Close").performClick()
    compose.onNodeWithTag("inbox-processor").assertExists()
    compose.onNodeWithTag("note-editor").assertExists()
    assertEquals(8, model.state.value.actions[ActionView.INBOX]!!.size)
  }

  @Test
  fun processesItemsOneAtATimeAndRestartsSkippedItems() {
    val skippedId = current()
    click("Skip")
    skipped.add(skippedId)
    val active = current()
    screenshot("inbox")
    compose.onNode(isToggleable()).performScrollTo().performClick()
    compose.onNodeWithTag("inbox-due").performScrollTo().performClick()
    tapToday(compose)
    click("Move…")
    compose.onNodeWithTag("note-picker-search").performTextInput("Inbox test project")
    val parent = model.state.value.notes.single {
      it.space == space &&
        it.title == "Inbox test project"
    }
    compose.onNodeWithTag("note-picker-result-${parent.id}").performClick()
    compose.waitUntil(10_000) {
      model.state.value.notes.find { it.id == active }?.parent != null
    }
    val editor = compose.onNodeWithTag("note-editor")
    editor.performScrollTo().performClick()
    editor.performTextClearance()
    editor.performTextInput("Edited in the Inbox")
    screenshot("inbox-keyboard")
    Espresso.pressBack()
    click("Active")
    awaitProcessed(active)
    runBlocking {
      model.repository.use {
        val note = it.note(active)!!
        assertEquals("Edited in the Inbox", note.body)
        assertEquals(ActionList.ACTIVE, note.action!!.list)
        assertEquals(true, note.action!!.project)
        assertNotNull(note.action!!.due)
        assertNotNull(note.parent)
      }
    }
    val waiting = current()
    click("Waiting…")
    compose.onNodeWithTag("inbox-waiting").performScrollTo().performTextInput("The test supplier")
    compose.onNodeWithTag("inbox-waiting").performImeAction()
    awaitProcessed(waiting)
    val scheduled = current()
    click("Schedule…")
    tapToday(compose)
    awaitProcessed(scheduled)
    val someday = current()
    click("Someday")
    awaitProcessed(someday)
    val maybe = current()
    click("Maybe")
    awaitProcessed(maybe)
    val done = current()
    click("Done")
    awaitProcessed(done)
    val archived = current()
    click("Archive")
    awaitProcessed(archived)
    compose.onNodeWithText("Nothing Left to Sort").assertExists()
    compose.onNodeWithText("Start over").performClick()
    skipped.clear()
    assertEquals(skippedId, current())
    click("Trash")
    awaitProcessed(skippedId)
    compose.onNodeWithText("Inbox Is Empty").assertExists()
    runBlocking {
      model.repository.use {
        assertEquals("The test supplier", it.note(waiting)!!.action!!.waitingFor)
        assertEquals(ActionList.WAITING, it.note(waiting)!!.action!!.list)
        assertEquals(LocalDate.now().toString(), it.note(scheduled)!!.action!!.defer)
        assertEquals(ActionList.ACTIVE, it.note(scheduled)!!.action!!.list)
        assertEquals(ActionList.SOMEDAY, it.note(someday)!!.action!!.list)
        assertEquals(ActionList.MAYBE, it.note(maybe)!!.action!!.list)
        assertEquals(ActionState.DONE, it.note(done)!!.action!!.state)
        assertNotNull(it.note(archived)!!.archived)
        assertEquals(true, it.note(skippedId)!!.inTrash)
      }
    }
  }
}
