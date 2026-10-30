// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package systems.thoughtfull.commonplace

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime

/** Lucide's 2px strokes drawn at 20dp, near the weight Obsidian gives them. */
@Composable
internal fun AppIcon(resource: Int, description: String? = null, modifier: Modifier = Modifier) {
  Icon(painterResource(resource), description, modifier.size(20.dp))
}

/** A single-line outlined field, shorter than Material's 56dp minimum. */
@Composable
internal fun CompactTextField(
  value: String,
  change: (String) -> Unit,
  placeholder: String,
  modifier: Modifier = Modifier,
  keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
  keyboardActions: KeyboardActions = KeyboardActions.Default,
  trailingIcon: (@Composable () -> Unit)? = null
) {
  val interactions = remember { MutableInteractionSource() }
  val colors = OutlinedTextFieldDefaults.colors()
  BasicTextField(
    value,
    change,
    modifier.heightIn(min = 44.dp),
    singleLine = true,
    textStyle = MaterialTheme.typography.bodyLarge.copy(
      color = MaterialTheme.colorScheme.onSurface
    ),
    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
    keyboardOptions = keyboardOptions,
    keyboardActions = keyboardActions,
    interactionSource = interactions
  ) { field ->
    OutlinedTextFieldDefaults.DecorationBox(
      value = value,
      innerTextField = field,
      enabled = true,
      singleLine = true,
      visualTransformation = VisualTransformation.None,
      interactionSource = interactions,
      placeholder = { Text(placeholder) },
      trailingIcon = trailingIcon,
      colors = colors,
      contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp)
    )
  }
}

@Composable
internal fun neutralButtonColors() = ButtonDefaults.outlinedButtonColors(
  contentColor = MaterialTheme.colorScheme.onSurface
)

@Composable
internal fun <T> MenuControl(
  value: T,
  options: List<T>,
  label: (T) -> String,
  modifier: Modifier = Modifier,
  compact: Boolean = false,
  selected: (T) -> Unit
) {
  var expanded by remember { mutableStateOf(false) }
  Box(modifier) {
    OutlinedButton(
      onClick = { expanded = true },
      colors = neutralButtonColors(),
      shape = RoundedCornerShape(6.dp),
      contentPadding = PaddingValues(horizontal = if (compact) 8.dp else 16.dp)
    ) {
      Text(label(value))
      AppIcon(R.drawable.dropdown)
    }
    DropdownMenu(expanded, onDismissRequest = { expanded = false }) {
      options.forEach { option ->
        DropdownMenuItem(
          text = { Text(label(option)) },
          onClick = {
            selected(option)
            expanded = false
          },
          trailingIcon = { if (option == value) AppIcon(R.drawable.actions) }
        )
      }
    }
  }
}

@Composable
internal fun CalendarDialog(
  initial: LocalDate?,
  dismiss: () -> Unit,
  clearLabel: String = "Clear date",
  selected: (LocalDate?) -> Unit
) {
  val state = rememberDatePickerState(
    initialDisplayedMonthMillis = (initial ?: LocalDate.now()).toEpochDay() * 86_400_000
  )
  LaunchedEffect(state.selectedDateMillis) {
    state.selectedDateMillis?.let { selected(LocalDate.ofEpochDay(it / 86_400_000)) }
  }
  DatePickerDialog(
    onDismissRequest = dismiss,
    confirmButton = {},
    dismissButton = {
      Row {
        TextButton(onClick = { selected(null) }) { Text(clearLabel) }
        TextButton(onClick = dismiss) { Text("Cancel") }
      }
    }
  ) { DatePicker(state, showModeToggle = false) }
}

@Composable
internal fun DateTimeDialog(
  initial: LocalDateTime,
  dismiss: () -> Unit,
  selected: (LocalDateTime) -> Unit
) {
  var day by remember { mutableStateOf<LocalDate?>(null) }
  val time = rememberTimePickerState(initial.hour, initial.minute, is24Hour = true)
  if (day == null) {
    CalendarDialog(initial.toLocalDate(), dismiss, "Today") {
      day = it ?: LocalDate.now()
    }
  } else {
    AlertDialog(
      onDismissRequest = dismiss,
      title = { Text("When it happened") },
      text = { TimePicker(time) },
      confirmButton = {
        TextButton(onClick = {
          selected(day!!.atTime(LocalTime.of(time.hour, time.minute)))
        }) { Text("Save") }
      },
      dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } }
    )
  }
}

@Composable
internal fun ReadOnlyBanner() {
  Surface(color = MaterialTheme.colorScheme.secondaryContainer) {
    Text(
      "A newer version of Commonplace wrote this library, so it’s read-only",
      Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
      textAlign = TextAlign.Center
    )
  }
}
