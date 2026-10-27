// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.Manifest
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import kotlinx.coroutines.launch

@Composable
internal fun LibrariesDialog(model: NotebookModel, state: NotebookState, dismiss: () -> Unit) {
  val scope = rememberCoroutineScope()
  val context = LocalContext.current
  val preferences = model.repository.preferences
  var creating by remember { mutableStateOf(false) }
  var removing by remember { mutableStateOf<String?>(null) }
  var merging by remember { mutableStateOf<String?>(null) }
  var mergeTarget by remember { mutableStateOf<String?>(null) }
  var notification by remember {
    mutableStateOf(preferences.getBoolean("capture.notification", false))
  }
  val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
    publishCaptureNotification(context)
  }
  AlertDialog(onDismissRequest = dismiss, title = {
    Row(verticalAlignment = Alignment.CenterVertically) {
      Text("Libraries", Modifier.weight(1f))
      IconButton(onClick = { creating = true }) { AppIcon(R.drawable.add, "New library") }
    }
  }, text = {
    Column(
      Modifier.verticalScroll(rememberScrollState()),
      verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
      state.spaces.sortedBy { model.repository.name(it.key) }.forEach { space ->
        val currentName = model.repository.name(space.key)
        var name by remember(space.key, currentName) { mutableStateOf(currentName) }
        var menu by remember { mutableStateOf(false) }
        Row(verticalAlignment = Alignment.CenterVertically) {
          OutlinedTextField(
            name,
            { name = it },
            singleLine = true,
            label = { Text(if (space.readOnly) "Name · Read only" else "Name on this device") },
            trailingIcon = {
              IconButton(
                modifier = Modifier.testTag("library-apply-${space.key}"),
                enabled = name.isNotBlank() && name.trim() != currentName,
                onClick = {
                  preferences.edit().putString("name.${space.key}", name.trim()).apply()
                  model.refresh()
                }
              ) { AppIcon(R.drawable.actions, "Apply name") }
            },
            modifier = Modifier.weight(1f).testTag("library-name-${space.key}")
          )
          if (state.spaces.size > 1 && !space.readOnly) {
            Box {
              IconButton(
                onClick = { menu = true },
                modifier = Modifier.testTag("library-menu-${space.key}")
              ) {
                AppIcon(R.drawable.more, "Library menu")
              }
              DropdownMenu(menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(text = { Text("Merge into…") }, onClick = {
                  menu = false
                  merging = space.key
                })
                DropdownMenuItem(text = { Text("Delete library") }, onClick = {
                  menu = false
                  removing = space.key
                })
              }
            }
          }
        }
      }
      HorizontalDivider()
      TextButton(onClick = { addCaptureTile(context) { model.report(Exception(it)) } }) {
        Text("Add quick capture to Quick Settings")
      }
      TextButton(onClick = {
        if (!pinQuickCapture(context)) {
          model.report(Exception("This launcher can't add shortcuts to the home screen"))
        }
      }) { Text("Add quick capture to home screen") }
      ToggleRow("Quick capture notification", notification) { enabled ->
        notification = enabled
        preferences.edit().putBoolean("capture.notification", enabled).apply()
        if (enabled && Build.VERSION.SDK_INT >= 33) {
          permission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        publishCaptureNotification(context)
      }
      if (notification && !NotificationManagerCompat.from(context).areNotificationsEnabled()) {
        TextButton(onClick = {
          context.startActivity(
            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
              .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
          )
        }) { Text("Allow Android notifications") }
      }
      HorizontalDivider()
      Text("Test library")
      Text("Add the same Personal and Work samples as Linux. Your existing notes stay here.")
      TextButton(
        enabled = !state.busy && !preferences.getBoolean("samples.loaded", false),
        onClick = { model.loadTestLibrary() }
      ) {
        Text(
          if (preferences.getBoolean("samples.loaded", false)) {
            "Test library loaded"
          } else {
            "Load test library"
          }
        )
      }
    }
  }, confirmButton = { TextButton(onClick = dismiss) { Text("Done") } })
  if (creating) {
    TextInputDialog(
      "Library name",
      "",
      "Name on this device",
      { creating = false },
      valid = { it.isNotBlank() }
    ) { name ->
      scope.launch {
        val key = model.read { it.createSpace() }
        if (key != null) preferences.edit().putString("name.$key", name.trim()).apply()
        creating = false
        model.refresh()
      }
    }
  }
  removing?.let { key ->
    ConfirmDialog(
      "Delete ${model.repository.name(key)}?",
      "Commonplace deletes the library and every note in it permanently from this device. " +
        "Other devices keep their copies.",
      { removing = null },
      confirmLabel = "Delete"
    ) {
      model.perform { it.removeSpace(key) }
      removing = null
    }
  }
  merging?.let { source ->
    if (mergeTarget == null) {
      AlertDialog(
        onDismissRequest = { merging = null },
        title = { Text("Merge into…") },
        text = {
          Column {
            state.spaces.filter { it.key != source && !it.readOnly }
              .sortedBy { model.repository.name(it.key) }.forEach { target ->
                TextButton(onClick = { mergeTarget = target.key }) {
                  Text(model.repository.name(target.key))
                }
              }
          }
        },
        confirmButton = { TextButton(onClick = { merging = null }) { Text("Cancel") } }
      )
    }
    mergeTarget?.let { target ->
      val from = model.repository.name(source)
      val to = model.repository.name(target)
      ConfirmDialog(
        "Merge $from into $to?",
        "Commonplace adds every note in $from to $to, then deletes $from from this device. " +
          "Other devices keep their copies.",
        {
          mergeTarget = null
          merging = null
        },
        confirmLabel = "Merge"
      ) {
        model.perform { it.mergeSpaces(source, target) }
        mergeTarget = null
        merging = null
      }
    }
  }
}
