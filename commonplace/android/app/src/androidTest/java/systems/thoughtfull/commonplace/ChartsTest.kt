// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.graphics.Bitmap
import androidx.compose.foundation.layout.Column
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.Answer
import systems.thoughtfull.commonplace.core.QuestionKind

class ChartsTest {
  @get:Rule
  val compose = createComposeRule()

  @Test
  fun draggingAcrossANumericChartSelectsTheClosestRecordedPoint() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val repository = (context.applicationContext as CommonplaceApplication).repository
    val (questions, entries, color) = runBlocking {
      repository.use { core ->
        if (core.journals().none {
            it.journal!!.questions.any { q ->
              q.kind is QuestionKind.Number
            }
          }
        ) {
          core.loadTestLibrary(LocalDate.now().toString())
        }
        val journal = core.journals().first {
          it.journal!!.questions.any { q ->
            q.kind is QuestionKind.Number
          }
        }
        Triple(
          journal.journal!!.questions,
          core.notes("", LocalDate.now().toString()).filter { it.entry?.journal == journal.id },
          journal.journal.color
        )
      }
    }
    val question = questions.first { it.kind is QuestionKind.Number }
    val since = LocalDate.now().minusDays(29)
      .atStartOfDay(ZoneId.systemDefault()).toInstant().toEpochMilli()
    compose.setContent {
      CommonplaceTheme {
        Column {
          JournalChart(question, entries, color, since)
          questions.firstOrNull { it.kind is QuestionKind.Choice }?.let {
            JournalChart(it, entries, color, since)
          }
        }
      }
    }
    compose.onAllNodesWithText("${LocalDate.now().minusDays(29)} – ${LocalDate.now()}")
      .assertCountEquals(2)
    val value = compose.onNodeWithTag("chart-value-${question.id}")
    val plot = compose.onNodeWithTag("chart-plot-${question.id}")
    plot.performTouchInput {
      down(Offset(width * 0.2f, height / 2f))
      moveTo(Offset(width * 0.75f, height / 2f), delayMillis = 100)
    }
    val middle = value.fetchSemanticsNode().config[SemanticsProperties.Text].single().text
    plot.performTouchInput {
      moveTo(Offset(width.toFloat(), height / 2f), delayMillis = 100)
      up()
    }
    val after = value.fetchSemanticsNode().config[SemanticsProperties.Text].single().text
    assertNotEquals("Dragging keeps updating after the first movement", middle, after)
    val points = entries.mapNotNull { note ->
      val entry = note.entry!!
      (entry.answers[question.id] as? Answer.Number)?.let { entry.at to it.v1.toFloat() }
    }.sortedBy { it.first }
    val zone = ZoneId.systemDefault()
    val first = since
    val last = LocalDate.now().plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli()
    val target = first + (last - first).toDouble()
    val expected = points.minBy { kotlin.math.abs(it.first - target) }
    assertEquals(dateLabel(expected.first), after.substringBeforeLast(" · "))
    val actual = after.substringAfterLast(" · ").substringBefore(' ').toFloat()
    assertEquals(expected.second, actual, 0.005f)
    compose.waitForIdle()
    val image =
      checkNotNull(InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot())
    try {
      File(context.cacheDir, "ui-charts.png").outputStream().use {
        image.compress(Bitmap.CompressFormat.PNG, 100, it)
      }
    } finally {
      image.recycle()
    }
  }

  @Test
  fun draggingAcrossAYesNoChartShowsOnlyTheAnswer() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val repository = (context.applicationContext as CommonplaceApplication).repository
    val (question, entries, color) = runBlocking {
      repository.use { core ->
        if (core.journals().none { journal ->
            journal.journal!!.questions.any { it.kind == QuestionKind.YesNo }
          }
        ) {
          core.loadTestLibrary(LocalDate.now().toString())
        }
        val journal = core.journals().first { journal ->
          journal.journal!!.questions.any { it.kind == QuestionKind.YesNo }
        }
        Triple(
          journal.journal!!.questions.first { it.kind == QuestionKind.YesNo },
          core.notes("", LocalDate.now().toString()).filter { it.entry?.journal == journal.id },
          journal.journal.color
        )
      }
    }
    val first = LocalDate.now().minusDays(29)
    val since = first.atStartOfDay(ZoneId.systemDefault()).toInstant().toEpochMilli()
    compose.setContent { CommonplaceTheme { JournalChart(question, entries, color, since) } }
    val row = daySeries(
      question,
      entries.mapNotNull { note ->
        note.entry!!.answers[question.id]?.let { note.entry.at to it }
      }
    ).single()
    listOf(DayMark.DOT to "Yes", DayMark.CROSS to "No", DayMark.MISSING to "No answer")
      .forEach { (mark, answer) ->
        val index = (0L..29L).first { row.mark(first.plusDays(it)) == mark }
        compose.onNodeWithTag("chart-row-${question.id}-Yes").performTouchInput {
          click(Offset(width * (index + 0.5f) / 30f, height / 2f))
        }
        compose.onNodeWithTag("chart-value-${question.id}")
          .assertTextEquals("${first.plusDays(index)} · $answer")
      }
  }
}
