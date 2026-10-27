// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import java.time.LocalDate
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Test
import systems.thoughtfull.commonplace.core.Answer
import systems.thoughtfull.commonplace.core.ChoiceOption
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind

class ChartsTest {
  private val day = LocalDate.of(2026, 10, 25)
  private fun at(days: Long) = day.plusDays(days).atStartOfDay(ZoneId.systemDefault())
    .toInstant().toEpochMilli()

  @Test
  fun choicesCountDaysAndLeaveMissingDaysDistinctFromUnpickedOptions() {
    val options = listOf(ChoiceOption("a", "Walk", null), ChoiceOption("b", "Run", null))
    val question = Question("exercise", "Exercise", QuestionKind.Choice(false, options), null)
    val rows = daySeries(
      question,
      listOf(
        at(0) to Answer.Choices(listOf("a")),
        at(0) + 1 to Answer.Choices(listOf("a")),
        at(2) to Answer.Choices(listOf("b"))
      )
    )
    assertEquals(listOf("Walk", "Run"), rows.map { it.label })
    assertEquals(listOf(1, 1), rows.map { it.count })
    assertEquals(DayMark.DOT, rows[0].mark(day))
    assertEquals(DayMark.MISSING, rows[0].mark(day.plusDays(1)))
    assertEquals(DayMark.EMPTY, rows[0].mark(day.plusDays(2)))
  }

  @Test
  fun yesTakesPrecedenceOverNoOnTheSameDay() {
    val question = Question("coffee", "Coffee", QuestionKind.YesNo, null)
    val row = daySeries(
      question,
      listOf(
        at(0) to Answer.Bool(false),
        at(0) + 1 to Answer.Bool(true),
        at(2) to Answer.Bool(false)
      )
    ).single()
    assertEquals(DayMark.DOT, row.mark(day))
    assertEquals(DayMark.MISSING, row.mark(day.plusDays(1)))
    assertEquals(DayMark.CROSS, row.mark(day.plusDays(2)))
    assertEquals(1, row.count)
    assertEquals(2, row.answered)
  }

  @Test
  fun scrubbingSelectsTheClosestRecordedTimeAndClampsAtItsEnds() {
    val points = listOf(100L to 2f, 200L to 10f, 1100L to 4f)
    assertEquals(points[1], closestPoint(points, 0.4f))
    assertEquals(points[2], closestPoint(points, 0.6f))
    assertEquals(points.first(), closestPoint(points, -1f))
    assertEquals(points.last(), closestPoint(points, 2f))
    assertEquals(points.first(), closestPoint(points.take(1), 0.7f))
  }

  @Test
  fun scrubbingHalfwayBetweenTwoSamplesSelectsTheEarlierOne() {
    val points = listOf(100L to 2f, 300L to 10f)
    assertEquals(points.first(), closestPoint(points, 0.5f))
  }

  @Test
  fun scrubbingTheFullWindowIncludesEmptyDaysBeforeAndAfterTheSamples() {
    val points = listOf(at(10) to 2f, at(20) to 4f)
    assertEquals(points.first(), closestPoint(points, 0f, at(0), at(30)))
    assertEquals(points.first(), closestPoint(points, 0.4f, at(0), at(30)))
    assertEquals(points.last(), closestPoint(points, 0.6f, at(0), at(30)))
    assertEquals(points.last(), closestPoint(points, 1f, at(0), at(30)))
  }
}
