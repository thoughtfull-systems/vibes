// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import java.time.LocalDate
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import systems.thoughtfull.commonplace.core.Action
import systems.thoughtfull.commonplace.core.ActionDates
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.ActionStatus
import systems.thoughtfull.commonplace.core.ActionView
import systems.thoughtfull.commonplace.core.MobileAction
import systems.thoughtfull.commonplace.core.MobileNote

private fun note(id: String, created: Long = 1) = MobileNote(
  action = null,
  archived = null,
  body = "$id\n\nA body that doesn't belong in the row",
  created = created,
  entry = null,
  id = id,
  inTrash = false,
  journal = null,
  kind = "memo",
  modified = created,
  parent = null,
  readOnly = false,
  space = "library",
  title = id,
  trashed = null
)

private fun task(id: String, status: ActionStatus = ActionStatus.NORMAL) = MobileAction(
  dates = ActionDates(null, null, status),
  depth = 0u,
  note = note(id).copy(
    action = Action(null, null, null, ActionList.ACTIVE, null, false, ActionState.OPEN, null)
  ),
  parents = emptyList(),
  stalled = false
)

class BrowseTest {
  @Test
  fun dateFiltersFollowTheDirectionAndResetDefaultsToCreated() {
    val zone = ZoneId.of("America/New_York")
    val day = LocalDate.of(2026, 10, 26)
    val old = note("older", day.minusDays(1).atStartOfDay(zone).toInstant().toEpochMilli())
    val recent = note("newer", day.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli())
    val same = note("on the date", day.atStartOfDay(zone).toInstant().toEpochMilli())
    val notes = listOf(old, recent, same)
    assertEquals(
      listOf(same, old),
      browseNotes(notes, "Memos", "", BrowseOrder.CREATED, false, day, zone)
    )
    assertEquals(
      listOf(same, recent),
      browseNotes(notes, "Memos", "", BrowseOrder.CREATED, true, day, zone)
    )
  }

  @Test
  fun dueDatesKeepUndatedNotesAtTheEndUntilADayIsPicked() {
    val withDue = task("due").note.let { it.copy(action = it.action!!.copy(due = "2026-10-26")) }
    val undated = note("undated")
    assertEquals(
      listOf(withDue, undated),
      browseNotes(listOf(undated, withDue), "All Notes", "", BrowseOrder.DUE, false, null)
    )
    assertEquals(
      listOf(withDue),
      browseNotes(
        listOf(undated, withDue),
        "All Notes",
        "",
        BrowseOrder.DUE,
        false,
        LocalDate.of(2026, 10, 26)
      )
    )
  }

  @Test
  fun trashListsTrashedParentsWithoutRepeatingTheirInheritedChildren() {
    val parent = note("parent").copy(inTrash = true, trashed = 2)
    val child = note("child").copy(inTrash = true, parent = "parent")
    assertEquals(
      listOf(parent),
      browseNotes(listOf(parent, child), "Trash", "", BrowseOrder.TRASHED, false, null)
    )
  }
}

class ActionSectionsTest {
  @Test
  fun executeGroupsByUrgencyAndWaitingOnlyAppearsInReview() {
    val active = task("active")
    val overdue = task("overdue", ActionStatus.OVERDUE)
    val waiting = task("waiting")
    val views = mapOf(
      ActionView.NEXT to listOf(active, overdue),
      ActionView.WAITING to listOf(waiting)
    )
    val execute = actionSections("Execute", views, "")
    assertEquals(listOf("overdue"), execute.first().entries.map { it.note.id })
    assertEquals(listOf("active"), execute[2].entries.map { it.note.id })
    assertTrue(execute.flatMap { it.entries }.none { it.note.id == "waiting" })
    val review = actionSections("Review", views, "")
    assertEquals(listOf(waiting), review.first { it.title == "Waiting For" }.entries)
  }

  @Test
  fun planningDoesNotRepeatQueuedStalledProjects() {
    val project = task("project").copy(stalled = true)
    val views = mapOf(ActionView.PLANNING to listOf(project), ActionView.STALLED to listOf(project))
    val sections = actionSections("Plan", views, "")
    assertEquals(listOf(project), sections.first().entries)
    assertTrue(sections.last().entries.isEmpty())
  }
}

class ParentChoicesTest {
  @Test
  fun excludesTheActionItsDescendantsAndTrashedParents() {
    val root = task("root").note
    val child = task("child").note.copy(parent = root.id)
    val grandchild = task("grandchild").note.copy(parent = child.id)
    val other = task("other").note
    val trashed = task("trashed").note.copy(inTrash = true)
    assertEquals(
      listOf(other),
      parentChoices(listOf(grandchild, child, other, root, trashed), root.id)
    )
  }
}

class StepVisibilityTest {
  @Test
  fun hidingAClosedProjectAlsoHidesItsDescendants() {
    val done = task("done").let {
      it.copy(note = it.note.copy(action = it.note.action!!.copy(state = ActionState.DONE)))
    }
    val child = task("child").copy(depth = 1u)
    val grandchild = task("grandchild").copy(depth = 2u)
    val sibling = task("sibling")
    val steps = listOf(done, child, grandchild, sibling)
    assertEquals(listOf(sibling), visibleSteps(steps, false))
    assertEquals(steps, visibleSteps(steps, true))
  }
}
