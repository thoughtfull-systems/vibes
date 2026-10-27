// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class StepDraggingTest {
  private fun row(id: String, top: Float, ancestors: List<String> = emptyList()) =
    StepDropRow(id, ancestors, Rect(0f, top, 400f, top + 50f), 148f)

  @Test fun draggingBesideRowsReordersAndDraggingOntoTitlesNests() {
    val rows = listOf(row("a", 0f), row("b", 50f), row("c", 100f))
    assertEquals(StepDrop("b", StepPlacement.BEFORE), stepDrop(rows, "a", Offset(380f, 55f)))
    assertEquals(StepDrop("b", StepPlacement.AFTER), stepDrop(rows, "a", Offset(380f, 95f)))
    assertEquals(StepDrop("b", StepPlacement.INSIDE), stepDrop(rows, "a", Offset(80f, 75f)))
    assertEquals(StepDrop("c", StepPlacement.AFTER), stepDrop(rows, "a", Offset(380f, 200f)))
  }

  @Test fun projectsCannotBeDroppedIntoThemselvesOrTheirDescendants() {
    val rows = listOf(row("parent", 0f), row("child", 50f, listOf("parent")))
    assertNull(stepDrop(rows, "parent", Offset(80f, 25f)))
    assertNull(stepDrop(rows, "parent", Offset(80f, 75f)))
    assertNull(stepDrop(rows + row("sibling", 100f), "parent", Offset(80f, 75f)))
    assertEquals(
      StepDrop("parent", StepPlacement.BEFORE),
      stepDrop(rows, "child", Offset(380f, 5f))
    )
  }
}
