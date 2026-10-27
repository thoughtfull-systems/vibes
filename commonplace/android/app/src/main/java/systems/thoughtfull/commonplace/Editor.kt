// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextGeometricTransform
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import systems.thoughtfull.commonplace.core.ListKey
import systems.thoughtfull.commonplace.core.MobileLink
import systems.thoughtfull.commonplace.core.MobileSpan
import systems.thoughtfull.commonplace.core.Style
import systems.thoughtfull.commonplace.core.editorMarkup
import systems.thoughtfull.commonplace.core.inlinePieces
import systems.thoughtfull.commonplace.core.listEdit
import systems.thoughtfull.commonplace.core.spans
import systems.thoughtfull.commonplace.core.toggleCheckbox

internal data class MarkdownColors(
  val code: Color = Color(0xFFF0F0F0),
  val highlight: Color = Color(0x70F5D42E),
  val link: Color = Color(0xFF3584E4),
  val muted: Color = Color(0xFF777777)
)

@Composable
private fun markdownColors() = MarkdownColors(
  code = MaterialTheme.colorScheme.surfaceContainer,
  link = MaterialTheme.colorScheme.primary,
  muted = MaterialTheme.colorScheme.onSurfaceVariant
)

private fun Style.appearance(colors: MarkdownColors): SpanStyle = when (this) {
  is Style.Heading -> SpanStyle(
    fontSize = (28 - v1.toInt() * 2).coerceAtLeast(18).sp,
    fontWeight = FontWeight.Bold
  )

  Style.Strong -> SpanStyle(fontWeight = FontWeight.Bold)

  Style.Emphasis -> SpanStyle(fontStyle = FontStyle.Italic)

  Style.Code, Style.CodeBlock -> SpanStyle(
    background = colors.code,
    fontFamily = FontFamily.Monospace
  )

  Style.ListPrefix -> SpanStyle(fontFamily = FontFamily.Monospace)

  Style.Strikethrough, Style.Completed -> SpanStyle(textDecoration = TextDecoration.LineThrough)

  Style.Highlight -> SpanStyle(background = colors.highlight)

  Style.Link -> SpanStyle(color = colors.link, textDecoration = TextDecoration.Underline)

  Style.BlockQuote -> SpanStyle(color = colors.muted, fontStyle = FontStyle.Italic)

  is Style.Checkbox -> SpanStyle(color = colors.link)
}

internal fun markdownPresentation(
  text: String,
  markup: List<MobileSpan>,
  selection: TextRange?,
  colors: MarkdownColors = MarkdownColors(),
  links: List<MobileLink> = emptyList()
): TransformedText {
  val hidden = BooleanArray(text.length)
  val replacements = mutableMapOf<Int, Char>()
  markup.forEach { span ->
    val touches = selection != null && span.start.toInt() <= selection.max &&
      selection.min <= span.end.toInt()
    if (!touches) {
      span.syntax.forEach { range ->
        for (index in range.start.toInt() until range.end.toInt()) hidden[index] = true
      }
      if (span.style is Style.Checkbox) {
        val start = span.start.toInt()
        replacements[start] = if (span.style.v1) '☑' else '☐'
        for (index in start + 1 until span.end.toInt()) hidden[index] = true
      }
    }
  }
  val icons = links.associateBy { it.start.toInt() }
  val iconOffsets = mutableListOf<Pair<Int, Int>>()
  val original = IntArray(text.length + 1)
  val rendered = buildString {
    text.indices.forEach { index ->
      if (index in icons) {
        iconOffsets.add(length to index)
        // Android trims whitespace at soft wraps, which can reverse its glyph's hit box.
        append("M\u2060")
      }
      original[index] = length
      if (!hidden[index]) append(replacements[index] ?: text[index])
    }
    original[text.length] = length
  }
  val displayed = IntArray(rendered.length + 1)
  original.forEachIndexed { index, offset -> displayed[offset] = index }
  iconOffsets.forEach { (offset, source) ->
    displayed[offset] = source
    displayed[offset + 1] = source
  }
  val offsets = object : OffsetMapping {
    override fun originalToTransformed(offset: Int) = original[offset.coerceIn(0, text.length)]
    override fun transformedToOriginal(offset: Int) = displayed[offset.coerceIn(0, rendered.length)]
  }
  val styled = buildAnnotatedString {
    append(rendered)
    markup.forEach { span ->
      val start = original[span.start.toInt()]
      val end = iconOffsets.find { it.second == span.end.toInt() }?.first
        ?: original[span.end.toInt()]
      if (end > start) addStyle(span.style.appearance(colors), start, end)
    }
    iconOffsets.forEach { (at, source) ->
      addStyle(
        SpanStyle(
          color = Color.Transparent,
          fontSize = 16.sp,
          textGeometricTransform = TextGeometricTransform(scaleX = 1.75f)
        ),
        at,
        at + 1
      )
      addStringAnnotation("open-link", source.toString(), at, at + 1)
    }
  }
  return TransformedText(styled, offsets)
}

@Composable
internal fun formattedTitle(title: String): AnnotatedString {
  val colors = markdownColors()
  return remember(title, colors) {
    buildAnnotatedString {
      inlinePieces(title).forEach { piece ->
        val start = length
        append(piece.text)
        piece.styles.forEach { addStyle(it.appearance(colors), start, length) }
      }
    }
  }
}

@Composable
internal fun renderedMarkdown(body: String): AnnotatedString {
  val colors = markdownColors()
  return remember(body, colors) {
    markdownPresentation(body, spans(body), null, colors).text
  }
}

internal fun wrapSelection(value: TextFieldValue, delimiter: String): TextFieldValue {
  val start = value.selection.min
  val end = value.selection.max
  val closing = if (delimiter == "[") "]" else delimiter
  val text = value.text.replaceRange(
    start,
    end,
    delimiter + value.text.substring(start, end) + closing
  )
  return TextFieldValue(text, TextRange(start + delimiter.length, end + delimiter.length))
}

internal fun applyListEdit(value: TextFieldValue, key: ListKey): TextFieldValue? {
  if (!value.selection.collapsed) return null
  val edit = listEdit(value.text, value.selection.start.toUInt(), key) ?: return null
  return TextFieldValue(
    value.text.replaceRange(edit.start.toInt(), edit.end.toInt(), edit.text),
    TextRange(edit.cursor.toInt())
  )
}

internal fun continueList(before: TextFieldValue, after: TextFieldValue): TextFieldValue {
  val at = before.selection.start
  if (before.selection.collapsed && after.text.length == before.text.length + 1 &&
    after.text == before.text.substring(0, at) + "\n" + before.text.substring(at)
  ) {
    return applyListEdit(before, ListKey.ENTER) ?: after
  }
  if (!before.selection.collapsed) {
    val prefix = before.text.substring(0, before.selection.min)
    val suffix = before.text.substring(before.selection.max)
    if (after.text.length == prefix.length + suffix.length + 1 &&
      after.text.startsWith(prefix) && after.text.endsWith(suffix)
    ) {
      val inserted = after.text.substring(prefix.length, prefix.length + 1)
      if (inserted in listOf("*", "_", "`", "~", "=", "[")) {
        return wrapSelection(before, inserted)
      }
    }
  }
  return after
}

internal fun prefixSelection(value: TextFieldValue, prefix: String): TextFieldValue {
  val start = if (value.selection.min == 0) {
    0
  } else {
    value.text.lastIndexOf('\n', value.selection.min - 1) + 1
  }
  val end = value.selection.max
  val selected = value.text.substring(start, end)
  val replacement = prefix + selected.replace("\n", "\n$prefix")
  val text = value.text.replaceRange(start, end, replacement)
  return TextFieldValue(
    text,
    TextRange(value.selection.min + prefix.length, end + replacement.length - selected.length)
  )
}

@Composable
internal fun MarkdownEditor(
  value: TextFieldValue,
  enabled: Boolean,
  changed: (TextFieldValue) -> Unit,
  focus: FocusRequester,
  focusedChanged: (Boolean) -> Unit,
  modifier: Modifier = Modifier,
  openLink: (MobileLink) -> Unit = {}
) {
  var focused by remember { mutableStateOf(false) }
  var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
  val colors = markdownColors()
  val parsed = remember(value.text) { editorMarkup(value.text) }
  val markup = parsed.spans
  val links = parsed.links
  val presentation = remember(value.text, value.selection, enabled, focused, colors, links) {
    markdownPresentation(
      value.text,
      markup,
      value.selection.takeIf { enabled && focused },
      colors,
      links
    )
  }
  val onChange by rememberUpdatedState(changed)
  val latestValue by rememberUpdatedState(value)
  val latestPresentation by rememberUpdatedState(presentation)
  val latestMarkup by rememberUpdatedState(markup)
  val latestLinks by rememberUpdatedState(links)
  val onOpenLink by rememberUpdatedState(openLink)
  val bringIntoView = remember { BringIntoViewRequester() }
  val margin = with(LocalDensity.current) { 12.dp.toPx() }
  BoxWithConstraints(modifier.testTag("editor-viewport")) {
    val viewport = maxHeight
    LaunchedEffect(focused, layout, value.selection, viewport) {
      if (focused) {
        layout?.let { result ->
          val at = presentation.offsetMapping.originalToTransformed(value.selection.end)
          bringIntoView.bringIntoView(result.getCursorRect(at).inflate(margin))
        }
      }
    }
    Box(Modifier.fillMaxWidth().verticalScroll(rememberScrollState())) {
      BasicTextField(
        value,
        onValueChange = { changed(continueList(value, it)) },
        cursorBrush = androidx.compose.ui.graphics.SolidColor(MaterialTheme.colorScheme.primary),
        readOnly = !enabled,
        visualTransformation = VisualTransformation { presentation },
        textStyle = MaterialTheme.typography.bodyLarge.copy(
          color = MaterialTheme.colorScheme.onSurface,
          fontSize = 20.sp,
          lineHeight = 24.sp
        ),
        onTextLayout = { layout = it },
        modifier = Modifier.fillMaxWidth().heightIn(min = viewport)
          .padding(horizontal = 20.dp, vertical = 16.dp).testTag("note-editor")
          .bringIntoViewRequester(bringIntoView)
          .drawWithContent {
            drawContent()
            layout?.let { result ->
              presentation.text.getStringAnnotations("open-link", 0, presentation.text.length)
                .forEach { icon ->
                  val box = result.getBoundingBox(icon.start)
                  val side = minOf(box.width - 4.dp.toPx(), 12.dp.toPx())
                  val left = box.left + 2.dp.toPx()
                  val top = box.center.y - side / 2
                  val path = Path().apply {
                    moveTo(left + side * 0.55f, top)
                    lineTo(left + side, top)
                    lineTo(left + side, top + side * 0.45f)
                    moveTo(left + side, top)
                    lineTo(left + side * 0.4f, top + side * 0.6f)
                    moveTo(left + side * 0.35f, top + side * 0.15f)
                    lineTo(left, top + side * 0.15f)
                    lineTo(left, top + side)
                    lineTo(left + side * 0.85f, top + side)
                    lineTo(left + side * 0.85f, top + side * 0.65f)
                  }
                  drawPath(
                    path,
                    colors.link,
                    style = Stroke(1.4.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round)
                  )
                }
            }
          }
          .focusRequester(focus)
          .onFocusChanged {
            focused = it.isFocused
            focusedChanged(focused)
          }.pointerInput(enabled) {
            awaitEachGesture {
              val down = awaitFirstDown(pass = PointerEventPass.Initial)
              val result = layout ?: return@awaitEachGesture
              if (result.layoutInput.text.isEmpty()) return@awaitEachGesture
              val icon = latestPresentation.text.getStringAnnotations(
                "open-link",
                0,
                latestPresentation.text.length
              ).firstOrNull {
                it.start in result.layoutInput.text.indices &&
                  result.getBoundingBox(it.start).contains(down.position)
              }
              if (icon != null) {
                down.consume()
                val up = waitForUpOrCancellation(pass = PointerEventPass.Initial)
                if (up != null) {
                  up.consume()
                  val at = icon.item.toInt()
                  latestLinks.find { it.start.toInt() == at }?.let(onOpenLink)
                }
                return@awaitEachGesture
              }
              val boundary = result.getOffsetForPosition(down.position)
              val offset = listOf(boundary, boundary - 1).firstOrNull {
                it in result.layoutInput.text.indices &&
                  result.getBoundingBox(it).contains(down.position)
              } ?: return@awaitEachGesture
              val original = latestPresentation.offsetMapping.transformedToOriginal(offset)
              val checkbox = latestMarkup.find {
                it.style is Style.Checkbox && original >= it.start.toInt() &&
                  original < it.end.toInt()
              }.takeIf { enabled }
              if (checkbox != null) {
                down.consume()
                val up = waitForUpOrCancellation(pass = PointerEventPass.Initial)
                if (up != null) {
                  up.consume()
                  toggleCheckbox(latestValue.text, checkbox.start)?.let { edit ->
                    onChange(
                      TextFieldValue(
                        latestValue.text.replaceRange(
                          edit.start.toInt(),
                          edit.end.toInt(),
                          edit.text
                        ),
                        latestValue.selection
                      )
                    )
                  }
                }
              }
            }
          }.onPreviewKeyEvent { event ->
            val key = when {
              event.key == Key.Tab && event.isShiftPressed -> ListKey.SHIFT_TAB
              event.key == Key.Tab -> ListKey.TAB
              event.key == Key.Enter && event.isShiftPressed -> ListKey.SHIFT_ENTER
              event.key == Key.Enter -> ListKey.ENTER
              else -> null
            }
            if (enabled && event.type == KeyEventType.KeyDown && key != null) {
              applyListEdit(value, key)?.let {
                changed(it)
                true
              } ?: false
            } else {
              false
            }
          }
      )
    }
  }
}

@Composable
internal fun EditorToolbar(
  value: TextFieldValue,
  changed: (TextFieldValue) -> Unit,
  enabled: Boolean = true,
  link: () -> Unit
) {
  val indent = remember(value) { applyListEdit(value, ListKey.TAB) }
  val outdent = remember(value) { applyListEdit(value, ListKey.SHIFT_TAB) }
  Surface(
    color = MaterialTheme.colorScheme.surfaceContainer,
    shadowElevation = 2.dp,
    tonalElevation = 2.dp
  ) {
    Row(Modifier.fillMaxWidth().height(48.dp).testTag("editor-toolbar")) {
      if (enabled) {
        // Indentation leads the row so list editing doesn't require scrolling.
        Row(Modifier.weight(1f).horizontalScroll(rememberScrollState())) {
          IconButton(enabled = outdent != null, onClick = { outdent?.let(changed) }) {
            AppIcon(R.drawable.outdent, "Outdent")
          }
          IconButton(enabled = indent != null, onClick = { indent?.let(changed) }) {
            AppIcon(R.drawable.indent, "Indent")
          }
          IconButton(onClick = link) { AppIcon(R.drawable.link, "Link") }
          listOf(
            Triple("Bold", "**", R.drawable.bold),
            Triple("Italic", "*", R.drawable.italic),
            Triple("Code", "`", R.drawable.code),
            Triple("Highlight", "==", R.drawable.highlight),
            Triple("Strike", "~~", R.drawable.strike)
          ).forEach { (label, delimiter, icon) ->
            IconButton(onClick = { changed(wrapSelection(value, delimiter)) }) {
              AppIcon(icon, label)
            }
          }
          listOf(
            Triple("Heading 1", "# ", R.drawable.heading_one),
            Triple("Heading 2", "## ", R.drawable.heading_two),
            Triple("Quote", "> ", R.drawable.quote),
            Triple("Bullets", "- ", R.drawable.bullets),
            Triple("Numbers", "1. ", R.drawable.numbers),
            Triple("Checklist", "- [ ] ", R.drawable.checklist)
          ).forEach { (label, prefix, icon) ->
            IconButton(onClick = { changed(prefixSelection(value, prefix)) }) {
              AppIcon(icon, label)
            }
          }
        }
      }
    }
  }
}
