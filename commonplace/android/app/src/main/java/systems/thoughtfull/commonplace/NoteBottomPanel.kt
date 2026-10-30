// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalMinimumInteractiveComponentSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.math.roundToLong
import systems.thoughtfull.commonplace.core.ActionState
import systems.thoughtfull.commonplace.core.MobileAction
import systems.thoughtfull.commonplace.core.MobileNote

/** The height in dp the owner dragged a note's panel to, which the spec documents. */
internal const val BOTTOM_HEIGHT = "android.bottom.height"

private val DIVIDER_REACH = 12.dp
private val MIN_PANEL_HEIGHT = 96.dp
private val COMPACT_BUTTON_PADDING = PaddingValues(horizontal = 8.dp, vertical = 4.dp)

/**
 * Where the line above a note's panel is, in the coordinates of the screen's column that holds the
 * note's text and the panel, and what dragging it or unpinning the panel does.
 */
internal class PanelDivider {
  var top by mutableStateOf<Float?>(null)
  var pinned by mutableStateOf(false)
  var drag: (Float) -> Unit = {}
  var started: () -> Unit = {}
  var stopped: () -> Unit = {}
  var unpin: () -> Unit = {}
}

/**
 * Drags the panel's line from anywhere within [DIVIDER_REACH] of it, as GTK reaches past its thin
 * handle on Linux. Watching the column before its children leaves taps near the line to them,
 * and claims a press only once it moves farther than a tap.
 */
internal fun Modifier.panelDividerDrag(divider: PanelDivider) = pointerInput(divider) {
  awaitEachGesture {
    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
    val top = divider.top ?: return@awaitEachGesture
    if (abs(down.position.y - top) > DIVIDER_REACH.toPx()) return@awaitEachGesture
    var moved = 0f
    var dragging = false
    while (true) {
      val change = awaitPointerEvent(PointerEventPass.Initial).changes
        .firstOrNull { it.id == down.id } ?: break
      if (!change.pressed) break
      val delta = change.position.y - change.previousPosition.y
      if (dragging) {
        divider.drag(delta)
      } else {
        moved += delta
        if (abs(moved) > viewConfiguration.touchSlop) {
          dragging = true
          divider.started()
          divider.drag(moved)
        }
      }
      if (dragging) change.consume()
    }
    if (dragging) divider.stopped()
  }
}

/** The unpin button on the panel's line, while the panel has a height of its own. */
@Composable
internal fun BoxScope.PanelUnpin(divider: PanelDivider) {
  val top = divider.top?.takeIf { divider.pinned } ?: return
  val target = 40.dp
  IconButton(
    onClick = divider.unpin,
    Modifier.align(Alignment.TopStart)
      .offset { IntOffset(2.dp.roundToPx(), (top - target.toPx() / 2).roundToInt()) }
      .size(target).testTag("panel-unpin")
  ) {
    AppIcon(R.drawable.pin, "Unpin panel height", Modifier.size(14.dp))
  }
}

internal fun visibleSteps(steps: List<MobileAction>, showDone: Boolean): List<MobileAction> {
  if (showDone) return steps
  var hiddenBelow: UInt? = null
  return steps.filter { step ->
    if (hiddenBelow?.let { step.depth > it } == true) {
      false
    } else {
      val done = step.note.action?.state == ActionState.DONE
      hiddenBelow = step.depth.takeIf { done }
      !done
    }
  }
}

@Composable
internal fun NoteBottomPanel(
  model: NotebookModel,
  note: MobileNote,
  backlinks: List<MobileNote>,
  open: (String) -> Unit,
  automaticLimit: Dp,
  maxHeight: Dp,
  move: () -> Unit,
  modifier: Modifier = Modifier,
  divider: PanelDivider = remember { PanelDivider() }
) {
  if (note.action == null && backlinks.isEmpty()) return
  val state by model.state.collectAsState()
  val context = rememberRowContext(state)
  var showDone by remember(note.id) { mutableStateOf(false) }
  var steps by remember(note.id) { mutableStateOf(emptyList<MobileAction>()) }
  var stepText by rememberSaveable(note.id) { mutableStateOf("") }
  var adding by remember(note.id) { mutableStateOf(false) }
  var revealStep by remember(note.id) { mutableStateOf<String?>(null) }
  fun addStep() {
    if (adding || stepText.isBlank()) return
    adding = true
    val text = stepText.trim()
    var created: String? = null
    model.perform(complete = { success ->
      adding = false
      if (success) {
        revealStep = created
        if (stepText.trim() == text) stepText = ""
      }
    }) { created = it.createStep(note.id, text) }
  }
  val parents = remember(note.id, note.parent, state.notes) {
    val notes = state.notes.associateBy { it.id }
    val seen = mutableSetOf(note.id)
    buildList {
      var parent = note.parent
      while (parent != null && seen.add(parent)) {
        val ancestor = notes[parent] ?: break
        add(ancestor)
        parent = ancestor.parent
      }
    }.asReversed()
  }
  var pinned by remember(note.id) { mutableStateOf<Dp?>(null) }
  var measured by remember { mutableStateOf(0.dp) }
  LaunchedEffect(note.id, state.revision) {
    pinned = model.read { it.noteSetting(note.id, BOTTOM_HEIGHT) }?.toInt()?.dp
    if (note.action != null) {
      showDone = model.read { it.showsDoneSteps(note.id) } ?: showDone
      steps = model.read { it.actionSteps(note.id, LocalDate.now().toString()) }.orEmpty()
    }
  }
  val density = LocalDensity.current
  val tallest = maxHeight.coerceAtLeast(MIN_PANEL_HEIGHT)
  divider.pinned = pinned != null
  divider.started = { pinned = (pinned ?: measured).coerceIn(MIN_PANEL_HEIGHT, tallest) }
  divider.drag = { delta ->
    pinned = ((pinned ?: measured) - with(density) { delta.toDp() })
      .coerceIn(MIN_PANEL_HEIGHT, tallest)
  }
  divider.stopped = {
    val height = pinned
    if (height != null && !note.readOnly) {
      model.perform { it.setNoteSetting(note.id, BOTTOM_HEIGHT, height.value.roundToLong()) }
    }
  }
  divider.unpin = {
    pinned = null
    if (!note.readOnly) model.perform { it.removeNoteSetting(note.id, BOTTOM_HEIGHT) }
  }
  DisposableEffect(divider) { onDispose { divider.top = null } }
  val scroll = rememberScrollState()
  var viewport by remember { mutableStateOf(Rect.Zero) }
  val editable = !note.readOnly && !note.inTrash
  Column(
    modifier.then(
      pinned?.let { Modifier.height(it.coerceIn(MIN_PANEL_HEIGHT, tallest)) }
        ?: Modifier.heightIn(max = automaticLimit)
    ).onSizeChanged { measured = with(density) { it.height.toDp() } }
      .onGloballyPositioned { divider.top = it.positionInParent().y }
      .testTag("note-bottom-panel")
  ) {
    // The line starts past the unpin button, which PanelUnpin draws over its start.
    HorizontalDivider(Modifier.padding(start = if (pinned != null) 32.dp else 0.dp))
    CompositionLocalProvider(LocalMinimumInteractiveComponentSize provides 32.dp) {
      if (note.action != null) {
        Row(
          Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("action-parent"),
          verticalAlignment = Alignment.CenterVertically
        ) {
          Text("In", style = MaterialTheme.typography.bodySmall)
          Row(
            Modifier.weight(1f).horizontalScroll(rememberScrollState())
              .padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically
          ) {
            if (parents.isEmpty()) Text("Top level")
            parents.forEachIndexed { index, parent ->
              if (index > 0) Text(" › ")
              TextButton(
                onClick = { open(parent.id) },
                Modifier.heightIn(min = 32.dp),
                contentPadding = COMPACT_BUTTON_PADDING
              ) { Text(formattedTitle(displayLabel(parent))) }
            }
          }
          if (editable) {
            TextButton(
              onClick = move,
              Modifier.heightIn(min = 32.dp),
              contentPadding = COMPACT_BUTTON_PADDING
            ) { Text("Move…") }
          }
        }
        HorizontalDivider()
      }
      if (note.action != null && (editable || steps.isNotEmpty())) {
        Row(
          Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
          horizontalArrangement = Arrangement.spacedBy(6.dp),
          verticalAlignment = Alignment.CenterVertically
        ) {
          Text("Steps", Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
          if (steps.isNotEmpty()) {
            Text("Show done?", color = MaterialTheme.colorScheme.onSurfaceVariant)
            Switch(showDone, {
              showDone = it
              if (!note.readOnly) model.perform { core -> core.setShowsDoneSteps(note.id, it) }
            }, modifier = Modifier.testTag("steps-show-done"))
          }
        }
      }
    }
    Column(
      Modifier.weight(1f, fill = pinned != null).fillMaxWidth().verticalScroll(scroll)
        .testTag("step-scroll").onGloballyPositioned { viewport = it.boundsInRoot() }
    ) {
      if (note.action != null && (editable || steps.isNotEmpty())) {
        StepRows(
          model,
          visibleSteps(steps, showDone),
          context,
          editable,
          scroll,
          viewport,
          open,
          revealStep,
          { revealStep = null }
        )
      }
      if (backlinks.isNotEmpty()) {
        Text(
          "Linked from",
          Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
          style = MaterialTheme.typography.titleSmall
        )
        backlinks.forEach { linked ->
          NoteRow(linked, context, open)
        }
      }
    }
    if (editable && note.action != null) {
      CompactTextField(
        stepText,
        { stepText = it },
        "Add a step…",
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)
          .testTag("step-entry"),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { addStep() })
      )
    }
  }
}
