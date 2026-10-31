// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import org.junit.Assert.assertEquals
import org.junit.Test

class RepeatTest {
  @Test
  fun remoteOffsetsReplaceTheDisplayedValue() {
    val shown = OffsetInput(-3, "3")
    assertEquals("8", shown.receive(-8, -1).text)
    assertEquals("0", shown.receive(null, -1).text)
    assertEquals("-8", OffsetInput(3, "3").receive(-8, 1).text)
  }

  @Test
  fun savingEarlierDigitsKeepsTheOwnersLatestInput() {
    val typing = OffsetInput(-3, "45", listOf(-4, -45))
    val firstSaved = typing.receive(-4, -1)
    assertEquals("45", firstSaved.text)
    assertEquals(listOf(-45), firstSaved.pending)
    val lastSaved = firstSaved.receive(-45, -1)
    assertEquals("45", lastSaved.text)
    assertEquals(emptyList<Int>(), lastSaved.pending)
    assertEquals("8", lastSaved.receive(-8, -1).text)
  }

  @Test
  fun remoteChangesDiscardSupersededInput() {
    val typing = OffsetInput(-3, "45", listOf(-4, -45))
    val remote = typing.receive(-8, -1)
    assertEquals("8", remote.text)
    assertEquals(emptyList<Int>(), remote.pending)
  }
}
