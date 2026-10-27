// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.graphics.Bitmap
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.Answer
import systems.thoughtfull.commonplace.core.ChoiceOption
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind

class EntryControlsTest {
  @get:Rule val compose = createComposeRule()
  private val instrumentation = InstrumentationRegistry.getInstrumentation()
  private val context = instrumentation.targetContext
  private val application = context.applicationContext as CommonplaceApplication
  private val repository = application.repository
  private val zone = ZoneId.systemDefault()
  private val yesterday = LocalDate.now().minusDays(1)
  private lateinit var entry: String
  private lateinit var model: NotebookModel
  private lateinit var space: String

  @Before fun setup() {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    runBlocking {
      repository.use { core ->
        space = core.createSpace()
        val journal = core.createJournal(space, "Entry controls journal")
        core.setJournalColor(journal, "purple")
        listOf(
          Question("mood", "Mood", QuestionKind.Scale(1, 5, "Awful", "Great"), null),
          Question("walk", "Walk?", QuestionKind.YesNo, null),
          Question(
            "weather",
            "Weather",
            QuestionKind.Choice(
              false,
              listOf(ChoiceOption("sun", "Sunny", null), ChoiceOption("rain", "Rainy", null))
            ),
            null
          )
        ).forEach { core.setQuestion(journal, it, true) }
        entry = core.createEntry(
          journal,
          "Entry controls",
          yesterday.atTime(9, 7).atZone(zone).toInstant().toEpochMilli(),
          null,
          emptyMap(),
          LocalDate.now().toString()
        )
      }
    }
    model = NotebookModel(application)
    compose.setContent { CommonplaceTheme { NoteScreen(model, entry, {}, {}) } }
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithText("Details").assertIsEnabled() }.isSuccess
    }
    compose.onNodeWithText("Details").performClick()
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithText("Mood").assertExists() }.isSuccess
    }
  }

  @After fun cleanup() {
    runBlocking { repository.use { it.removeSpace(space) } }
  }

  private fun at() = Instant.ofEpochMilli(
    model.state.value.notes.single { it.id == entry }.entry!!.at
  ).atZone(zone)

  @Test fun dateAndTimeEditIndependentlyWithLabelsBesideTheirControls() {
    compose.onNodeWithText("Journal").assertExists()
    compose.onNodeWithText("When").assertExists()
    compose.onNodeWithTag("entry-time").performScrollTo().performTextClearance()
    compose.onNodeWithTag("entry-time").performTextInput("14:35")
    compose.onNodeWithTag("entry-time").performImeAction()
    compose.waitUntil(10_000) { at().toLocalTime() == LocalTime.of(14, 35) }
    assertEquals(yesterday, at().toLocalDate())
    compose.onNodeWithTag("entry-day").performScrollTo().performClick()
    tapToday(compose)
    compose.waitUntil(10_000) { at().toLocalDate() == LocalDate.now() }
    assertEquals(LocalTime.of(14, 35), at().toLocalTime())
    compose.onNodeWithTag("entry-time").performScrollTo().performTextClearance()
    compose.onNodeWithTag("entry-time").performTextInput("25:00")
    compose.onNodeWithTag("entry-time").performImeAction()
    compose.waitForIdle()
    assertEquals(LocalTime.of(14, 35), at().toLocalTime())
  }

  @Test fun scaleButtonsToggleAnswersAndYesPrecedesNo() {
    compose.onNodeWithText("Awful").performScrollTo().assertExists()
    compose.onNodeWithText("Great").assertExists()
    compose.onNodeWithTag("answer-mood-4").performClick()
    compose.waitUntil(10_000) {
      model.state.value.notes.single { it.id == entry }.entry!!.answers["mood"] ==
        Answer.Number(4.0)
    }
    compose.onNodeWithText("Yes").performScrollTo().performClick()
    compose.waitUntil(10_000) {
      model.state.value.notes.single { it.id == entry }.entry!!.answers["walk"] == Answer.Bool(true)
    }
    val yes = compose.onNodeWithText("Yes").fetchSemanticsNode().boundsInRoot
    val no = compose.onNodeWithText("No").fetchSemanticsNode().boundsInRoot
    assertTrue(yes.left < no.left)
    compose.onNodeWithText("Sunny").performScrollTo().performClick()
    compose.waitForIdle()
    val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
    try {
      File(context.cacheDir, "ui-entry-controls.png").outputStream().use {
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
      }
    } finally {
      bitmap.recycle()
    }
    compose.onNodeWithTag("answer-mood-4").performScrollTo().performClick()
    compose.waitUntil(10_000) {
      "mood" !in model.state.value.notes.single { it.id == entry }.entry!!.answers
    }
  }
}
