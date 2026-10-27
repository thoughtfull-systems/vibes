// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.content.Intent
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test

class CaptureTest {
  @get:Rule
  val compose = createComposeRule()
  private lateinit var model: NotebookModel
  private lateinit var space: String
  private val incoming = mutableStateOf<Intent?>(null)
  private val title = "Android capture ${System.nanoTime()}"

  @Before
  fun openAgentLibrary() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent.")) {
      "Run UI tests with -Pcommonplace.applicationId=" +
        "systems.thoughtfull.CommonplaceAgent.androidtest"
    }
    model = NotebookModel(context.applicationContext as CommonplaceApplication)
    space = runBlocking { model.repository.use { it.createSpace() } }
    model.repository.preferences.edit().putString("capture.space", space)
      .putString("capture.kind", "Memo").putString("tab", "Memos")
      .putString("query", "").remove("selected").remove("from.Memos")
      .putString("order.Memos", "Created").putBoolean("ascending.Memos", false).commit()
    model.refresh()
    compose.setContent {
      CommonplaceTheme {
        CommonplaceApp(model, incoming.value) {
          incoming.value =
            null
        }
      }
    }
    compose.waitUntil(10_000) {
      model.state.value.loaded && model.state.value.spaces.any { it.key == space }
    }
  }

  @After
  fun removeCapturedNote() {
    if (!::model.isInitialized) return
    runBlocking {
      model.repository.use { it.removeSpace(space) }
    }
  }

  @Test
  fun capturesAndRestoresATrashedMemo() {
    compose.runOnIdle { incoming.value = Intent(CAPTURE_ACTION) }
    compose.onNodeWithText("Write in Markdown").performTextInput(title)
    compose.onNodeWithText("Save").performClick()
    compose.waitUntil(10_000) { model.state.value.notes.any { it.body == title } }
    val captured = model.state.value.notes.single { it.body == title }
    compose.onNodeWithTag("note-editor").assertDoesNotExist()
    compose.onNode(hasScrollToIndexAction())
      .performScrollToNode(hasTestTag("note-row-${captured.id}"))
    compose.onNodeWithTag("note-row-${captured.id}").performClick()
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithContentDescription("Note menu").assertExists() }.isSuccess
    }
    compose.onNodeWithContentDescription("Note menu").performClick()
    compose.onNodeWithText("Move to trash").performClick()
    compose.waitUntil(10_000) { model.state.value.notes.any { it.body == title && it.inTrash } }
    compose.onNodeWithText("Restore").assertExists().performClick()
    compose.waitUntil(10_000) { model.state.value.notes.any { it.body == title && !it.inTrash } }
  }
}
