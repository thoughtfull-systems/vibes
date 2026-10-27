// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind
import systems.thoughtfull.commonplace.core.Times

internal fun journalInk(name: String, dark: Boolean): Color {
  val shades = when (name) {
    "brown" -> 0xFF8F6A45 to 0xFFB08A63
    "gray" -> 0xFF7A7A74 to 0xFF9A9A93
    "green" -> 0xFF008300 to 0xFF008300
    "orange" -> 0xFFEB6834 to 0xFFD95926
    "purple" -> 0xFF6250D6 to 0xFF9085E9
    "red" -> 0xFFE34948 to 0xFFE66767
    "yellow" -> 0xFFEDA100 to 0xFFC98500
    else -> 0xFF2A78D6 to 0xFF3987E5
  }
  return Color(if (dark) shades.second else shades.first)
}

@Composable
internal fun JournalDot(color: String) {
  val ink = journalInk(color, isSystemInDarkTheme())
  Canvas(Modifier.size(12.dp).testTag("journal-dot-$color")) { drawCircle(ink) }
}

@Composable
internal fun JournalColorPicker(color: String, selected: (String) -> Unit) {
  var expanded by remember { mutableStateOf(false) }
  // Use the Linux picker's color order.
  val colors = listOf("blue", "green", "yellow", "orange", "red", "purple", "brown", "gray")
  Box {
    OutlinedButton(
      onClick = { expanded = true },
      shape = RoundedCornerShape(6.dp),
      modifier = Modifier.testTag("journal-color")
    ) {
      Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically
      ) {
        JournalDot(color)
        Text(color.replaceFirstChar { it.uppercase() })
        AppIcon(R.drawable.dropdown)
      }
    }
    DropdownMenu(expanded, onDismissRequest = { expanded = false }) {
      colors.forEach { choice ->
        DropdownMenuItem(
          text = { Text(choice.replaceFirstChar { it.uppercase() }) },
          onClick = {
            selected(choice)
            expanded = false
          },
          leadingIcon = { JournalDot(choice) },
          trailingIcon = { if (choice == color) AppIcon(R.drawable.actions) }
        )
      }
    }
  }
}

internal fun describeDays(rule: String): String {
  val days = ruleWeekdays(rule)
  if (days.size == 7) return "Every day"
  if (days == ruleWeekdays(WORKDAYS)) return "Workdays"
  return scheduleWeekdays.filter { it.first in days }.joinToString(", ") { it.second }
    .ifBlank { "Custom days" }
}

internal fun describeTimes(times: Times): String = when (times) {
  is Times.Fixed -> "At " + times.v1.joinToString(", ") { minuteLabel(it.toInt()) }

  is Times.Random ->
    "About every ${times.mean} minutes from " +
      "${minuteLabel(times.start.toInt())} until ${minuteLabel(times.end.toInt())}"
}

internal fun listTimestamp(
  timestamp: Long,
  zone: ZoneId = ZoneId.systemDefault(),
  today: LocalDate = LocalDate.now(zone)
): String {
  val time = Instant.ofEpochMilli(timestamp).atZone(zone)
  val pattern = when {
    time.toLocalDate() == today -> "HH:mm"
    time.year == today.year -> "MMM d"
    else -> "yyyy-MM-dd"
  }
  return time.format(DateTimeFormatter.ofPattern(pattern))
}

internal fun entryTime(timestamp: Long): String = Instant.ofEpochMilli(timestamp)
  .atZone(ZoneId.systemDefault()).format(DateTimeFormatter.ofPattern("HH:mm"))

// Match the Linux dropdown's increasing ranges, including today in each range.
internal enum class JournalRange(val label: String, val days: Int) {
  TODAY("Today", 1),
  WEEK("Last 7 Days", 7),
  MONTH("Last 30 Days", 30),
  QUARTER("Last 90 Days", 90),
  YEAR("Last Year", 365),
  ALL("All Time", 0);

  fun since(today: LocalDate = LocalDate.now(), zone: ZoneId = ZoneId.systemDefault()): Long =
    if (days == 0) {
      0
    } else {
      today.minusDays(days.toLong() - 1).atStartOfDay(zone).toInstant().toEpochMilli()
    }
}

// In the order the Linux question type menu lists them.
internal val questionKinds = listOf(
  "Text" to QuestionKind.Text,
  "Short text" to QuestionKind.ShortText,
  "Number" to QuestionKind.Number(null, null, null),
  "Scale" to QuestionKind.Scale(1, 5, null, null),
  "Choice" to QuestionKind.Choice(false, emptyList()),
  "Yes or no" to QuestionKind.YesNo,
  "Duration" to QuestionKind.Duration,
  "Time of day" to QuestionKind.TimeOfDay
)

internal fun questionKindName(kind: QuestionKind) =
  questionKinds.first { it.second::class == kind::class }.first

/** A short description of a question's type and its limits. */
internal fun describeQuestion(question: Question): String {
  val name = questionKindName(question.kind)
  return when (val kind = question.kind) {
    is QuestionKind.Number -> kind.unit?.let { "$name, in $it" } ?: name

    is QuestionKind.Scale -> "$name from ${kind.min} to ${kind.max}"

    is QuestionKind.Choice -> {
      val labels = kind.options.filter { it.retired == null }.joinToString(", ") { it.label }
      "$name: ${if (kind.multiple) "any of" else "one of"} $labels"
    }

    else -> name
  }
}

internal const val WORKDAYS = "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"

// In calendar order, which the recurrence rule uses for its days.
private val ruleOrder = listOf("MO", "TU", "WE", "TH", "FR", "SA", "SU")

/** The days of the week as the Linux schedule editor shows them, starting on Sunday. */
internal val scheduleWeekdays = listOf(
  "SU" to "Su",
  "MO" to "Mo",
  "TU" to "Tu",
  "WE" to "We",
  "TH" to "Th",
  "FR" to "Fr",
  "SA" to "Sa"
)

internal fun ruleWeekdays(rule: String): Set<String> = if (rule == "FREQ=DAILY") {
  ruleOrder.toSet()
} else {
  rule.split(';').find { it.startsWith("BYDAY=") }?.substringAfter('=')?.split(',')
    ?.toSet().orEmpty()
}

/** The rule with `day` turned on or off, writing every day as the core does. */
internal fun toggleWeekday(rule: String, day: String): String {
  val current = ruleWeekdays(rule)
  val next = if (day in current) current - day else current + day
  return when {
    next.isEmpty() -> rule
    next.size == ruleOrder.size -> "FREQ=DAILY"
    else -> "FREQ=WEEKLY;BYDAY=" + ruleOrder.filter { it in next }.joinToString(",")
  }
}
