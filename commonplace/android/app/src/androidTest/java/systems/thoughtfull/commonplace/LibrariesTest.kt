// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextReplacement
import androidx.test.espresso.Espresso
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class LibrariesTest {
  @get:Rule val compose = createComposeRule()

  @Test fun librariesRenameInlineAndConfirmBeforeMerging() {
    val context = InstrumentationRegistry.getInstrumentation().targetContext
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val app = context.applicationContext as CommonplaceApplication
    val repository = app.repository
    val (source, target, memo) = runBlocking {
      repository.use { core ->
        val source = core.createSpace()
        val target = core.createSpace()
        Triple(source, target, core.createMemo(source, "Keep this when merging"))
      }
    }
    repository.preferences.edit().putString("name.$source", "A source fixture")
      .putString("name.$target", "A target fixture").commit()
    val model = NotebookModel(app)
    try {
      compose.setContent {
        CommonplaceTheme {
          val state by model.state.collectAsState()
          if (state.loaded) LibrariesDialog(model, state) {}
        }
      }
      compose.waitUntil(10_000) { model.state.value.loaded }
      compose.onNodeWithTag("library-name-$source").performScrollTo()
        .performTextReplacement("A renamed source")
      Espresso.closeSoftKeyboard()
      compose.onNodeWithTag("library-apply-$source").performClick()
      compose.waitUntil(10_000) { repository.name(source) == "A renamed source" }
      compose.onNodeWithTag("library-menu-$source").performScrollTo().performClick()
      compose.onNodeWithText("Merge into…").performClick()
      compose.onAllNodesWithText("A target fixture").onLast().performClick()
      compose.onNodeWithText("Merge A renamed source into A target fixture?").assertExists()
      assertEquals(source, runBlocking { repository.use { it.note(memo)!!.space } })
      compose.onNodeWithText("Merge").performClick()
      compose.waitUntil(10_000) {
        model.state.value.notes.find { it.id == memo }?.space == target
      }
      assertEquals(
        "Keep this when merging",
        runBlocking {
          repository.use { it.note(memo)!!.body }
        }
      )
    } finally {
      runBlocking {
        repository.use { core ->
          if (core.spaces().any { it.key == source }) core.removeSpace(source)
          core.removeSpace(target)
        }
      }
      repository.preferences.edit().remove("name.$source").remove("name.$target").apply()
    }
  }
}
