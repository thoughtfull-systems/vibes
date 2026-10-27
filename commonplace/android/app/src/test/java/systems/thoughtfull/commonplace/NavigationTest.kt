// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import org.junit.Assert.assertEquals
import org.junit.Test

class NavigationTest {
  @Test
  fun backRetracesNotesAndTabsRatherThanClosingEveryNote() {
    val start = Navigation(Location("Memos"))
    val first = start.go(Location("Memos", "a"))
    val second = first.go(Location("Memos", "b"))
    assertEquals(first, second.back { true })
    assertEquals(start, second.back { true }.back { true })
    assertEquals(start, start.go(Location("Actions")).back { true })
  }

  @Test
  fun missingNotesAreSkippedAndOpeningTheCurrentLocationDoesNotAddHistory() {
    val start = Navigation(Location("All Notes"))
    val first = start.go(Location("All Notes", "deleted"))
    val second = first.go(Location("All Notes", "current"))
    assertEquals(start, second.back { it != "deleted" })
    assertEquals(second, second.go(second.current))
  }

  @Test
  fun aRestoredNoteWithoutHistoryReturnsToItsListing() {
    assertEquals(
      Navigation(Location("Trash")),
      Navigation(Location("Trash", "note")).back { true }
    )
  }
}
