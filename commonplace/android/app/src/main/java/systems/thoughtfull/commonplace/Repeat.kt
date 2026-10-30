// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package systems.thoughtfull.commonplace

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import java.time.LocalDate
import systems.thoughtfull.commonplace.core.DayKind
import systems.thoughtfull.commonplace.core.DayOffEffect
import systems.thoughtfull.commonplace.core.DaysOffKind
import systems.thoughtfull.commonplace.core.DaysOffUse
import systems.thoughtfull.commonplace.core.Ends
import systems.thoughtfull.commonplace.core.Frequency
import systems.thoughtfull.commonplace.core.Missed
import systems.thoughtfull.commonplace.core.MonthlyOn
import systems.thoughtfull.commonplace.core.RecurrenceSettings
import systems.thoughtfull.commonplace.core.RepeatForm
import systems.thoughtfull.commonplace.core.Rule
import systems.thoughtfull.commonplace.core.daysOffKindName
import systems.thoughtfull.commonplace.core.describeRule
import systems.thoughtfull.commonplace.core.newRepeatForm
import systems.thoughtfull.commonplace.core.ordinalWord
import systems.thoughtfull.commonplace.core.repeatForm
import systems.thoughtfull.commonplace.core.repeatFormRule
import systems.thoughtfull.commonplace.core.repeatPresets

/** What the owner picked in a repeat menu. */
internal sealed class RepeatChoice {
  data object None : RepeatChoice()
  data class Preset(val rule: Rule) : RepeatChoice()
  data object Custom : RepeatChoice()
}

/**
 * A button that shows how something repeats, by a rule that counts from `start`, with the same
 * menu as the Linux app: presets that fall on `day`, Custom, and, where the owner can turn
 * repeating off, "Doesn't repeat." With `unread` set and no `rule`, it shows a rule this version
 * can't read.
 */
@Composable
internal fun RepeatMenu(
  rule: Rule?,
  start: String,
  allowNone: Boolean,
  modifier: Modifier = Modifier,
  day: String = start,
  unread: Boolean = false,
  choose: (RepeatChoice) -> Unit
) {
  var expanded by remember { mutableStateOf(false) }
  fun pick(choice: RepeatChoice) {
    expanded = false
    choose(choice)
  }
  Box(modifier) {
    OutlinedButton(
      onClick = { expanded = true },
      colors = neutralButtonColors(),
      shape = RoundedCornerShape(6.dp),
      contentPadding = PaddingValues(horizontal = 12.dp)
    ) {
      AppIcon(R.drawable.repeat, modifier = Modifier.padding(end = 6.dp))
      Text(
        rule?.let { describeRule(it, start) } ?: if (unread) "Custom repeat" else "Doesn't repeat",
        Modifier.weight(1f, fill = false),
        maxLines = 1,
        overflow = TextOverflow.Ellipsis
      )
      AppIcon(R.drawable.dropdown)
    }
    DropdownMenu(expanded, onDismissRequest = { expanded = false }) {
      if (allowNone) {
        DropdownMenuItem(
          text = { Text("Doesn't repeat") },
          onClick = { pick(RepeatChoice.None) },
          trailingIcon = { if (rule == null && !unread) AppIcon(R.drawable.actions) }
        )
        HorizontalDivider()
      }
      repeatPresets(day).forEach { preset ->
        DropdownMenuItem(
          text = { Text(describeRule(preset, day)) },
          onClick = { pick(RepeatChoice.Preset(preset)) },
          trailingIcon = { if (preset == rule) AppIcon(R.drawable.actions) }
        )
      }
      HorizontalDivider()
      DropdownMenuItem(text = { Text("Custom…") }, onClick = { pick(RepeatChoice.Custom) })
    }
  }
}

/** What the custom repeat editor describes. */
internal data class RepeatValues(val rule: Rule, val start: String)

private val units = listOf("day", "week", "month", "year")
private val frequencies =
  listOf(Frequency.DAILY, Frequency.WEEKLY, Frequency.MONTHLY, Frequency.YEARLY)
private val shortDays = listOf("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")

/** The days of the week from Sunday, numbered from 0 for Monday, as the core's `WEEK` has them. */
private val week = listOf(6, 0, 1, 2, 3, 4, 5)
private val dayNames =
  listOf("Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday")
private val monthNames = listOf(
  "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
  "November", "December"
)

/** The ordinals the monthly "On" menu offers after "Day", in the Linux app's order. */
private val ordinals = listOf<Byte>(1, 2, 3, 4, 5, -1)
internal val missedChoices = listOf(
  "Keep each one" to Missed.ALL,
  "Only the latest" to Missed.LATEST,
  "Count from when done" to Missed.AFTER
)
internal val effectChoices = listOf(
  "Ignore" to DayOffEffect.IGNORE,
  "Skip" to DayOffEffect.SKIP,
  "Move to next day" to DayOffEffect.MOVE
)

/** A label and a control on one line, like the Linux editor's grid. */
@Composable
internal fun EditorRow(label: String, control: @Composable () -> Unit) {
  Row(
    Modifier.fillMaxWidth(),
    horizontalArrangement = Arrangement.spacedBy(12.dp),
    verticalAlignment = Alignment.CenterVertically
  ) {
    Text(label, Modifier.width(88.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
    Box(Modifier.weight(1f)) { control() }
  }
}

/** A number field for whole numbers, empty while the owner types a new one. */
@Composable
private fun NumberField(value: String, change: (String) -> Unit, modifier: Modifier = Modifier) {
  CompactTextField(
    value,
    { text -> change(text.filter(Char::isDigit).take(3)) },
    "1",
    modifier.width(72.dp),
    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number)
  )
}

/**
 * A checkbox and a number of days before or after an occurrence's date, as the Linux app's row
 * shows them. `sign` is -1 for days before. `key` names what the row belongs to, so that the
 * number the owner is typing stays put while saves of its first digits come back.
 */
@Composable
internal fun OffsetRow(
  key: String,
  offset: Int?,
  sign: Int,
  after: String,
  change: (Int?) -> Unit
) {
  var text by remember(key, offset == null) {
    mutableStateOf(((offset ?: 0) * sign).coerceAtLeast(0).toString())
  }
  Row(verticalAlignment = Alignment.CenterVertically) {
    Checkbox(offset != null, { on -> change(if (on) (text.toIntOrNull() ?: 0) * sign else null) })
    NumberField(text, { typed ->
      text = typed
      typed.toIntOrNull()?.let { if (offset != null) change(it * sign) }
    })
    Text(" $after")
  }
}

/**
 * The custom repeat editor, as a full-screen dialog, with the Starting row if `showStart` is set.
 * The preview moves dates around the days off of `noteId`'s library as `daysOff` says.
 */
@Composable
internal fun RepeatEditorDialog(
  model: NotebookModel,
  noteId: String?,
  rule: Rule?,
  start: String,
  showStart: Boolean,
  daysOff: DaysOffUse,
  dismiss: () -> Unit,
  save: (RepeatValues) -> Unit
) {
  val form = remember { rule?.let { repeatForm(it, start) } ?: newRepeatForm(start) }
  var frequency by remember { mutableStateOf(form.frequency) }
  var interval by remember { mutableStateOf(form.interval.toString()) }
  var weekdays by remember { mutableStateOf(form.weekdays.map { it.toInt() }.toSet()) }
  val formOn = form.on
  var pick by remember {
    mutableStateOf(
      when {
        formOn is MonthlyOn.Day && formOn.day > 0 -> 0

        formOn is MonthlyOn.Day -> ordinals.indexOf(formOn.day).let { if (it < 0) 6 else it + 1 }

        formOn is MonthlyOn.Nth -> ordinals.indexOf(formOn.ordinal).let {
          if (it <
            0
          ) {
            6
          } else {
            it + 1
          }
        }

        else -> 0
      }
    )
  }
  var day by remember {
    mutableStateOf(
      ((formOn as? MonthlyOn.Day)?.day?.takeIf { it > 0 } ?: LocalDate.parse(start).dayOfMonth)
        .toString()
    )
  }
  var kind by remember {
    mutableStateOf(
      when (val picked = (formOn as? MonthlyOn.Nth)?.kind) {
        is DayKind.Weekday -> 2 + week.indexOf(picked.weekday.toInt()).coerceAtLeast(0)
        is DayKind.Workday -> 1
        else -> 0
      }
    )
  }
  var month by remember { mutableStateOf(form.month.toInt().coerceIn(1, 12)) }
  var startDate by remember { mutableStateOf(start) }
  val formEnds = form.ends
  var ends by remember {
    mutableStateOf(
      when (formEnds) {
        is Ends.On -> 1
        is Ends.After -> 2
        else -> 0
      }
    )
  }
  var until by remember {
    mutableStateOf((formEnds as? Ends.On)?.date ?: LocalDate.parse(start).plusDays(365).toString())
  }
  var count by remember { mutableStateOf(((formEnds as? Ends.After)?.count ?: 10u).toString()) }
  var picking by remember { mutableStateOf<String?>(null) }

  fun values(): RepeatValues {
    val on = if (pick == 0) {
      MonthlyOn.Day((day.toIntOrNull() ?: 1).coerceIn(1, 31).toByte())
    } else {
      val picked = when (kind) {
        0 -> DayKind.Day
        1 -> DayKind.Workday
        else -> DayKind.Weekday(week[(kind - 2).coerceIn(0, 6)].toUByte())
      }
      MonthlyOn.Nth(ordinals[(pick - 1).coerceIn(0, ordinals.size - 1)], picked)
    }
    val days = weekdays.ifEmpty { setOf(LocalDate.parse(startDate).dayOfWeek.value - 1) }
    val ending = when (ends) {
      1 -> Ends.On(until)
      2 -> Ends.After((count.toIntOrNull() ?: 1).coerceAtLeast(1).toUInt())
      else -> Ends.Never
    }
    val repeat = RepeatForm(
      frequency,
      (interval.toIntOrNull() ?: 1).coerceAtLeast(1).toUInt(),
      days.sorted().map { it.toByte() }.toByteArray(),
      on,
      month.toUByte(),
      ending
    )
    return RepeatValues(repeatFormRule(repeat), startDate)
  }
  val current = values()
  var preview by remember { mutableStateOf("") }
  LaunchedEffect(current) {
    val settings = RecurrenceSettings(current.rule, current.start, Missed.ALL, 0, null, daysOff)
    val from = maxOf(LocalDate.now().toString(), current.start)
    val dates = model.read { it.previewDates(noteId, null, settings, from, 3u) }.orEmpty()
    preview =
      if (dates.isEmpty()) "No dates" else "Next: " + dates.joinToString(" · ") { formatDate(it) }
  }
  val plural = (interval.toIntOrNull() ?: 1) != 1
  val unitNames = units.map { if (plural) "${it}s" else it }

  Dialog(dismiss, DialogProperties(usePlatformDefaultWidth = false)) {
    Scaffold(
      modifier = Modifier.imePadding(),
      topBar = {
        TopAppBar(
          title = { Text("Repeat") },
          navigationIcon = { IconButton(onClick = dismiss) { AppIcon(R.drawable.back, "Cancel") } },
          actions = { TextButton(onClick = { save(values()) }) { Text("Save") } }
        )
      }
    ) { padding ->
      Column(
        Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
          .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
      ) {
        EditorRow("Every") {
          Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            NumberField(interval, { interval = it })
            MenuControl(frequency, frequencies, { unitNames[frequencies.indexOf(it)] }) {
              frequency = it
            }
          }
        }
        if (frequency == Frequency.WEEKLY) {
          EditorRow("On") {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
              week.forEach { index ->
                FilterChip(
                  index in weekdays,
                  onClick = {
                    weekdays =
                      if (index in weekdays) weekdays - index else weekdays + index
                  },
                  label = { Text(shortDays[index].take(2)) }
                )
              }
            }
          }
        }
        if (frequency == Frequency.MONTHLY || frequency == Frequency.YEARLY) {
          if (frequency == Frequency.YEARLY) {
            EditorRow("In") {
              MenuControl(month, (1..12).toList(), { monthNames[it - 1] }) { month = it }
            }
          }
          EditorRow("On") {
            val picks = listOf("Day") + ordinals.map { "The ${ordinalWord(it)}" }
            MenuControl(pick, picks.indices.toList(), { picks[it] }) { pick = it }
          }
          EditorRow("") {
            if (pick == 0) {
              NumberField(day, { day = it })
            } else {
              val kinds = listOf("day", "workday") + week.map { dayNames[it] }
              MenuControl(kind, kinds.indices.toList(), { kinds[it] }) { kind = it }
            }
          }
        }
        if (showStart) {
          EditorRow("Starting") {
            TextButton(onClick = { picking = "start" }) { Text(formatDate(startDate)) }
          }
        }
        EditorRow("Ends") {
          Row(verticalAlignment = Alignment.CenterVertically) {
            val choices = listOf("Never", "On a date", "After")
            MenuControl(ends, choices.indices.toList(), { choices[it] }) { ends = it }
            when (ends) {
              1 -> TextButton(onClick = { picking = "until" }) { Text(formatDate(until)) }

              2 -> {
                Box(Modifier.padding(start = 8.dp)) { NumberField(count, { count = it }) }
                Text(" times")
              }
            }
          }
        }
        Text(
          preview,
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          style = MaterialTheme.typography.bodySmall
        )
      }
    }
  }
  picking?.let { field ->
    val initial = if (field == "start") startDate else until
    CalendarDialog(LocalDate.parse(initial), { picking = null }, clearLabel = "Today") {
      val picked = (it ?: LocalDate.now()).toString()
      if (field == "start") startDate = picked else until = picked
      picking = null
    }
  }
}

/** A row for each kind of days off, with what a rule does on them. */
@Composable
internal fun DaysOffRows(
  holidays: DayOffEffect,
  vacations: DayOffEffect,
  setHolidays: (DayOffEffect) -> Unit,
  setVacations: (DayOffEffect) -> Unit
) {
  listOf(
    Triple(DaysOffKind.HOLIDAYS, holidays, setHolidays),
    Triple(DaysOffKind.VACATIONS, vacations, setVacations)
  ).forEach { (kind, effect, set) ->
    EditorRow(daysOffKindName(kind)) {
      MenuControl(effect, effectChoices.map { it.second }, { choice ->
        effectChoices.first { it.second == choice }.first
      }) { set(it) }
    }
  }
}
