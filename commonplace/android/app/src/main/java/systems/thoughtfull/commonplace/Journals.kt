// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.Manifest
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.ResolverStyle
import systems.thoughtfull.commonplace.core.Answer
import systems.thoughtfull.commonplace.core.ChoiceOption
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind
import systems.thoughtfull.commonplace.core.Schedule
import systems.thoughtfull.commonplace.core.Times
import systems.thoughtfull.commonplace.core.newId
import systems.thoughtfull.commonplace.core.parseTimeOfDay
import systems.thoughtfull.commonplace.core.scheduleIsValid

@Composable
internal fun AnswerField(
  question: Question,
  answer: Answer?,
  validity: (Boolean) -> Unit = {},
  changed: (Answer?) -> Unit
) {
  val kind = question.kind
  if (kind == QuestionKind.Text) return
  Text(question.prompt, style = MaterialTheme.typography.labelLarge)
  when (kind) {
    is QuestionKind.Choice -> ChipRow {
      val selected = (answer as? Answer.Choices)?.v1.orEmpty()
      kind.options.filter { it.retired == null || it.id in selected }.forEach { option ->
        FilterChip(option.id in selected, onClick = {
          val ids = when {
            option.id in selected -> selected - option.id
            kind.multiple -> selected + option.id
            else -> listOf(option.id)
          }
          changed(ids.takeIf { it.isNotEmpty() }?.let { Answer.Choices(it) })
        }, label = { Text(option.label) })
      }
    }

    is QuestionKind.Scale -> Row(
      Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
      horizontalArrangement = Arrangement.spacedBy(6.dp),
      verticalAlignment = Alignment.CenterVertically
    ) {
      kind.low?.let { Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant) }
      (kind.min..kind.max).forEach { value ->
        val selected = (answer as? Answer.Number)?.v1 == value.toDouble()
        FilterChip(
          selected,
          onClick = { changed(if (selected) null else Answer.Number(value.toDouble())) },
          label = { Text(value.toString()) },
          modifier = Modifier.testTag("answer-${question.id}-$value")
        )
      }
      kind.high?.let { Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant) }
    }

    QuestionKind.YesNo -> ChipRow {
      // Follow the Linux answer order.
      listOf(true, false).forEach { boolean ->
        FilterChip((answer as? Answer.Bool)?.v1 == boolean, onClick = {
          changed(
            if ((answer as? Answer.Bool)?.v1 ==
              boolean
            ) {
              null
            } else {
              Answer.Bool(boolean)
            }
          )
        }, label = { Text(if (boolean) "Yes" else "No") })
      }
    }

    else -> {
      val initial = when (answer) {
        is Answer.Number -> answer.v1.toString().removeSuffix(".0")
        is Answer.Text -> answer.v1
        else -> ""
      }
      var text by rememberSaveable(question.id) { mutableStateOf(initial) }
      val valid = text.isBlank() || typedAnswer(kind, text) != null
      LaunchedEffect(valid) { validity(valid) }
      val hint = when (kind) {
        is QuestionKind.Number -> kind.unit ?: "Number"
        QuestionKind.Duration -> "Minutes"
        QuestionKind.TimeOfDay -> "HH:MM"
        else -> "Answer"
      }
      OutlinedTextField(
        text,
        { value ->
          text = value
          if (value.isBlank()) {
            changed(null)
          } else {
            typedAnswer(kind, value)?.let(changed)
          }
        },
        label = { Text(hint) },
        singleLine = true,
        isError = !valid,
        supportingText = { if (!valid) Text("Enter a valid answer, or clear the field.") },
        modifier = Modifier.fillMaxWidth()
      )
    }
  }
}

/** The answer `text` gives to a question of `kind`, or null if it isn't a valid one. */
private fun typedAnswer(kind: QuestionKind, text: String): Answer? {
  val number = text.toDoubleOrNull()?.takeIf { it.isFinite() }
  return when (kind) {
    is QuestionKind.Number -> number?.takeIf { value ->
      kind.min.let { it == null || value >= it } && kind.max.let { it == null || value <= it }
    }?.let { Answer.Number(it) }

    QuestionKind.Duration -> number?.takeIf { it >= 0 }?.let { Answer.Number(it) }

    QuestionKind.TimeOfDay -> text.takeIf { parseTimeOfDay(it) != null }?.let { Answer.Text(it) }

    else -> Answer.Text(text)
  }
}

@Composable
internal fun EntryDetails(model: NotebookModel, note: MobileNote, open: (String) -> Unit) {
  var journal by remember(note.id) { mutableStateOf<MobileNote?>(null) }
  var calendar by remember { mutableStateOf(false) }
  val entry = note.entry!!
  val zone = ZoneId.systemDefault()
  val at = Instant.ofEpochMilli(entry.at).atZone(zone).toLocalDateTime()
  val timeFormat = remember { DateTimeFormatter.ofPattern("HH:mm") }
  var time by rememberSaveable(note.id) { mutableStateOf(at.format(timeFormat)) }
  var timeFocused by remember { mutableStateOf(false) }
  var invalidTime by remember { mutableStateOf(false) }
  val keyboard = LocalSoftwareKeyboardController.current
  LaunchedEffect(entry.journal) { journal = model.read { it.note(entry.journal) } }
  LaunchedEffect(entry.at) {
    if (!timeFocused) {
      time = at.format(timeFormat)
      invalidTime = false
    }
  }
  fun setWhen(day: LocalDate? = null, minute: LocalTime? = null) {
    model.perform { core ->
      val current = core.note(note.id)?.entry?.at ?: return@perform
      val local = Instant.ofEpochMilli(current).atZone(zone)
      val next = (day ?: local.toLocalDate()).atTime(minute ?: local.toLocalTime())
        .atZone(zone).toInstant().toEpochMilli()
      if (next != current) core.setEntryAt(note.id, next)
    }
  }
  fun saveTime() {
    val parsed = runCatching {
      LocalTime.parse(
        time.trim(),
        DateTimeFormatter.ofPattern("H:mm").withResolverStyle(ResolverStyle.STRICT)
      )
    }.getOrNull()
    invalidTime = parsed == null
    if (parsed != null) {
      time = parsed.format(timeFormat)
      setWhen(minute = parsed)
    }
  }
  HorizontalDivider()
  Row(verticalAlignment = Alignment.CenterVertically) {
    Text("Journal", Modifier.width(64.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
    TextButton(
      onClick = { open(entry.journal) },
      enabled = journal != null,
      modifier = Modifier.testTag("entry-journal")
    ) {
      journal?.journal?.let { JournalDot(it.color) }
      Text(formattedTitle(journal?.title ?: "Journal entry"), Modifier.padding(start = 8.dp))
    }
  }
  Row(
    Modifier.fillMaxWidth(),
    horizontalArrangement = Arrangement.spacedBy(8.dp),
    verticalAlignment = Alignment.CenterVertically
  ) {
    Text("When", Modifier.width(56.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
    OutlinedButton(
      onClick = { calendar = true },
      colors = neutralButtonColors(),
      contentPadding = PaddingValues(horizontal = 8.dp),
      shape = RoundedCornerShape(6.dp),
      modifier = Modifier.testTag("entry-day")
    ) {
      Text(at.toLocalDate().format(DateTimeFormatter.ofPattern("MMM d, yyyy")))
      AppIcon(R.drawable.dropdown)
    }
    OutlinedTextField(
      time,
      { time = it },
      placeholder = { Text("HH:MM") },
      singleLine = true,
      isError = invalidTime,
      keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
      keyboardActions = KeyboardActions(onDone = {
        saveTime()
        keyboard?.hide()
      }),
      modifier = Modifier.width(92.dp).testTag("entry-time").onFocusChanged {
        val lostFocus = timeFocused && !it.isFocused
        timeFocused = it.isFocused
        if (lostFocus) saveTime()
      }
    )
  }
  journal?.journal?.questions?.filter { it.retired == null || it.id in entry.answers }
    ?.forEach { question ->
      AnswerField(question, entry.answers[question.id]) { answer ->
        model.perform { it.setAnswer(note.id, question.id, answer) }
      }
    }
  if (calendar) {
    CalendarDialog(at.toLocalDate(), { calendar = false }, "Today") {
      setWhen(day = it ?: LocalDate.now())
      calendar = false
    }
  }
}

@Composable
internal fun JournalDetails(model: NotebookModel, note: MobileNote) {
  val journal = note.journal!!
  val context = LocalContext.current
  var prompts by remember(note.id) { mutableStateOf(model.repository.prompts(note.id)) }
  val permission =
    rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
      PromptScheduler.schedule(context)
    }
  var question by remember { mutableStateOf<Question?>(null) }
  var addingQuestion by remember { mutableStateOf(false) }
  var schedule by remember { mutableStateOf<Schedule?>(null) }
  var addingSchedule by remember { mutableStateOf(false) }
  ToggleRow("Prompt here?", prompts) { enabled ->
    prompts = enabled
    model.repository.setPrompts(note.id, enabled)
    if (enabled &&
      Build.VERSION.SDK_INT >= 33
    ) {
      permission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
    PromptScheduler.schedule(context)
  }
  if (prompts && !NotificationManagerCompat.from(context).areNotificationsEnabled()) {
    TextButton(onClick = {
      context.startActivity(
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
          .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
      )
    }) { Text("Allow Android notifications") }
  }
  Row(
    horizontalArrangement = Arrangement.spacedBy(12.dp),
    verticalAlignment = Alignment.CenterVertically
  ) {
    Text("Color")
    JournalColorPicker(journal.color) { color ->
      model.perform { it.setJournalColor(note.id, color) }
    }
  }
  val questions = journal.questions.filter { it.retired == null }
  JournalHeading("Questions")
  if (questions.isEmpty()) JournalPlaceholder("No questions")
  questions.forEachIndexed { index, item ->
    Row(verticalAlignment = Alignment.CenterVertically) {
      Column(Modifier.weight(1f)) {
        Text(item.prompt.trim(), style = MaterialTheme.typography.titleSmall)
        Text(
          describeQuestion(item),
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          style = MaterialTheme.typography.bodySmall
        )
      }
      IconButton(enabled = index > 0, onClick = {
        model.perform { it.moveQuestion(note.id, item.id, true) }
      }) { AppIcon(R.drawable.up, "Move up") }
      IconButton(enabled = index < questions.lastIndex, onClick = {
        model.perform { it.moveQuestion(note.id, item.id, false) }
      }) { AppIcon(R.drawable.down, "Move down") }
      IconButton(onClick = { question = item }) { AppIcon(R.drawable.edit, "Edit") }
      IconButton(onClick = {
        model.perform {
          it.setQuestion(note.id, item.copy(retired = System.currentTimeMillis()), false)
        }
      }) { AppIcon(R.drawable.trash, "Remove") }
    }
  }
  TextButton(onClick = { addingQuestion = true }) {
    AppIcon(R.drawable.add)
    Text("Add question")
  }
  JournalHeading("Schedules")
  if (journal.schedules.isEmpty()) JournalPlaceholder("Takes entries only when you write one")
  journal.schedules.forEach { item ->
    Row(verticalAlignment = Alignment.CenterVertically) {
      Column(Modifier.weight(1f)) {
        Text(describeDays(item.days), style = MaterialTheme.typography.titleSmall)
        Text(
          describeTimes(item.times),
          color = MaterialTheme.colorScheme.onSurfaceVariant,
          style = MaterialTheme.typography.bodySmall
        )
      }
      IconButton(onClick = { schedule = item }) { AppIcon(R.drawable.edit, "Edit") }
      IconButton(onClick = { model.perform { it.removeSchedule(note.id, item.id) } }) {
        AppIcon(R.drawable.trash, "Remove")
      }
    }
  }
  TextButton(onClick = { addingSchedule = true }) {
    AppIcon(R.drawable.add)
    Text("Add schedule")
  }
  if (addingQuestion || question != null) {
    QuestionDialog(
      question,
      {
        addingQuestion = false
        question = null
      }
    ) { value ->
      val new = addingQuestion
      model.perform { it.setQuestion(note.id, value, new) }
      addingQuestion = false
      question = null
    }
  }
  if (addingSchedule || schedule != null) {
    ScheduleDialog(
      schedule,
      {
        addingSchedule = false
        schedule = null
      }
    ) { value ->
      if (prompts && Build.VERSION.SDK_INT >= 33) {
        permission.launch(Manifest.permission.POST_NOTIFICATIONS)
      }
      model.perform { it.setSchedule(note.id, value) }
      addingSchedule = false
      schedule = null
    }
  }
}

internal fun minuteLabel(minute: Int) = "%02d:%02d".format(minute / 60, minute % 60)

@Composable
private fun QuestionDialog(initial: Question?, dismiss: () -> Unit, save: (Question) -> Unit) {
  var prompt by rememberSaveable { mutableStateOf(initial?.prompt.orEmpty()) }
  var type by remember { mutableStateOf(initial?.kind ?: QuestionKind.Text) }
  var min by rememberSaveable {
    mutableStateOf(
      when (val kind = type) {
        is QuestionKind.Number -> kind.min?.toString().orEmpty()
        is QuestionKind.Scale -> kind.min.toString()
        else -> ""
      }
    )
  }
  var max by rememberSaveable {
    mutableStateOf(
      when (val kind = type) {
        is QuestionKind.Number -> kind.max?.toString().orEmpty()
        is QuestionKind.Scale -> kind.max.toString()
        else -> ""
      }
    )
  }
  var unit by rememberSaveable { mutableStateOf((type as? QuestionKind.Number)?.unit.orEmpty()) }
  var low by rememberSaveable { mutableStateOf((type as? QuestionKind.Scale)?.low.orEmpty()) }
  var high by rememberSaveable { mutableStateOf((type as? QuestionKind.Scale)?.high.orEmpty()) }
  var multiple by remember { mutableStateOf((type as? QuestionKind.Choice)?.multiple ?: false) }
  var options by remember { mutableStateOf((type as? QuestionKind.Choice)?.options.orEmpty()) }
  val validBounds = when (type) {
    is QuestionKind.Scale -> min.toLongOrNull() != null && max.toLongOrNull() != null &&
      min.toLong() <= max.toLong()

    is QuestionKind.Number -> (min.isBlank() || min.toDoubleOrNull()?.isFinite() == true) &&
      (max.isBlank() || max.toDoubleOrNull()?.isFinite() == true) &&
      (min.isBlank() || max.isBlank() || min.toDouble() <= max.toDouble())

    else -> true
  }
  AlertDialog(
    onDismissRequest = dismiss,
    title = { Text(if (initial == null) "Add question" else "Edit question") },
    text = {
      Column(Modifier.verticalScroll(rememberScrollState())) {
        OutlinedTextField(prompt, { prompt = it }, label = { Text("Question") })
        Row(
          horizontalArrangement = Arrangement.spacedBy(12.dp),
          verticalAlignment = Alignment.CenterVertically
        ) {
          Text("Type")
          if (initial == null) {
            MenuControl(
              questionKinds.first { it.second::class == type::class },
              questionKinds,
              { it.first }
            ) { (_, kind) ->
              type = kind
              if (kind is QuestionKind.Scale && min.isBlank()) {
                min = "1"
                max = "5"
              }
            }
          } else {
            Text(questionKindName(type))
          }
        }
        if (initial != null) {
          Text(
            "A question keeps its type. Add a new question for another type.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall
          )
        }
        if (type is QuestionKind.Number) {
          OutlinedTextField(
            unit,
            { unit = it },
            label = { Text("Unit") },
            placeholder = { Text("Unit, such as kg") }
          )
        }
        if (type is QuestionKind.Number || type is QuestionKind.Scale) {
          OutlinedTextField(min, { min = it }, label = { Text("Least") })
          OutlinedTextField(max, { max = it }, label = { Text("Greatest") })
        }
        if (type is QuestionKind.Scale) {
          OutlinedTextField(
            low,
            { low = it },
            label = { Text("Low label") },
            placeholder = { Text("Label for the least") }
          )
          OutlinedTextField(
            high,
            { high = it },
            label = { Text("High label") },
            placeholder = { Text("Label for the greatest") }
          )
        }
        if (type is QuestionKind.Choice) {
          ToggleRow("Pick several?", multiple) { multiple = it }
          options.filter { it.retired == null }.forEach { option ->
            Row(verticalAlignment = Alignment.CenterVertically) {
              OutlinedTextField(
                option.label,
                { label ->
                  options =
                    options.map { if (it.id == option.id) it.copy(label = label) else it }
                },
                Modifier.weight(1f),
                placeholder = { Text("Option") }
              )
              IconButton(onClick = {
                options = options.map {
                  if (it.id == option.id) {
                    it.copy(retired = System.currentTimeMillis())
                  } else {
                    it
                  }
                }
              }) { AppIcon(R.drawable.trash, "Remove option") }
            }
          }
          TextButton(onClick = { options = options + ChoiceOption(newId(), "", null) }) {
            Text("Add option")
          }
        }
      }
    },
    confirmButton = {
      TextButton(
        enabled = prompt.isNotBlank() && validBounds &&
          (
            type !is QuestionKind.Choice ||
              options.any { it.retired == null && it.label.isNotBlank() }
            ),
        onClick = {
          val kind = when (type) {
            is QuestionKind.Choice -> QuestionKind.Choice(multiple, options)

            is QuestionKind.Number -> QuestionKind.Number(
              unit.ifBlank { null },
              min.toDoubleOrNull(),
              max.toDoubleOrNull()
            )

            is QuestionKind.Scale -> QuestionKind.Scale(
              min.toLong(),
              max.toLong(),
              low.ifBlank { null },
              high.ifBlank { null }
            )

            else -> type
          }
          save(Question(initial?.id ?: newId(), prompt, kind, initial?.retired))
        }
      ) { Text(if (initial == null) "Add" else "Save") }
    },
    dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } }
  )
}

@Composable
private fun ScheduleDialog(initial: Schedule?, dismiss: () -> Unit, save: (Schedule) -> Unit) {
  var random by remember { mutableStateOf(initial?.times is Times.Random) }
  val original = initial?.times as? Times.Random
  var times by rememberSaveable {
    mutableStateOf(
      (initial?.times as? Times.Fixed)?.v1
        ?.joinToString(", ") { minuteLabel(it.toInt()) } ?: "10:00, 14:00, 17:00"
    )
  }
  var start by rememberSaveable { mutableStateOf(minuteLabel(original?.start?.toInt() ?: 540)) }
  var end by rememberSaveable { mutableStateOf(minuteLabel(original?.end?.toInt() ?: 1020)) }
  var mean by rememberSaveable { mutableStateOf((original?.mean?.toInt() ?: 60).toString()) }
  var gap by rememberSaveable { mutableStateOf((original?.minGap?.toInt() ?: 20).toString()) }
  var days by remember { mutableStateOf(initial?.days ?: WORKDAYS) }
  val id = remember { initial?.id ?: newId() }
  val schedule = if (random) {
    val from = parseTimeOfDay(start.trim())
    val to = parseTimeOfDay(end.trim())
    val meanGap = mean.trim().toUShortOrNull()
    val minGap = gap.trim().toUShortOrNull()
    if (from != null && to != null && meanGap != null && minGap != null) {
      Schedule(id, days, Times.Random(from, to, meanGap, minGap))
    } else {
      null
    }
  } else {
    val fixed = times.split(',', ' ').filter { it.isNotBlank() }.map { parseTimeOfDay(it.trim()) }
    if (fixed.all { it != null }) {
      Schedule(id, days, Times.Fixed(fixed.filterNotNull().distinct().sorted()))
    } else {
      null
    }
  }
  val valid = schedule != null && scheduleIsValid(schedule)
  AlertDialog(
    onDismissRequest = dismiss,
    title = { Text(if (initial == null) "Add schedule" else "Edit schedule") },
    text = {
      Column(
        Modifier.verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(8.dp)
      ) {
        ChipRow {
          scheduleWeekdays.forEach { (code, name) ->
            FilterChip(
              code in ruleWeekdays(days),
              onClick = { days = toggleWeekday(days, code) },
              label = { Text(name) }
            )
          }
        }
        ToggleRow("Random times?", random) { random = it }
        if (random) {
          OutlinedTextField(start, { start = it }, label = { Text("From") })
          OutlinedTextField(end, { end = it }, label = { Text("Until") })
          OutlinedTextField(mean, { mean = it }, label = { Text("Minutes apart, on average") })
          OutlinedTextField(gap, { gap = it }, label = { Text("Minutes apart, at least") })
        } else {
          OutlinedTextField(
            times,
            { times = it },
            label = { Text("Times") },
            placeholder = { Text("10:00, 14:00, 17:00") }
          )
        }
      }
    },
    confirmButton = {
      TextButton(enabled = valid, onClick = { schedule?.let(save) }) {
        Text(if (initial == null) "Add" else "Save")
      }
    },
    dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } }
  )
}

@Composable
internal fun JournalsScreen(
  model: NotebookModel,
  state: NotebookState,
  query: String,
  open: (String) -> Unit
) {
  val preferences = model.repository.preferences
  var newJournal by rememberSaveable { mutableStateOf(false) }
  var hidden by remember {
    mutableStateOf<Set<String>>(preferences.getStringSet("journals.hidden", emptySet())!!.toSet())
  }
  var menu by remember { mutableStateOf(false) }
  var range by rememberSaveable {
    mutableStateOf(
      JournalRange.entries.find { it.days == preferences.getInt("journals.range", 7) }
        ?: JournalRange.WEEK
    )
  }
  val visible = state.journals.filter { it.id !in hidden }
  val since = range.since()
  val entries = state.notes.filter {
    !it.inTrash && it.entry != null && it.entry.at >= since && it.matches(query) &&
      visible.any { journal -> journal.id == it.entry.journal }
  }
    .sortedByDescending { it.entry!!.at }
  LaunchedEffect(hidden, range) {
    preferences.edit().putStringSet(
      "journals.hidden",
      hidden
    ).putInt("journals.range", range.days).apply()
  }
  Row(
    Modifier.fillMaxWidth().padding(horizontal = 12.dp),
    horizontalArrangement = Arrangement.SpaceBetween,
    verticalAlignment = Alignment.CenterVertically
  ) {
    Box {
      OutlinedButton(
        onClick = { menu = true },
        shape = RoundedCornerShape(6.dp),
        modifier = Modifier.testTag("journals-menu")
      ) {
        Text("Journals")
        AppIcon(R.drawable.dropdown)
      }
      DropdownMenu(menu, onDismissRequest = { menu = false }) {
        DropdownMenuItem(
          text = { Text("New journal…") },
          enabled = state.spaces.any { !it.readOnly },
          onClick = {
            newJournal = true
            menu = false
          }
        )
        if (state.journals.isNotEmpty()) HorizontalDivider()
        state.journals.forEach { journal ->
          DropdownMenuItem(
            text = { Text("Edit ${formattedTitle(journal.title).text}") },
            leadingIcon = { JournalDot(journal.journal!!.color) },
            onClick = {
              menu = false
              open(journal.id)
            }
          )
        }
      }
    }
    MenuControl(
      range,
      JournalRange.entries,
      { it.label },
      Modifier.testTag("journals-range")
    ) { range = it }
  }
  ChipRow {
    Box(Modifier.height(48.dp), contentAlignment = Alignment.Center) { Text("Show") }
    state.journals.forEach { journal ->
      FilterChip(journal.id !in hidden, onClick = {
        hidden = if (journal.id in hidden) hidden - journal.id else hidden + journal.id
      }, label = { Text(formattedTitle(journal.title)) }, leadingIcon = {
        JournalDot(journal.journal!!.color)
      })
    }
  }
  LazyColumn {
    val chartable = visible.any { journal ->
      journal.journal!!.questions.any { it.retired == null && charts(it.kind) }
    }
    if (visible.size > 1 && chartable && entries.isNotEmpty()) {
      item {
        Text(
          "Show one journal alone to chart its answers",
          Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("journal-chart-hint"),
          style = MaterialTheme.typography.bodySmall,
          color = MaterialTheme.colorScheme.onSurfaceVariant
        )
      }
    }
    if (visible.size == 1) {
      val journal = visible.single()
      val questions = journal.journal!!.questions.filter { it.retired == null }
      items(questions, key = { "chart.${it.id}" }) { question ->
        JournalChart(question, entries, journal.journal.color, since)
      }
    }
    if (entries.isEmpty()) item { JournalsEmpty(state.journals.isEmpty(), query) }
    val groups = entries.groupBy {
      Instant.ofEpochMilli(it.entry!!.at).atZone(ZoneId.systemDefault()).toLocalDate()
    }
    groups.forEach { (day, notes) ->
      item(key = day.toString()) {
        Text(
          formatDate(day),
          Modifier.padding(16.dp),
          style = MaterialTheme.typography.titleMedium
        )
      }
      items(notes, key = { it.id }) { note ->
        val journal = visible.first { it.id == note.entry!!.journal }
        val entry = note.entry!!
        Column(
          Modifier.fillMaxWidth().clickable { open(note.id) }
            .padding(horizontal = 16.dp, vertical = 8.dp).testTag("note-row-${note.id}"),
          verticalArrangement = Arrangement.spacedBy(4.dp)
        ) {
          Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp)
          ) {
            JournalDot(journal.journal!!.color)
            Text(
              "${entryTime(entry.at)} · ${formattedTitle(journal.title).text}",
              style = MaterialTheme.typography.bodySmall,
              color = MaterialTheme.colorScheme.onSurfaceVariant
            )
          }
          val preview = entryPreview(note.body)
          if (preview.isNotEmpty()) {
            Text(
              renderedMarkdown(preview),
              maxLines = 4,
              overflow = TextOverflow.Ellipsis
            )
          }
          val summary = answerSummary(journal.journal!!.questions, entry.answers)
          if (summary.isNotEmpty()) {
            Text(
              summary,
              color = MaterialTheme.colorScheme.onSurfaceVariant,
              style = MaterialTheme.typography.bodySmall
            )
          }
        }
        HorizontalDivider()
      }
    }
  }
  if (newJournal) {
    NewJournalDialog(model, state, { newJournal = false }) { id ->
      newJournal = false
      open(id)
    }
  }
}

@Composable
private fun NewJournalDialog(
  model: NotebookModel,
  state: NotebookState,
  dismiss: () -> Unit,
  created: (String) -> Unit
) {
  val preferences = model.repository.preferences
  val libraries = state.spaces.filter { !it.readOnly }.sortedBy { model.repository.name(it.key) }
  var name by rememberSaveable { mutableStateOf("") }
  var space by rememberSaveable {
    mutableStateOf(
      preferences.getString("capture.space", null)?.takeIf { key ->
        libraries.any { it.key == key }
      }
        ?: libraries.firstOrNull()?.key.orEmpty()
    )
  }
  var creating by remember { mutableStateOf(false) }
  AlertDialog(
    onDismissRequest = { if (!creating) dismiss() },
    title = { Text("New journal") },
    text = {
      Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        OutlinedTextField(
          name,
          { name = it },
          label = { Text("Name") },
          singleLine = true,
          modifier = Modifier.fillMaxWidth().testTag("new-journal-name")
        )
        if (libraries.size > 1) {
          Text("Library", style = MaterialTheme.typography.labelSmall)
          MenuControl(
            space,
            libraries.map { it.key },
            { model.repository.name(it) },
            Modifier.testTag("new-journal-library")
          ) { space = it }
        }
      }
    },
    confirmButton = {
      TextButton(
        enabled = !creating && name.isNotBlank() && libraries.any { it.key == space },
        onClick = {
          creating = true
          var id: String? = null
          model.perform(complete = { success ->
            creating = false
            if (success) id?.let(created)
          }) { core ->
            val journal = core.createJournal(space, name.trim())
            id = journal
            model.repository.setPrompts(journal, true)
            preferences.edit().putString("capture.space", space).apply()
          }
        }
      ) { Text(if (creating) "Creating…" else "Create") }
    },
    dismissButton = {
      TextButton(enabled = !creating, onClick = dismiss) { Text("Cancel") }
    }
  )
}

@Composable
private fun JournalHeading(text: String) {
  Text(text, Modifier.padding(top = 6.dp), style = MaterialTheme.typography.titleSmall)
}

@Composable
private fun JournalPlaceholder(text: String) {
  Text(
    text,
    Modifier.padding(12.dp),
    color = MaterialTheme.colorScheme.onSurfaceVariant
  )
}

@Composable
private fun JournalsEmpty(noJournals: Boolean, query: String) {
  when {
    noJournals -> EmptyNotes(R.drawable.journal, "No Journals", "Start one with New journal")

    query.isNotBlank() ->
      EmptyNotes(R.drawable.search, "No Entries Found", "Try other words or a longer range")

    else -> EmptyNotes(R.drawable.journal, "No Entries", "Write one with quick capture")
  }
}
