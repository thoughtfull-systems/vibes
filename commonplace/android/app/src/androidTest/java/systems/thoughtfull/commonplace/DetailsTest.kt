// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.graphics.Bitmap
import android.os.SystemClock
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.Density
import androidx.test.espresso.Espresso
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.time.LocalDate
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.DayOffEffect
import systems.thoughtfull.commonplace.core.DaysOffUse
import systems.thoughtfull.commonplace.core.Missed
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.RecurrenceSettings
import systems.thoughtfull.commonplace.core.parseRule

class DetailsTest {
  @get:Rule
  val compose = createComposeRule()
  private val instrumentation = InstrumentationRegistry.getInstrumentation()
  private val context = instrumentation.targetContext
  private val application = context.applicationContext as CommonplaceApplication
  private val repository = application.repository
  private val spaces = mutableListOf<String>()
  private lateinit var model: NotebookModel

  private fun space(name: String): String {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    return runBlocking { repository.use { it.createSpace() } }.also {
      spaces.add(it)
      repository.preferences.edit().putString("name.$it", name).commit()
    }
  }

  private fun show(id: String) {
    model = NotebookModel(application)
    val selected = mutableStateOf(id)
    compose.setContent {
      CommonplaceTheme { NoteScreen(model, selected.value, {}, { selected.value = it }) }
    }
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithTag("note-editor").assertExists() }.isSuccess
    }
  }

  private fun saved(id: String, predicate: (MobileNote) -> Boolean) {
    compose.waitUntil(10_000) {
      model.state.value.notes.find { it.id == id }?.let(predicate) == true
    }
    compose.waitForIdle()
  }

  private fun clickControl(tag: String, toggle: Boolean = false) {
    val control = if (toggle) isToggleable() else hasClickAction()
    compose.onNode(control and hasAnyAncestor(hasTestTag(tag))).performScrollTo().performClick()
  }

  private fun screenshot(name: String) {
    compose.waitForIdle()
    // Window transitions can outlast Compose's idle state.
    SystemClock.sleep(300)
    val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
    try {
      File(context.cacheDir, "ui-details-$name.png").outputStream().use {
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
      }
    } finally {
      bitmap.recycle()
    }
  }

  @After
  fun removeFixtures() {
    runBlocking { repository.use { core -> spaces.forEach { core.removeSpace(it) } } }
    repository.preferences.edit().apply {
      spaces.forEach { remove("name.$it") }
    }.apply()
  }

  @Test
  fun memoDetailsMoveArchiveAndConvertWithoutAnExtraLinksSection() {
    val source = space("A memo details")
    val target = space("B memo details")
    val (memo, journal) = runBlocking {
      repository.use { core ->
        core.createMemo(source, "Time off\n\n[Holiday](https://example.com)") to
          core.createJournal(target, "Details journal")
      }
    }
    show(memo)
    compose.onNodeWithContentDescription("Note menu").performClick()
    compose.onNodeWithText("Archive").assertDoesNotExist()
    compose.onNodeWithText("Move to another library").assertDoesNotExist()
    Espresso.pressBack()
    compose.onNodeWithText("Details").performClick()
    compose.onNodeWithText("Links").assertDoesNotExist()
    compose.onNodeWithTag("detail-library-$target").performScrollTo().performClick()
    compose.onNodeWithText("Cancel").performClick()
    assertEquals(source, runBlocking { repository.use { it.note(memo)!!.space } })
    compose.onNodeWithTag("detail-library-$target").performScrollTo().performClick()
    compose.onNodeWithText("Move").performClick()
    saved(memo) { it.space == target }
    compose.onNodeWithTag("detail-archive").performScrollTo().performClick()
    saved(memo) { it.archived != null }
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithText("Unarchive").assertExists() }.isSuccess
    }
    compose.onNodeWithText("Unarchive").performClick()
    saved(memo) { it.archived == null }
    screenshot("memo")
    compose.onNodeWithText("Make journal entry").performScrollTo().performClick()
    compose.onNodeWithText("Details journal").performClick()
    compose.waitUntil(10_000) {
      model.state.value.notes.any { it.entry?.journal == journal && it.title == "Time off" }
    }
    assertNotNull(runBlocking { repository.use { it.note(memo)!!.archived } })
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithTag("note-details").assertDoesNotExist() }.isSuccess
    }
    compose.onNodeWithTag("note-editor").assertExists()
  }

  @Test
  fun actionDetailsKeepLinuxOrderAndParentMovesStayWithSteps() {
    val space = space("A action details")
    val (action, parent, destination) = runBlocking {
      repository.use { core ->
        val parent = core.createAction(space, "Original parent")
        val action = core.createStep(parent, "Action details test")
        core.createStep(action, "Child step")
        Triple(action, parent, core.createAction(space, "Destination parent"))
      }
    }
    show(action)
    compose.onNodeWithText("Original parent").assertExists()
    val parentTop = compose.onNodeWithTag("action-parent").fetchSemanticsNode().positionInRoot.y
    val stepsTop = compose.onNodeWithText("Steps").fetchSemanticsNode().positionInRoot.y
    assertTrue(parentTop < stepsTop)
    screenshot("parent")
    compose.onNodeWithText("Move…").performClick()
    compose.onNodeWithText("Destination parent").performClick()
    saved(action) { it.parent == destination }
    compose.onNodeWithText("Move…").performClick()
    compose.onNodeWithText("Top level").performClick()
    saved(action) { it.parent == null }
    compose.onNodeWithText("Details").performClick()
    // Field order follows the desktop panel rather than alphabetical order.
    val tags = listOf(
      "detail-library-$space",
      "detail-starred",
      "detail-done",
      "detail-list",
      "detail-project",
      "detail-starts",
      "detail-due",
      "detail-unclear",
      "detail-timestamps",
      "detail-archive"
    )
    val positions = tags.map { compose.onNodeWithTag(it).fetchSemanticsNode().positionInRoot.y }
    assertEquals(positions.sorted(), positions)
    compose.onNodeWithText("Change parent").assertDoesNotExist()
    compose.onNodeWithText("In planning queue").assertDoesNotExist()
    screenshot("action")
    clickControl("detail-done", toggle = true)
    saved(action) { it.action?.state == ActionState.DONE }
    clickControl("detail-done", toggle = true)
    saved(action) { it.action?.state == ActionState.OPEN }
    clickControl("detail-list")
    compose.onNodeWithText("Waiting").performClick()
    saved(action) { it.action?.list == ActionList.WAITING }
    compose.onNodeWithTag("waiting-for").performScrollTo().performTextInput("A supplier")
    saved(action) { it.action?.waitingFor == "A supplier" }
    Espresso.closeSoftKeyboard()
    clickControl("detail-list")
    compose.onNodeWithText("Someday").performClick()
    saved(action) { it.action?.list == ActionList.SOMEDAY }
    clickControl("detail-project", toggle = true)
    saved(action) { it.action?.project == false }
    clickControl("detail-starts")
    tapToday(compose)
    saved(action) { it.action?.defer == LocalDate.now().toString() }
    clickControl("detail-due")
    tapToday(compose)
    saved(action) { it.action?.due == LocalDate.now().toString() }
    clickControl("detail-unclear", toggle = true)
    saved(action) { it.action?.planning != null }
    clickControl("detail-starred")
    saved(action) { it.starred != null }
    compose.onNodeWithTag("detail-archive").performScrollTo().performClick()
    saved(action) { it.archived != null }
    assertNotNull(runBlocking { repository.use { it.note(parent) } })
  }

  @Test
  fun finishedRulesKeepHistoryAvailable() {
    val library = space("Finished repeat")
    val today = LocalDate.now().toString()
    val action = runBlocking {
      repository.use { core ->
        core.createAction(library, "Finished repeat").also { id ->
          val settings = RecurrenceSettings(
            checkNotNull(parseRule("FREQ=DAILY;COUNT=1")),
            today,
            Missed.ALL,
            0,
            0,
            DaysOffUse(DayOffEffect.IGNORE, DayOffEffect.IGNORE)
          )
          core.setRecurrence(id, settings, today)
          core.checkAction(id, true, today)
        }
      }
    }
    show(action)
    compose.onNodeWithText("Details").performClick()
    compose.onNodeWithText("History…").performScrollTo().performClick()
    compose.onNodeWithTag("history").assertExists()
    compose.onNodeWithText("Past").assertExists()
  }

  @Test
  fun nestedRepeatsDoNotExposeControlsForIgnoredOccurrences() {
    val library = space("Nested repeat")
    val today = LocalDate.now().toString()
    val inner = runBlocking {
      repository.use { core ->
        val outer = core.createAction(library, "Outer repeat")
        core.createStep(outer, "Inner repeat").also { inner ->
          val settings = RecurrenceSettings(
            checkNotNull(parseRule("FREQ=DAILY")),
            today,
            Missed.ALL,
            0,
            0,
            DaysOffUse(DayOffEffect.IGNORE, DayOffEffect.IGNORE)
          )
          listOf(outer, inner).forEach { core.setRecurrence(it, settings, today) }
        }
      }
    }
    show(inner)
    compose.onNodeWithText("Details").performClick()
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithText("Doesn't repeat").assertExists() }.isSuccess
    }
    compose.onNodeWithText("Doesn't repeat").performScrollTo().assertExists()
    compose.onNodeWithTag("detail-this-time").assertDoesNotExist()
  }

  @Test
  fun everyWeekdayRemainsSelectableInANarrowRepeatEditor() {
    space("Narrow repeat editor")
    model = NotebookModel(application)
    compose.setContent {
      val density = LocalDensity.current
      CompositionLocalProvider(LocalDensity provides Density(density.density * 1.5f)) {
        CommonplaceTheme {
          RepeatEditorDialog(
            model,
            null,
            checkNotNull(parseRule("FREQ=WEEKLY;BYDAY=MO")),
            LocalDate.now().toString(),
            true,
            DaysOffUse(DayOffEffect.IGNORE, DayOffEffect.IGNORE),
            {},
            {}
          )
        }
      }
    }
    compose.onNodeWithText("Sa").performScrollTo().assertIsDisplayed().performClick()
    compose.onNodeWithText("Su").performScrollTo().assertIsDisplayed().performClick()
  }

  @Test
  fun syncedOffsetsReplaceTheDisplayedValue() {
    space("Synced offset")
    val offset = mutableStateOf<Int?>(-3)
    compose.setContent {
      CommonplaceTheme {
        OffsetRow("synced", offset.value, -1, "days before", false) { offset.value = it }
      }
    }
    compose.onNodeWithText("3").assertExists()
    compose.runOnIdle { offset.value = -8 }
    compose.onNodeWithText("8").assertExists()
    compose.onNodeWithText("3").assertDoesNotExist()
  }
}
