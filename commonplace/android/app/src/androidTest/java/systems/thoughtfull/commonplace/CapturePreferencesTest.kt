// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test

class CapturePreferencesTest {
  @get:Rule val compose = createComposeRule()
  private val context = InstrumentationRegistry.getInstrumentation().targetContext
  private val application = context.applicationContext as CommonplaceApplication
  private val repository = application.repository
  private val previous = mutableMapOf<String, String?>()
  private val request = mutableStateOf<CaptureRequest?>(null)
  private lateinit var model: NotebookModel
  private lateinit var first: String
  private lateinit var second: String

  @Before fun setup() {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    listOf("capture.journal", "capture.kind", "capture.space").forEach {
      previous[it] = repository.preferences.getString(it, null)
    }
    runBlocking {
      repository.use {
        first = it.createSpace()
        second = it.createSpace()
      }
    }
    repository.preferences.edit().putString("name.$first", "A capture library")
      .putString("name.$second", "B capture library").putString("capture.kind", "Memo")
      .putString("capture.space", first).remove("capture.journal").commit()
    model = NotebookModel(application)
    compose.setContent {
      CommonplaceTheme {
        val state by model.state.collectAsState()
        Box(Modifier.fillMaxSize()) {
          if (state.loaded) {
            request.value?.let {
              CaptureDialog(model, state, it, { request.value = null }) { request.value = null }
            }
          }
        }
      }
    }
    compose.waitUntil(10_000) { model.state.value.loaded }
  }

  @After fun cleanup() {
    runBlocking {
      repository.use {
        it.removeSpace(first)
        it.removeSpace(second)
      }
    }
    repository.preferences.edit().remove("name.$first").remove("name.$second").apply {
      previous.forEach { (key, value) -> putString(key, value) }
    }.commit()
  }

  private fun open() {
    compose.runOnIdle { request.value = CaptureRequest() }
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithText("Write in Markdown").assertExists() }.isSuccess
    }
  }

  private fun save(text: String) {
    compose.onNodeWithText("Write in Markdown").performTextInput(text)
    compose.onNodeWithText("Save").performClick()
    compose.waitUntil(10_000) { request.value == null }
    compose.waitForIdle()
  }

  @Test fun remembersTheLastSavedTypeAndSpaceButNotCanceledChoices() {
    open()
    compose.onNodeWithText("Action").performClick()
    compose.onNodeWithText("B capture library").performScrollTo().performClick()
    save("Remember these choices")
    assertEquals("Action", repository.preferences.getString("capture.kind", null))
    assertEquals(second, repository.preferences.getString("capture.space", null))
    val captured = runBlocking {
      repository.use { core -> core.notes("").single { it.body == "Remember these choices" } }
    }
    assertEquals(second, captured.space)
    check(captured.action != null)
    open()
    compose.onNodeWithText("Action").assertIsSelected()
    compose.onNodeWithText("B capture library").assertIsSelected()
    compose.onNodeWithText("Memo").performClick()
    compose.onNodeWithText("A capture library").performScrollTo().performClick()
    compose.onNodeWithText("Cancel").performClick()
    compose.waitForIdle()
    open()
    compose.onNodeWithText("Action").assertIsSelected()
    compose.onNodeWithText("B capture library").assertIsSelected()
  }

  @Test fun anEntryRemembersItsJournalAndItsActualSpace() {
    val journal = runBlocking {
      repository.use { it.createJournal(second, "Capture preference journal") }
    }
    repository.preferences.edit().putString("capture.journal", journal).commit()
    model.refresh()
    compose.waitUntil(10_000) { model.state.value.journals.any { it.id == journal } }
    open()
    compose.onNodeWithText("Entry").performClick()
    save("\nA remembered entry")
    assertEquals("Entry", repository.preferences.getString("capture.kind", null))
    assertEquals(second, repository.preferences.getString("capture.space", null))
    assertEquals(journal, repository.preferences.getString("capture.journal", null))
    open()
    compose.onNodeWithText("Entry").assertIsSelected()
    compose.onNodeWithText("Capture preference journal").assertIsSelected()
    compose.onNodeWithText("Memo").performClick()
    compose.onNodeWithText("B capture library").assertIsSelected()
  }
}
