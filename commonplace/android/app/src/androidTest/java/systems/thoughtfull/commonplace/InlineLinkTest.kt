// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class InlineLinkTest {
  @get:Rule
  val compose = createComposeRule()

  @Test
  fun onlyTheInlineIconOpensACollapsedOrExpandedLink() {
    val body = "🦀 Before [café](https://example.com/a/long/address) after"
    var value by mutableStateOf(TextFieldValue(body))
    var opened = 0
    val focus = FocusRequester()
    compose.setContent {
      CommonplaceTheme {
        MarkdownEditor(
          value,
          true,
          { value = it },
          focus,
          {},
          Modifier.fillMaxSize(),
          { opened++ }
        )
      }
    }
    val editor = compose.onNodeWithTag("note-editor")
    fun layout(): TextLayoutResult {
      val layouts = mutableListOf<TextLayoutResult>()
      editor.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
      return layouts.single()
    }
    fun tapIcon() {
      val result = layout()
      val at = result.layoutInput.text.getStringAnnotations(
        "open-link",
        0,
        result.layoutInput.text.length
      ).single().start
      editor.performTouchInput { click(result.getBoundingBox(at).center) }
    }
    tapIcon()
    compose.runOnIdle { assertEquals(1, opened) }
    val preview = layout()
    editor.performTouchInput {
      click(preview.getBoundingBox(preview.layoutInput.text.text.indexOf("café")).center)
    }
    compose.waitUntil(5_000) { layout().layoutInput.text.contains("[café]") }
    compose.runOnIdle { assertEquals(1, opened) }
    tapIcon()
    compose.runOnIdle {
      assertEquals(2, opened)
      assertEquals(body, value.text)
    }
  }
}
