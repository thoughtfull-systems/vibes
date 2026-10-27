// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import systems.thoughtfull.commonplace.core.ActionChange
import systems.thoughtfull.commonplace.core.ActionList
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.ListKey
import systems.thoughtfull.commonplace.core.MobileNotebook
import systems.thoughtfull.commonplace.core.listEdit
import systems.thoughtfull.commonplace.core.parseNoteUri

@RunWith(AndroidJUnit4::class)
class MobileBridgeTest {
  @Test
  fun nativeLibraryPersistsNotesAndActionHierarchy() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    val directory = File(context.cacheDir, "bridge-${System.nanoTime()}")
    try {
      var memo = ""
      var task = ""
      var step = ""
      MobileNotebook.open(directory.path).use { core ->
        val space = core.spaces().first().key
        memo = core.createMemo(space, "Android 🦀 note")
        task = core.createAction(space, "Project")
        core.changeAction(task, ActionChange.List(ActionList.ACTIVE))
        step = core.createStep(task, "Step")
        core.changeAction(step, ActionChange.State(ActionState.DONE))
        core.trash(task)
        assertTrue(core.note(step)!!.inTrash)
        core.restore(task)
        assertFalse(core.note(step)!!.inTrash)
      }
      MobileNotebook.open(directory.path).use { core ->
        assertEquals("Android 🦀 note", core.note(memo)!!.body)
        assertEquals(task, core.note(step)!!.parent)
        assertEquals(ActionState.DONE, core.note(step)!!.action!!.state)
        assertEquals(memo, parseNoteUri("commonplace:note/$memo"))
      }
    } finally {
      directory.deleteRecursively()
    }
  }

  @Test
  fun nativeEditorUsesAndroidStringOffsets() {
    val body = "🦀\n- café"
    val edit = listEdit(body, body.length.toUInt(), ListKey.ENTER)!!
    assertEquals(body.length.toUInt(), edit.start)
    assertEquals("\n- ", edit.text)
    assertEquals((body.length + 3).toUInt(), edit.cursor)
  }
}
