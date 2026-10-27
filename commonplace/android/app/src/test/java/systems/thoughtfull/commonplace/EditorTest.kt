// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.assertEquals
import org.junit.Test

class EditorTest {
  @Test
  fun typingOverASingleSelectedCharacterWrapsIt() {
    val before = TextFieldValue("a", TextRange(0, 1))
    val after = continueList(before, TextFieldValue("*", TextRange(1)))
    assertEquals("*a*", after.text)
    assertEquals(TextRange(1, 2), after.selection)
  }

  @Test
  fun wrappingKeepsUnicodeAndCanDoubleItsDelimiters() {
    val original = TextFieldValue("A 🦀 idea", TextRange(2, 4))
    val first = continueList(original, TextFieldValue("A = idea", TextRange(3)))
    assertEquals("A =🦀= idea", first.text)
    val second = continueList(first, TextFieldValue("A === idea", TextRange(4)))
    assertEquals("A ==🦀== idea", second.text)
    assertEquals(TextRange(4, 6), second.selection)
  }

  @Test
  fun ordinaryReplacementStaysOrdinaryText() {
    val before = TextFieldValue("a thought", TextRange(2, 9))
    val after = TextFieldValue("a note", TextRange(6))
    assertEquals(after, continueList(before, after))
  }
}

class MarkdownPresentationTest {
  @Test
  fun hiddenLinkTargetsOccupyNoSpaceAndKeepCursorOffsets() {
    val body = "🦀 [café](https://example.com/a/long/address) end"
    val start = body.indexOf('[')
    val end = body.indexOf(')') + 1
    val span = systems.thoughtfull.commonplace.core.MobileSpan(
      end.toUInt(),
      start.toUInt(),
      systems.thoughtfull.commonplace.core.Style.Link,
      listOf(
        systems.thoughtfull.commonplace.core.MobileRange((start + 1).toUInt(), start.toUInt()),
        systems.thoughtfull.commonplace.core.MobileRange(end.toUInt(), (start + 5).toUInt())
      )
    )
    val result = markdownPresentation(body, listOf(span), null)
    assertEquals("🦀 café end", result.text.text)
    assertEquals(start + 1, result.offsetMapping.transformedToOriginal(start))
    assertEquals(end, result.offsetMapping.transformedToOriginal(start + 4))
    val offsets = (0..body.length).map(result.offsetMapping::originalToTransformed)
    assertEquals(offsets.sorted(), offsets)
    assertEquals(result.text.length, offsets.last())
    val original = (0..result.text.length).map(result.offsetMapping::transformedToOriginal)
    assertEquals(original.sorted(), original)
    assertEquals(body.length, original.last())
    val editing = markdownPresentation(body, listOf(span), TextRange(start + 2))
    assertEquals(body, editing.text.text)
    assertEquals(start + 2, editing.offsetMapping.transformedToOriginal(start + 2))
  }

  @Test
  fun wikiLinksHideTheIdentifierWithoutChangingTheStoredText() {
    val target = "f".repeat(32)
    val body = "before [[$target|a note]] after"
    val start = body.indexOf("[[")
    val label = body.indexOf('|') + 1
    val end = body.indexOf("]]") + 2
    val span = systems.thoughtfull.commonplace.core.MobileSpan(
      end.toUInt(),
      start.toUInt(),
      systems.thoughtfull.commonplace.core.Style.Link,
      listOf(
        systems.thoughtfull.commonplace.core.MobileRange(label.toUInt(), start.toUInt()),
        systems.thoughtfull.commonplace.core.MobileRange(end.toUInt(), (end - 2).toUInt())
      )
    )
    val result = markdownPresentation(body, listOf(span), TextRange(0))
    assertEquals("before a note after", result.text.text)
    assertEquals("before [[$target|a note]] after", body)
  }

  @Test
  fun listPrefixHandlesAnEmptyFirstLineAndUnicodeSelections() {
    val empty = prefixSelection(TextFieldValue("\n", TextRange(0)), "- ")
    assertEquals("- \n", empty.text)
    assertEquals(TextRange(2), empty.selection)
    val value = prefixSelection(TextFieldValue("🦀\nnext", TextRange(0, 7)), "- [ ] ")
    assertEquals("- [ ] 🦀\n- [ ] next", value.text)
    assertEquals(TextRange(6, 19), value.selection)
  }
}

class InlineLinkPresentationTest {
  @Test
  fun theIconStaysBeforeCollapsedAndExpandedLinksWithUnicodeOffsets() {
    val body = "🦀 [café](https://example.com) end"
    val start = body.indexOf('[')
    val end = body.indexOf(')') + 1
    val link = systems.thoughtfull.commonplace.core.MobileLink(
      end.toUInt(),
      null,
      start.toUInt(),
      "https://example.com"
    )
    val span = systems.thoughtfull.commonplace.core.MobileSpan(
      end.toUInt(),
      start.toUInt(),
      systems.thoughtfull.commonplace.core.Style.Link,
      listOf(
        systems.thoughtfull.commonplace.core.MobileRange((start + 1).toUInt(), start.toUInt()),
        systems.thoughtfull.commonplace.core.MobileRange(end.toUInt(), (start + 5).toUInt())
      )
    )
    for (selection in listOf(null, TextRange(start + 2))) {
      val quote = systems.thoughtfull.commonplace.core.MobileSpan(
        body.length.toUInt(),
        0u,
        systems.thoughtfull.commonplace.core.Style.BlockQuote,
        emptyList()
      )
      val result = markdownPresentation(body, listOf(span, quote), selection, links = listOf(link))
      val expected = if (selection ==
        null
      ) {
        "🦀 M\u2060café end"
      } else {
        "🦀 M\u2060${body.drop(start)}"
      }
      assertEquals(expected, result.text.text)
      val icon = result.text.getStringAnnotations("open-link", 0, result.text.length).single()
      assertEquals(start, icon.start)
      assertEquals(start.toString(), icon.item)
      val appearance = result.text.spanStyles.last {
        icon.start >= it.start && icon.start < it.end
      }.item
      assertEquals(androidx.compose.ui.graphics.Color.Transparent, appearance.color)
      assertEquals(start, result.offsetMapping.transformedToOriginal(icon.start))
      val original = (0..body.length).map(result.offsetMapping::originalToTransformed)
      assertEquals(original.sorted(), original)
      val displayed = (0..result.text.length).map(result.offsetMapping::transformedToOriginal)
      assertEquals(displayed.sorted(), displayed)
      assertEquals(body.length, displayed.last())
    }
  }
}
