// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.temporal.ChronoUnit
import kotlin.math.roundToInt
import systems.thoughtfull.commonplace.core.Answer
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind

internal enum class DayMark { CROSS, DOT, EMPTY, MISSING }

internal data class DaySeries(val label: String, val marks: Map<LocalDate, DayMark>) {
  val count get() = marks.values.count { it == DayMark.DOT }
  val answered get() = marks.size
  fun mark(day: LocalDate) = marks[day] ?: DayMark.MISSING
}

internal fun daySeries(question: Question, samples: List<Pair<Long, Answer>>): List<DaySeries> {
  val days = samples.groupBy { (at, _) ->
    Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()).toLocalDate()
  }
  return when (val kind = question.kind) {
    is QuestionKind.Choice -> kind.options.map { option ->
      DaySeries(
        option.label,
        days.mapValues { (_, answers) ->
          if (answers.any { option.id in (it.second as? Answer.Choices)?.v1.orEmpty() }) {
            DayMark.DOT
          } else {
            DayMark.EMPTY
          }
        }
      ) to option.retired
    }.filter { (series, retired) -> retired == null || series.count > 0 }.map { it.first }

    QuestionKind.YesNo -> listOf(
      DaySeries(
        "Yes",
        days.mapValues { (_, answers) ->
          val yes = answers.any { (it.second as? Answer.Bool)?.v1 == true }
          if (yes) {
            DayMark.DOT
          } else {
            DayMark.CROSS
          }
        }
      )
    )

    else -> emptyList()
  }
}

internal fun closestPoint(
  points: List<Pair<Long, Float>>,
  fraction: Float,
  first: Long = points.first().first,
  last: Long = points.last().first
): Pair<Long, Float> {
  require(points.isNotEmpty())
  val at = first + (last - first) * fraction.coerceIn(0f, 1f).toDouble()
  val match = points.binarySearchBy(at) { it.first.toDouble() }
  if (match >= 0) return points[match]
  val right = -match - 1
  if (right == 0) return points.first()
  if (right == points.size) return points.last()
  val before = points[right - 1]
  val after = points[right]
  return if (at - before.first <= after.first - at) before else after
}

private fun Modifier.scrub(selection: MutableState<Float?>) = pointerInput(selection) {
  awaitEachGesture {
    val down = awaitFirstDown()
    selection.value = (down.position.x / size.width).coerceIn(0f, 1f)
    down.consume()
    do {
      val event = awaitPointerEvent()
      val change = event.changes.first()
      selection.value = (change.position.x / size.width).coerceIn(0f, 1f)
      change.consume()
    } while (event.changes.any { it.pressed })
  }
}

private fun chartNumber(value: Float, kind: QuestionKind): String = when (kind) {
  QuestionKind.TimeOfDay -> {
    val minutes = value.roundToInt().coerceIn(0, 1439)
    "%02d:%02d".format(minutes / 60, minutes % 60)
  }

  QuestionKind.Duration -> "${(value * 100).roundToInt() / 100.0} min"

  is QuestionKind.Number ->
    "${(value * 100).roundToInt() / 100.0}${kind.unit?.let { " $it" }.orEmpty()}"

  else -> ((value * 100).roundToInt() / 100.0).toString()
}

@Composable
internal fun JournalChart(
  question: Question,
  entries: List<MobileNote>,
  journalColor: String,
  since: Long
) {
  val samples = remember(question, entries) {
    entries.mapNotNull { note ->
      note.entry?.answers?.get(question.id)?.let { note.entry.at to it }
    }.sortedBy { it.first }
  }
  if (samples.isEmpty()) return
  val numbers = remember(samples) {
    samples.mapNotNull { (at, answer) ->
      val value = when (answer) {
        is Answer.Number -> answer.v1.toFloat()

        is Answer.Text -> runCatching {
          val time = LocalTime.parse(answer.v1)
          (time.hour * 60 + time.minute).toFloat()
        }.getOrNull().takeIf { question.kind == QuestionKind.TimeOfDay }

        else -> null
      }
      value?.let { at to it }
    }
  }
  val rows = remember(question, samples) { daySeries(question, samples) }
  if (numbers.isEmpty() && rows.isEmpty()) return
  val color = journalInk(journalColor, isSystemInDarkTheme())
  Column(Modifier.padding(16.dp).testTag("chart-${question.id}")) {
    Text(question.prompt, style = MaterialTheme.typography.titleSmall)
    val zone = ZoneId.systemDefault()
    val first = Instant.ofEpochMilli(
      if (since == 0L) {
        entries.mapNotNull { it.entry?.at }.min()
      } else {
        since
      }
    ).atZone(zone).toLocalDate()
    val last = maxOf(
      LocalDate.now(),
      entries.mapNotNull { it.entry?.at }.max().let {
        Instant.ofEpochMilli(it).atZone(zone).toLocalDate()
      }
    )
    if (numbers.isNotEmpty()) {
      NumberChart(question, numbers, color, first, last)
    } else {
      DayChart(question, rows, first, last, color)
    }
  }
}

@Composable
private fun NumberChart(
  question: Question,
  points: List<Pair<Long, Float>>,
  color: Color,
  first: LocalDate,
  last: LocalDate
) {
  val zone = ZoneId.systemDefault()
  val start = first.atStartOfDay(zone).toInstant().toEpochMilli()
  val end = last.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli()
  val selection = remember(question.id, points, first, last) { mutableStateOf<Float?>(null) }
  val selected = selection.value
  val ink = MaterialTheme.colorScheme.onSurfaceVariant
  val scale = question.kind as? QuestionKind.Scale
  var low = scale?.min?.toFloat() ?: points.minOf { it.second }
  var high = scale?.max?.toFloat() ?: points.maxOf { it.second }
  if (low == high) {
    low -= 1
    high += 1
  }
  val sample = selected?.let { closestPoint(points, it, start, end) }
  Text(
    sample?.let { "${dateLabel(it.first)} · ${chartNumber(it.second, question.kind)}" }
      ?: "${chartNumber(low, question.kind)} – ${chartNumber(high, question.kind)}",
    Modifier.testTag("chart-value-${question.id}"),
    style = MaterialTheme.typography.labelSmall
  )
  Canvas(
    Modifier.fillMaxWidth().height(150.dp).padding(vertical = 12.dp)
      .testTag("chart-plot-${question.id}").scrub(selection)
  ) {
    fun y(value: Float) = size.height * (high - value) / (high - low)
    val duration = (end - start).coerceAtLeast(1)
    fun x(at: Long) = (at - start).toFloat() / duration * size.width
    listOf(0f, size.height).forEach {
      drawLine(ink.copy(alpha = 0.15f), Offset(0f, it), Offset(size.width, it))
    }
    val placed = points.map { Offset(x(it.first), y(it.second)) }
    placed.zipWithNext().forEach { (a, b) -> drawLine(color, a, b, 2.dp.toPx()) }
    placed.forEach { drawCircle(color, 3.dp.toPx(), it) }
    sample?.let { (at, value) ->
      val line = x(at)
      drawLine(ink, Offset(line, 0f), Offset(line, size.height), 1.dp.toPx())
      drawCircle(color, 5.dp.toPx(), Offset(line, y(value)))
    }
  }
  Text("$first – $last", style = MaterialTheme.typography.labelSmall)
}

@Composable
private fun DayChart(
  question: Question,
  rows: List<DaySeries>,
  first: LocalDate,
  last: LocalDate,
  color: Color
) {
  val days = ChronoUnit.DAYS.between(first, last).toInt() + 1
  val selection = remember(question.id, first, last) { mutableStateOf<Float?>(null) }
  val selected = selection.value?.let { (it * days).toInt().coerceIn(0, days - 1) }
  val ink = MaterialTheme.colorScheme.onSurfaceVariant
  Text(
    selected?.let { index ->
      val day = first.plusDays(index.toLong())
      "$day · " + rows.joinToString(" · ") { row ->
        val state = when (row.mark(day)) {
          DayMark.CROSS -> "No"
          DayMark.DOT -> if (question.kind == QuestionKind.YesNo) "Yes" else "Picked"
          DayMark.EMPTY -> "Not picked"
          DayMark.MISSING -> "No answer"
        }
        if (question.kind == QuestionKind.YesNo) state else "${row.label}: $state"
      }
    } ?: "A dot for a selection, × for no, a dashed ring for no answer",
    Modifier.testTag("chart-value-${question.id}"),
    style = MaterialTheme.typography.labelSmall
  )
  rows.forEach { row ->
    Row(Modifier.fillMaxWidth().height(32.dp), verticalAlignment = Alignment.CenterVertically) {
      Text(
        row.label,
        Modifier.width(80.dp),
        style = MaterialTheme.typography.labelSmall,
        maxLines = 2
      )
      Canvas(
        Modifier.weight(1f).height(32.dp).testTag("chart-row-${question.id}-${row.label}")
          .scrub(selection)
      ) {
        val cell = size.width / days
        val radius = (cell / 2 - 1.dp.toPx()).coerceIn(1.dp.toPx(), 5.dp.toPx())
        for (index in 0 until days) {
          val center = Offset(cell * (index + 0.5f), size.height / 2)
          when (row.mark(first.plusDays(index.toLong()))) {
            DayMark.DOT -> drawCircle(color, radius, center)

            DayMark.EMPTY -> Unit

            DayMark.MISSING -> drawCircle(
              color.copy(alpha = 0.55f),
              radius,
              center,
              style = Stroke(
                1.dp.toPx(),
                pathEffect = PathEffect.dashPathEffect(floatArrayOf(3f, 3f))
              )
            )

            DayMark.CROSS -> {
              val arm = Offset(radius * 0.8f, radius * 0.8f)
              drawLine(color, center - arm, center + arm, 1.5.dp.toPx(), StrokeCap.Round)
              drawLine(
                color,
                center + Offset(-arm.x, arm.y),
                center + Offset(arm.x, -arm.y),
                1.5.dp.toPx(),
                StrokeCap.Round
              )
            }
          }
        }
        selected?.let {
          val x = cell * (it + 0.5f)
          drawLine(ink, Offset(x, 0f), Offset(x, size.height), 1.dp.toPx())
        }
      }
      Text(
        if (question.kind == QuestionKind.YesNo) "${row.count}/${row.answered}" else "${row.count}",
        Modifier.width(48.dp),
        textAlign = TextAlign.End,
        style = MaterialTheme.typography.labelSmall
      )
    }
  }
  Text("$first – $last", style = MaterialTheme.typography.labelSmall)
}

internal fun charts(kind: QuestionKind) =
  kind !is QuestionKind.Text && kind !is QuestionKind.ShortText
