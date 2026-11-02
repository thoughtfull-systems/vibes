// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import org.junit.Assert.assertEquals
import org.junit.Test

class RepeatTest {
  @Test
  fun coalescedRepeatedOffsetsDoNotHideLaterRemoteChanges() {
    val typing = OffsetInput(-3, "4", pending = 3, sequence = 3)
    val saved = typing.receive(-4, -1).saved(3, -4, true, -1)
    assertEquals("45", saved.receive(-45, -1).text)
  }

  @Test
  fun remoteOffsetsReplaceTheDisplayedValue() {
    val shown = OffsetInput(-3, "3")
    assertEquals("8", shown.receive(-8, -1).text)
    assertEquals("0", shown.receive(null, -1).text)
    assertEquals("-8", OffsetInput(3, "3").receive(-8, 1).text)
  }

  @Test
  fun savingEarlierDigitsKeepsTheOwnersLatestInput() {
    val typing = OffsetInput(-3, "45", pending = 2, sequence = 2)
    val firstSaved = typing.receive(-4, -1).saved(1, -4, true, -1)
    assertEquals("45", firstSaved.text)
    assertEquals(2L, firstSaved.pending)
    val lastSaved = firstSaved.receive(-45, -1).saved(2, -45, true, -1)
    assertEquals("45", lastSaved.text)
    assertEquals(null, lastSaved.pending)
    assertEquals("8", lastSaved.receive(-8, -1).text)
  }

  @Test
  fun failedSavesRestoreTheLastReceivedValue() {
    val typing = OffsetInput(-3, "45", pending = 2, sequence = 2)
    val remote = typing.receive(-8, -1).saved(2, -45, false, -1)
    assertEquals("8", remote.text)
    assertEquals(null, remote.pending)
  }

  @Test
  fun returningToTheOriginalOffsetStillCompletesTheSave() {
    val typing = OffsetInput(-3, "3", pending = 3, sequence = 3)
    val saved = typing.saved(3, -3, true, -1)
    assertEquals(null, saved.pending)
    assertEquals("4", saved.receive(-4, -1).text)
  }

  @Test
  fun savesKeepAnUnfinishedNumberAndIgnoreOlderCompletions() {
    val typing = OffsetInput(-3, "-", pending = 2, sequence = 2)
    val saved = typing.saved(2, -45, true, -1).saved(1, -4, true, -1)
    assertEquals("-", saved.text)
    assertEquals(-45, saved.value)
    assertEquals(null, saved.pending)
  }
}
