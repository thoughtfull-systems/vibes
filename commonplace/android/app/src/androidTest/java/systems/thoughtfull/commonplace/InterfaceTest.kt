// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.graphics.Bitmap
import android.os.SystemClock
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.ime
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextInputSelection
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.test.espresso.Espresso
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.time.LocalDate
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class InterfaceTest {
  @get:Rule
  val compose = createComposeRule()
  private val instrumentation = InstrumentationRegistry.getInstrumentation()
  private val context = instrumentation.targetContext

  private fun screenshot(name: String) {
    compose.waitForIdle()
    // Window transitions can outlast Compose's idle state.
    SystemClock.sleep(300)
    val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
    try {
      File(context.cacheDir, "ui-$name.png").outputStream().use {
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
      }
    } finally {
      bitmap.recycle()
    }
  }

  @Test
  fun desktopFiltersAndTheKeyboardToolbarWorkWithTheSharedSamples() {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    val repository = (context.applicationContext as CommonplaceApplication).repository
    repository.preferences.edit().putString("tab", "Memos").remove("selected")
      .putString("query", "").putString("action.mode", "Execute")
      .putString("order.Memos", "Created").putBoolean("ascending.Memos", false)
      .putString("from.Memos", "").commit()
    val model = NotebookModel(context.applicationContext as CommonplaceApplication)
    val keyboardVisible = AtomicBoolean(false)
    compose.setContent {
      val shown = WindowInsets.ime.getBottom(LocalDensity.current) > 0
      SideEffect { keyboardVisible.set(shown) }
      CommonplaceTheme { CommonplaceApp(model, null) {} }
    }
    compose.waitUntil(10_000) { model.state.value.loaded }
    if (!repository.preferences.getBoolean("samples.loaded", false)) {
      compose.onNodeWithText("Libraries").performClick()
      compose.onAllNodesWithText("Load test library").onLast().performScrollTo().performClick()
      compose.waitUntil(10_000) { repository.preferences.getBoolean("samples.loaded", false) }
      compose.onNodeWithText("Done").performClick()
    }
    compose.waitUntil(10_000) { model.state.value.notes.any { it.title == "Formatting sampler" } }
    screenshot("memos")
    compose.onNodeWithText("Modified").assertDoesNotExist()
    compose.onNodeWithText("Created").performClick()
    compose.onNodeWithText("Modified").performClick()
    compose.onNodeWithText("Any day").performClick()
    compose.onNodeWithText("Choose").assertDoesNotExist()
    screenshot("calendar")
    tapToday(compose)
    compose.onNodeWithText("Reset").performClick()
    compose.onNodeWithText("Created").assertExists()
    compose.onNodeWithTag("search").performTextInput("bold")
    compose.onNodeWithText("A bold and italic title with strikethrough, code, and highlight")
      .assertExists()
    screenshot("title")
    compose.onNodeWithTag("search").performTextClearance()
    compose.onNodeWithText("Actions").performClick()
    compose.onNodeWithText("Overdue").assertExists()
    compose.onNodeWithText("Waiting for").assertDoesNotExist()
    screenshot("execute")
    compose.onNodeWithText("Execute").performClick()
    compose.onNodeWithText("Review").performClick()
    compose.onNodeWithTag("actions").performScrollToNode(hasText("Waiting for"))
    compose.onNodeWithText("Waiting for").assertExists()
    compose.onNodeWithText("Memos").performClick()
    compose.onNodeWithTag("search").performTextInput("Packing list")
    runBlocking {
      repository.use { core ->
        val packing = core.notes("Packing list", LocalDate.now().toString()).first()
        val body = core.editorBody(packing.id)
        core.setBody(
          packing.id,
          body.lineSequence().joinToString("\n") {
            when {
              it.contains("Walking shoes") -> "- [ ] Walking shoes"
              it.contains("Passport") -> "- [x] Passport"
              else -> it
            }
          }
        )
      }
    }
    val packingId = model.state.value.notes.first { it.body.startsWith("Packing list for ") }.id
    compose.onNodeWithText("Packing list for the trip").performClick()
    val editor = compose.onNodeWithTag("note-editor")
    compose.waitUntil(10_000) {
      runCatching {
        editor.fetchSemanticsNode().config[SemanticsProperties.EditableText].text
          .contains("Walking shoes")
      }.getOrDefault(false)
    }
    fun tapFirstCheckbox() {
      val layouts = mutableListOf<TextLayoutResult>()
      editor.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
      val layout = layouts.single()
      val mark = layout.layoutInput.text.indexOfFirst { it == '☑' || it == '☐' }
      editor.performTouchInput { click(layout.getBoundingBox(mark).center) }
    }
    tapFirstCheckbox()
    compose.waitUntil(10_000) {
      runBlocking {
        repository.use { core ->
          core.notes("Packing list", LocalDate.now().toString()).any {
            it.body.contains("- [ ] Passport")
          }
        }
      }
    }
    tapFirstCheckbox()
    compose.waitUntil(10_000) {
      runBlocking {
        repository.use { core ->
          core.notes("Packing list", LocalDate.now().toString()).any {
            it.body.contains("- [x] Passport")
          }
        }
      }
    }
    editor.performClick()
    val body = editor.fetchSemanticsNode().config[SemanticsProperties.EditableText].text
    val at = body.indexOf("Walking shoes") + "Walking shoes".length
    editor.performTextInputSelection(TextRange(at), relativeToOriginalText = false)
    compose.onNodeWithContentDescription("Indent").performScrollTo().performClick()
    compose.waitUntil(10_000) {
      runBlocking {
        repository.use { core ->
          core.notes("Packing list", LocalDate.now().toString()).any {
            it.body.contains("  - [ ] Walking shoes")
          }
        }
      }
    }
    val indented = editor.fetchSemanticsNode().config[SemanticsProperties.EditableText].text
    val word = indented.indexOf("Walking shoes")
    editor.performTextInputSelection(TextRange(word, word + "Walking shoes".length), false)
    compose.onNodeWithContentDescription("Bold").performScrollTo().performClick()
    compose.waitUntil(10_000) {
      runBlocking {
        repository.use { core ->
          core.notes("Packing list", LocalDate.now().toString()).any {
            it.body.contains("**Walking shoes**")
          }
        }
      }
    }
    compose.onNodeWithContentDescription("Indent").assertExists()
    compose.onNodeWithText("Format").assertDoesNotExist()
    compose.onNodeWithText("Live preview").assertDoesNotExist()
    compose.waitUntil(5_000) { keyboardVisible.get() }
    assertTrue("Formatting keeps the keyboard open", keyboardVisible.get())
    screenshot("keyboard")
    compose.onNodeWithText("Details").performClick()
    Espresso.pressBack()
    compose.waitUntil(5_000) { !keyboardVisible.get() }
    screenshot("preview")
    val layouts = mutableListOf<TextLayoutResult>()
    compose.onNodeWithTag("note-editor").performSemanticsAction(
      SemanticsActions.GetTextLayoutResult
    ) { it(layouts) }
    val layout = layouts.single()
    val offset = layout.layoutInput.text.text.indexOf("the trip")
    compose.onNodeWithTag("note-editor").performTouchInput {
      click(layout.getBoundingBox(offset).center)
    }
    compose.onNodeWithText("Trip to Portugal").assertDoesNotExist()
    compose.waitUntil(5_000) { keyboardVisible.get() }
    assertTrue(
      "A link tap reveals its editable Markdown",
      editor.fetchSemanticsNode().config[SemanticsProperties.EditableText].contains("[[")
    )
    val expandedLayouts = mutableListOf<TextLayoutResult>()
    editor.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(expandedLayouts) }
    val expanded = expandedLayouts.single()
    val icon = expanded.layoutInput.text.getStringAnnotations(
      "open-link",
      0,
      expanded.layoutInput.text.length
    ).first()
    screenshot("link-expanded")
    val iconBox = expanded.getBoundingBox(icon.start)
    val iconLine = expanded.getLineForOffset(icon.start)
    assertTrue(
      "The link icon adds no line height",
      expanded.getLineBottom(iconLine) - expanded.getLineTop(iconLine) <=
        expanded.getLineBottom(0) - expanded.getLineTop(0) + 1f
    )
    editor.performTouchInput { click(Offset(iconBox.right - 1f, iconBox.center.y)) }
    screenshot("link-clicked")
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithText("Trip to Portugal").assertExists() }.isSuccess
    }
    compose.onNodeWithTag("note-bottom-panel").assertExists()
    compose.onNodeWithText("Linked from").assertExists()
    screenshot("bottom")
    compose.onNodeWithContentDescription("Back").performClick()
    compose.waitUntil(5_000) {
      repository.preferences.getString("selected", null) == packingId &&
        runCatching {
          editor.fetchSemanticsNode().config[SemanticsProperties.EditableText]
            .contains("Walking shoes")
        }.getOrDefault(false)
    }
    screenshot("history")
    compose.onNodeWithContentDescription("Back").performClick()
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithText("All notes").assertExists() }.isSuccess
    }
    compose.onNodeWithText("All notes").performClick()
    compose.onNodeWithTag("search").performTextClearance()
    compose.onNodeWithTag("search").performTextInput("Fix up the house")
    val house = model.state.value.notes.first { it.title == "Fix up the house" }
    compose.onAllNodesWithText("Fix up the house").onLast().performClick()
    compose.onNodeWithText("Steps").assertExists()
    compose.onNodeWithTag("step-entry").assertExists()
    compose.waitUntil(10_000) {
      runCatching { compose.onNodeWithText("Paint the fence").assertExists() }.isSuccess
    }
    compose.onNodeWithText("Clean the gutters").assertDoesNotExist()
    compose.onNodeWithTag("steps-show-done").performClick()
    compose.onNodeWithText("Clean the gutters").performScrollTo().assertExists()
    screenshot("steps")
  }
}
