// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import systems.thoughtfull.commonplace.core.MobileAction

internal enum class StepPlacement { AFTER, BEFORE, INSIDE }
internal data class StepDrop(val id: String, val placement: StepPlacement)
internal data class StepDropRow(
  val id: String,
  val ancestors: List<String>,
  val bounds: Rect,
  val titleEnd: Float
)

internal fun stepDrop(rows: List<StepDropRow>, dragged: String, pointer: Offset): StepDrop? {
  if (rows.any { row ->
      (row.id == dragged || dragged in row.ancestors) && row.bounds.contains(pointer)
    }
  ) {
    return null
  }
  val eligible = rows.filter {
    it.id != dragged && dragged !in it.ancestors && it.bounds.height > 0
  }
  var previous: StepDropRow? = null
  for (row in eligible) {
    if (pointer.y < row.bounds.top) {
      return previous?.let { StepDrop(it.id, StepPlacement.AFTER) }
        ?: StepDrop(row.id, StepPlacement.BEFORE)
    }
    if (pointer.y <= row.bounds.bottom) {
      val place = when {
        pointer.x in row.bounds.left..row.titleEnd -> StepPlacement.INSIDE
        pointer.y < row.bounds.center.y -> StepPlacement.BEFORE
        else -> StepPlacement.AFTER
      }
      return StepDrop(row.id, place)
    }
    previous = row
  }
  return previous?.let { StepDrop(it.id, StepPlacement.AFTER) }
}

@Composable
internal fun StepRows(
  model: NotebookModel,
  steps: List<MobileAction>,
  context: RowContext,
  editable: Boolean,
  scroll: ScrollState,
  viewport: Rect,
  open: (String) -> Unit,
  reveal: String?,
  revealed: () -> Unit
) {
  val bounds = remember { mutableStateMapOf<String, Rect>() }
  val handles = remember { mutableStateMapOf<String, Offset>() }
  var dragged by remember { mutableStateOf<String?>(null) }
  var pointer by remember { mutableStateOf(Offset.Zero) }
  val haptics = LocalHapticFeedback.current
  val density = LocalDensity.current
  val titleWidth = with(density) { 148.dp.toPx() }
  val edge = with(density) { 36.dp.toPx() }
  val drop = dragged?.let { source ->
    stepDrop(
      steps.mapNotNull { step ->
        bounds[step.note.id]?.let {
          StepDropRow(step.note.id, step.parents, it, it.left + titleWidth)
        }
      },
      source,
      pointer
    )
  }
  val currentSteps by rememberUpdatedState(steps)
  val currentViewport by rememberUpdatedState(viewport)
  LaunchedEffect(dragged, pointer, viewport) {
    if (dragged == null) return@LaunchedEffect
    val visible = currentViewport
    val distance = when {
      pointer.y < visible.top + edge -> pointer.y - visible.top - edge
      pointer.y > visible.bottom - edge -> pointer.y - visible.bottom + edge
      else -> 0f
    }
    val speed = (distance / edge).coerceIn(-1f, 1f)
    while (dragged != null && speed != 0f) {
      if (scroll.scrollBy(speed * with(density) { 8.dp.toPx() }) == 0f) break
      withFrameNanos { }
    }
  }
  if (editable && steps.isNotEmpty()) {
    Text(
      "Drag a handle to reorder; drop on a title to nest.",
      Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
      style = MaterialTheme.typography.labelSmall,
      color = MaterialTheme.colorScheme.onSurfaceVariant
    )
  }
  val accent = MaterialTheme.colorScheme.primary
  val inside = MaterialTheme.colorScheme.primaryContainer
  Column {
    steps.forEach { step ->
      key(step.note.id) {
        val id = step.note.id
        val bringIntoView = remember { BringIntoViewRequester() }
        LaunchedEffect(reveal) {
          if (reveal == id) {
            withFrameNanos { }
            bringIntoView.bringIntoView()
            revealed()
          }
        }
        DisposableEffect(id) {
          onDispose {
            bounds.remove(id)
            handles.remove(id)
          }
        }
        val target = drop?.takeIf { it.id == id }
        Row(
          Modifier.fillMaxWidth().bringIntoViewRequester(bringIntoView)
            .padding(start = (step.depth.toInt().coerceAtMost(6) * 12).dp)
            .testTag("step-row-$id").onGloballyPositioned { bounds[id] = it.boundsInRoot() }
            .alpha(if (dragged == id) 0.45f else 1f)
            .background(
              if (target?.placement == StepPlacement.INSIDE) {
                inside
              } else {
                androidx.compose.ui.graphics.Color.Transparent
              }
            )
            .drawBehind {
              if (target != null && target.placement != StepPlacement.INSIDE) {
                val y = if (target.placement == StepPlacement.BEFORE) 0f else size.height
                drawLine(accent, Offset(44.dp.toPx(), y), Offset(size.width, y), 3.dp.toPx())
              }
            }
        ) {
          Box(Modifier.weight(1f)) {
            NoteRow(
              step.note,
              context,
              open,
              dates = step.dates,
              showPath = false,
              stalled = step.stalled,
              complete = if (editable) {
                { toggleAction(model, step.note) }
              } else {
                null
              }
            )
          }
          if (editable) {
            Box(
              Modifier.padding(top = 8.dp).size(40.dp).testTag("step-drag-$id")
                .onGloballyPositioned { handles[id] = it.positionInRoot() }
                .pointerInput(id) {
                  detectDragGestures(
                    onDragStart = { at ->
                      dragged = id
                      pointer = (handles[id] ?: Offset.Zero) + at
                      haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    },
                    onDragCancel = { dragged = null },
                    onDragEnd = {
                      val destination = stepDrop(
                        currentSteps.mapNotNull { item ->
                          bounds[item.note.id]?.let {
                            StepDropRow(item.note.id, item.parents, it, it.left + titleWidth)
                          }
                        },
                        id,
                        pointer
                      )
                      dragged = null
                      if (destination != null) {
                        model.perform { core ->
                          when (destination.placement) {
                            StepPlacement.INSIDE -> core.moveAction(id, destination.id, null)

                            else -> core.moveActionBeside(
                              id,
                              destination.id,
                              destination.placement == StepPlacement.AFTER
                            )
                          }
                        }
                      }
                    }
                  ) { change, _ ->
                    change.consume()
                    pointer = (handles[id] ?: Offset.Zero) + change.position
                  }
                },
              contentAlignment = Alignment.Center
            ) { AppIcon(R.drawable.drag, "Drag step") }
          }
        }
      }
    }
  }
}
