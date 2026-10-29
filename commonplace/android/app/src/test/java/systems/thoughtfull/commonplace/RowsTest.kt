// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import java.time.LocalDate
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import systems.thoughtfull.commonplace.core.Action
import systems.thoughtfull.commonplace.core.ActionDates
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.ActionStatus
import systems.thoughtfull.commonplace.core.ChoiceOption
import systems.thoughtfull.commonplace.core.Dated
import systems.thoughtfull.commonplace.core.Journal
import systems.thoughtfull.commonplace.core.MobileEntry
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind

private val today = LocalDate.of(2026, 10, 27)

private fun note(id: String, title: String = id) = MobileNote(
  action = null,
  archived = null,
  body = title,
  created = 1,
  entry = null,
  id = id,
  inTrash = false,
  journal = null,
  kind = "memo",
  modified = 1,
  parent = null,
  readOnly = false,
  space = "library",
  starred = null,
  title = title,
  trashed = null
)

private fun action(
  id: String,
  project: Boolean = false,
  list: ActionList = ActionList.ACTIVE,
  state: ActionState = ActionState.OPEN
) = note(id).copy(
  action = Action(null, null, null, list, null, project, state, null),
  kind = "action"
)

class RowsTest {
  @Before
  fun englishDates() {
    Locale.setDefault(Locale.US)
  }

  @Test
  fun datesReadLikeLinuxWithTheYearOnlyWhenItDiffers() {
    assertEquals("Tue Oct 27", formatDate("2026-10-27", today))
    assertEquals("Jan 15, 2027", formatDate("2027-01-15", today))
    assertEquals("not a date", formatDate("not a date", today))
  }

  @Test
  fun actionSummariesNameOverdueInheritedAndFutureDates() {
    val project = action("project", project = true).copy(title = "House")
    val labels = RowContext(listOf(project))::label
    val dates = ActionDates(
      Dated("2026-10-20", "project"),
      Dated("2026-10-29", "task"),
      ActionStatus.OVERDUE
    )
    assertEquals(
      "Overdue since Tue Oct 20 from House · Starts Thu Oct 29",
      actionSummary(action("task"), dates, false, labels, today)
    )
    val started = ActionDates(null, Dated("2026-10-01", "task"), ActionStatus.NORMAL)
    assertEquals("", actionSummary(action("task"), started, false, labels, today))
    val done = action("task", state = ActionState.DONE)
    assertEquals(
      "Due Tue Oct 20 from House",
      actionSummary(done, dates.copy(start = null), false, labels, today)
    )
    assertEquals(
      "Stalled project · Waiting",
      actionSummary(action("p", project = true, list = ActionList.WAITING), null, true, labels)
    )
    val inherited = dates.copy(due = null, start = Dated("2026-10-29", "project"))
    assertEquals("Starts Thu Oct 29 from House", inheritedDates("task", inherited, labels, today))
  }

  @Test
  fun rowsNameTheirJournalAndMarkArchivedNotes() {
    val journal = note("journal", "Mood").copy(
      journal = Journal("green", emptyList(), emptyList(), emptyMap()),
      kind = "journal"
    )
    val entry = note("entry", "").copy(
      entry = MobileEntry(emptyMap(), 5, "journal", null, null),
      kind = "entry"
    )
    val context = RowContext(listOf(journal, entry))
    assertEquals("green", context.entryColor(entry))
    assertEquals("Empty entry", displayLabel(entry))
    assertEquals("Untitled journal", displayLabel(journal.copy(title = "")))
    assertEquals("Empty project", displayLabel(action("p", project = true).copy(title = "")))
    val created = rowSummary(entry, Stamp.CREATED, null, false, context, today)
    assertEquals("Mood · Created ${listTimestamp(1)}", created)
    val archived = note("memo").copy(archived = 1)
    assertEquals(
      "Archived · Modified ${listTimestamp(1)}",
      rowSummary(archived, Stamp.MODIFIED, null, false, context, today)
    )
    assertEquals(
      "Archived ${listTimestamp(1)}",
      rowSummary(archived, Stamp.ARCHIVED, null, false, context, today)
    )
  }

  @Test
  fun scheduleDaysStartOnSundayAndWriteEveryDayAsTheCoreDoes() {
    assertEquals(
      listOf("Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"),
      scheduleWeekdays.map { it.second }
    )
    assertEquals("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR,SA", toggleWeekday(WORKDAYS, "SA"))
    assertEquals("FREQ=DAILY", toggleWeekday("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR,SA", "SU"))
    assertEquals("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR,SA", toggleWeekday("FREQ=DAILY", "SU"))
    assertEquals("FREQ=WEEKLY;BYDAY=MO", toggleWeekday("FREQ=WEEKLY;BYDAY=MO", "MO"))
  }

  @Test
  fun questionDescriptionsMatchLinux() {
    fun question(kind: QuestionKind) = Question("q", "Prompt", kind, null)
    val number = question(QuestionKind.Number("kg", null, null))
    assertEquals("Number, in kg", describeQuestion(number))
    val scale = question(QuestionKind.Scale(1, 5, null, null))
    assertEquals("Scale from 1 to 5", describeQuestion(scale))
    assertEquals(
      "Choice: any of Walk, Swim",
      describeQuestion(
        question(
          QuestionKind.Choice(
            true,
            listOf(
              ChoiceOption("a", "Walk", null),
              ChoiceOption("b", "Swim", null),
              ChoiceOption("c", "Cycle", 1)
            )
          )
        )
      )
    )
    assertEquals("Yes or no", describeQuestion(question(QuestionKind.YesNo)))
  }
}
