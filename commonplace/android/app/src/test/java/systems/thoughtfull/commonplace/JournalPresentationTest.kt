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
import systems.thoughtfull.commonplace.core.Times

class JournalPresentationTest {
  @Test
  fun scheduleTimeDescriptionsMatchLinuxForFixedAndRandomTimes() {
    assertEquals("At 09:00, 17:30", describeTimes(Times.Fixed(listOf(540u, 1050u))))
    assertEquals(
      "About every 60 minutes from 09:00 until 17:00",
      describeTimes(Times.Random(540u, 1020u, 60u, 15u))
    )
  }

  @Test
  fun listDatesShowTimeTodayAndShortDatesOtherwiseInTheLocalZone() {
    val today = LocalDate.of(2026, 10, 26)
    val zone = ZoneId.of("America/New_York")
    fun stamp(day: LocalDate) = day.atTime(9, 7).atZone(zone).toInstant().toEpochMilli()
    assertEquals("09:07", listTimestamp(stamp(today), zone, today))
    assertEquals("Oct 25", listTimestamp(stamp(today.minusDays(1)), zone, today))
    assertEquals("2025-10-26", listTimestamp(stamp(today.minusYears(1)), zone, today))
    val justAfterMidnight = today.atStartOfDay(ZoneId.of("UTC")).plusHours(1)
      .toInstant().toEpochMilli()
    assertEquals("Oct 25", listTimestamp(justAfterMidnight, zone, today))
  }

  @Test
  fun rangesIncludeTodayAndUseLocalMidnightAcrossClockChanges() {
    val today = LocalDate.of(2026, 11, 2)
    val zone = ZoneId.of("America/New_York")
    assertEquals(
      today.atStartOfDay(zone).toInstant().toEpochMilli(),
      JournalRange.TODAY.since(today, zone)
    )
    assertEquals(
      today.minusDays(6).atStartOfDay(zone).toInstant().toEpochMilli(),
      JournalRange.WEEK.since(today, zone)
    )
    assertEquals(0L, JournalRange.ALL.since(today, zone))
  }

  @Test
  fun timelineSummariesPreserveQuestionOrderUnitsAndRetiredAnswers() {
    val questions = listOf(
      Question("distance", "Distance", QuestionKind.Number("km", null, null), null),
      Question("time", "Time", QuestionKind.Duration, null),
      Question(
        "choices",
        "Activities",
        QuestionKind.Choice(
          true,
          listOf(ChoiceOption("walk", "Walk", null), ChoiceOption("swim", "Swim", 1L))
        ),
        null
      ),
      Question("done", "Finished?", QuestionKind.YesNo, null),
      Question("missing", "Missing", QuestionKind.ShortText, null)
    )
    val answers = mapOf(
      "done" to Answer.Bool(false),
      "choices" to Answer.Choices(listOf("swim", "walk")),
      "time" to Answer.Number(85.0),
      "distance" to Answer.Number(2.5)
    )
    assertEquals(
      "Distance 2.5 km · Time 1 h 25 min · Activities Swim, Walk · Finished? No",
      answerSummary(questions, answers)
    )
    assertEquals(
      "**A bold entry**\nA [link](https://example.com)",
      entryPreview("# Template heading\n\n**A bold entry**\nA [link](https://example.com)")
    )
  }
}
