// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.os.SystemClock
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.unit.dp
import androidx.test.platform.app.InstrumentationRegistry
import java.time.LocalDate
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind
import systems.thoughtfull.commonplace.core.Schedule
import systems.thoughtfull.commonplace.core.Times

class RefinementsTest {
  @get:Rule val compose = createComposeRule()
  private val app = InstrumentationRegistry.getInstrumentation().targetContext.applicationContext
    as CommonplaceApplication
  private val repository = app.repository
  private val spaces = mutableListOf<String>()
  private val visibility = mutableMapOf<String, Boolean>()
  private val hidden = repository.preferences.getStringSet("journals.hidden", emptySet())!!.toSet()

  private fun space(): String {
    check(app.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    return runBlocking { repository.use { it.createSpace() } }.also { spaces.add(it) }
  }

  @After fun cleanup() {
    runBlocking { repository.use { core -> spaces.forEach { core.removeSpace(it) } } }
    val settings = repository.preferences.edit().putStringSet("journals.hidden", hidden)
    visibility.forEach { (key, value) -> settings.putBoolean("shown.$key", value) }
    spaces.forEach { settings.remove("name.$it") }
    settings.commit()
  }

  @Test fun draggingReordersAndNestsStepsWhileTheMoveRowStaysFixed() {
    val library = space()
    val (parent, a, b) = runBlocking {
      repository.use { core ->
        val parent = core.createAction(library, "Drag project")
        val a = core.createStep(parent, "First drag step")
        val b = core.createStep(parent, "Second drag step")
        repeat(14) { core.createStep(parent, "Extra step $it") }
        Triple(parent, a, b)
      }
    }
    val model = NotebookModel(app)
    var density = 1f
    compose.setContent {
      CommonplaceTheme {
        density = LocalDensity.current.density
        val state by model.state.collectAsState()
        state.notes.find { it.id == parent }?.let { note ->
          Column { NoteBottomPanel(model, note, emptyList(), {}, Modifier.height(560.dp)) }
        }
      }
    }
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithTag("step-drag-$b").assertExists() }.isSuccess
    }
    fun drag(source: String, destination: Offset) {
      val start = compose.onNodeWithTag(
        "step-drag-$source"
      ).fetchSemanticsNode().boundsInRoot.center
      val root = compose.onRoot()
      root.performTouchInput { down(start) }
      root.performTouchInput { moveTo(destination, delayMillis = 100) }
      compose.waitForIdle()
      val image = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()!!
      java.io.File(app.cacheDir, "ui-step-drag.png").outputStream().use {
        image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)
      }
      image.recycle()
      root.performTouchInput { up() }
    }
    val target = compose.onNodeWithTag("step-row-$b").fetchSemanticsNode().boundsInRoot
    drag(a, Offset(target.right - 16f, target.bottom - 4f))
    compose.waitUntil(10_000) {
      runBlocking {
        repository.use {
          it.actionSteps(parent, LocalDate.now().toString()).take(2).map { step -> step.note.id } ==
            listOf(b, a)
        }
      }
    }
    compose.waitUntil(5_000) {
      compose.onNodeWithTag("step-row-$b").fetchSemanticsNode().boundsInRoot.top <
        compose.onNodeWithTag("step-row-$a").fetchSemanticsNode().boundsInRoot.top
    }
    val parentRow = compose.onNodeWithTag("step-row-$b").fetchSemanticsNode().boundsInRoot
    drag(a, Offset(parentRow.left + 80f * density, parentRow.center.y))
    compose.waitUntil(10_000) {
      model.state.value.notes.find { it.id == a }?.parent == b
    }
    val before = compose.onNodeWithTag("action-parent").fetchSemanticsNode().boundsInRoot.top
    compose.onNodeWithTag("step-scroll").performTouchInput { swipeUp() }
    val after = compose.onNodeWithTag("action-parent").fetchSemanticsNode().boundsInRoot.top
    assertEquals(before, after, 0.5f)
    compose.onNodeWithText("Move…").assertExists()
  }

  @Test fun spaceFilteringDoesNotShiftControlsOrOfferSamples() {
    val library = space()
    repository.preferences.edit().putString("name.$library", "Filter fixture")
      .putString("tab", "Memos").putString("query", "").remove("selected")
      .putString("order.Memos", "Created").putString("from.Memos", "").commit()
    runBlocking {
      repository.use { core ->
        core.spaces().filter { it.key != library }.forEach {
          visibility[it.key] = repository.preferences.getBoolean("shown.${it.key}", true)
          repository.preferences.edit().putBoolean("shown.${it.key}", false).commit()
        }
        core.createMemo(library, "Existing filter data")
      }
    }
    val model = NotebookModel(app)
    compose.setContent { CommonplaceTheme { CommonplaceApp(model, null) {} } }
    compose.waitUntil(10_000) { model.state.value.loaded }
    val order = compose.onNodeWithTag("browse-order").fetchSemanticsNode().boundsInRoot
    val date = compose.onNodeWithTag("browse-date").fetchSemanticsNode().boundsInRoot
    assertTrue(
      "Filters share a row on a phone",
      kotlin.math.abs(order.center.y - date.center.y) < 2f
    )
    val before = compose.onNodeWithTag("search").fetchSemanticsNode().boundsInRoot.top
    compose.onNodeWithText("Filter fixture").performClick()
    compose.waitUntil(10_000) { model.state.value.spaces.none { it.shown } }
    compose.onNodeWithText("Load test library").assertDoesNotExist()
    assertEquals(
      before,
      compose.onNodeWithTag("search").fetchSemanticsNode().boundsInRoot.top,
      0.5f
    )
  }

  @Test fun entriesLinkToJournalsAndJournalControlsUseIcons() {
    val library = space()
    val (journal, other, entry) = runBlocking {
      repository.use { core ->
        val journal = core.createJournal(library, "Journal controls fixture")
        val other = core.createJournal(library, "Second journal fixture")
        core.setQuestion(journal, Question("one", "First question", QuestionKind.YesNo, null), true)
        core.setQuestion(
          journal,
          Question("two", "Second question", QuestionKind.YesNo, null),
          true
        )
        core.setSchedule(journal, Schedule("daily", "FREQ=DAILY", Times.Fixed(listOf(540u))))
        Triple(
          journal,
          other,
          core.createEntry(
            journal,
            "Linked entry fixture",
            System.currentTimeMillis(),
            null,
            emptyMap(),
            LocalDate.now().toString()
          )
        )
      }
    }
    val others = runBlocking { repository.use { it.journals().map { n -> n.id }.toSet() } } -
      setOf(journal, other)
    repository.preferences.edit().putStringSet(
      "journals.hidden",
      others
    ).putString("tab", "Journals")
      .putString("query", "").remove("selected").commit()
    val model = NotebookModel(app)
    compose.setContent { CommonplaceTheme { CommonplaceApp(model, null) {} } }
    compose.waitUntil(10_000) { model.state.value.loaded }
    compose.onNodeWithTag("journal-chart-hint").assertExists()
    compose.onNodeWithTag("note-row-$entry").performClick()
    compose.onNodeWithText("Details").performClick()
    compose.onNodeWithTag("entry-journal").performClick()
    compose.waitUntil(10_000) { repository.preferences.getString("selected", null) == journal }
    compose.onNodeWithText("Details").performClick()
    compose.onAllNodesWithContentDescription("Move down").assertCountEquals(2)
    compose.onAllNodesWithContentDescription("Move down").onFirst()
      .performScrollTo().performClick()
    compose.waitUntil(10_000) {
      model.state.value.journals.find { it.id == journal }?.journal?.questions?.first()?.id == "two"
    }
    compose.onAllNodesWithContentDescription("Edit").onLast().performScrollTo().performClick()
    compose.onNodeWithText("Edit schedule").assertExists()
    compose.onNodeWithText("Cancel").performClick()
  }
}
