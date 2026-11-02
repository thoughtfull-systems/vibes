// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import systems.thoughtfull.commonplace.core.DaysOff
import systems.thoughtfull.commonplace.core.DaysOffKind
import systems.thoughtfull.commonplace.core.MobileDaysOff

class PromptSettingsTest {
  @Test
  fun movingTodaysHolidayReschedulesPromptsWithoutChangingAJournal() {
    val holiday = DaysOff(
      "holiday",
      DaysOffKind.HOLIDAYS,
      "Holiday",
      "2026-10-31",
      "2026-10-31",
      null,
      null,
      null
    )
    val previous = NotebookState(loaded = true, daysOff = listOf(MobileDaysOff(holiday, "library")))
    val tomorrow = holiday.copy(first = "2026-11-01", last = "2026-11-01")
    val changed = previous.copy(daysOff = listOf(MobileDaysOff(tomorrow, "library")))
    assertTrue(changed.promptsChangedSince(previous))
    assertTrue(previous.copy(daysOff = emptyList()).promptsChangedSince(previous))
    assertFalse(previous.copy(revision = 2).promptsChangedSince(previous))
  }
}
