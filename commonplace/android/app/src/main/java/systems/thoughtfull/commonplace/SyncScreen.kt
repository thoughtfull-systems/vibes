// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package systems.thoughtfull.commonplace

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.widget.Toast
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import systems.thoughtfull.commonplace.core.MobileException
import systems.thoughtfull.commonplace.core.MobileNotebook
import systems.thoughtfull.commonplace.core.MobileOffer
import systems.thoughtfull.commonplace.core.MobileSyncConfig
import systems.thoughtfull.commonplace.core.MobileSyncStatus

private data class SyncView(
  val address: String,
  val config: MobileSyncConfig,
  val statuses: Map<String, MobileSyncStatus>
)

/** Sets up sync with other devices, as the Linux app's Sync dialog does. */
@Composable
internal fun SyncScreen(model: NotebookModel, state: NotebookState, close: () -> Unit) {
  val sync = model.repository.sync
  val revision by sync.revision.collectAsState()
  val startError by sync.error.collectAsState()
  val context = LocalContext.current
  val scope = rememberCoroutineScope()
  var view by remember { mutableStateOf<SyncView?>(null) }
  var adding by remember { mutableStateOf(false) }
  var naming by remember { mutableStateOf<MobileOffer?>(null) }
  LaunchedEffect(revision, state.revision) {
    view = try {
      sync.call { core ->
        val config = core.syncConfig()
        SyncView(
          core.syncAddress(),
          config,
          config.devices.associate { it.address to core.syncStatus(it.address) }
        )
      }
    } catch (cancelled: CancellationException) {
      throw cancelled
    } catch (error: Exception) {
      null
    }
  }
  fun act(done: () -> Unit = {}, block: (MobileNotebook) -> Unit) {
    scope.launch {
      try {
        sync.call(block)
        done()
      } catch (cancelled: CancellationException) {
        throw cancelled
      } catch (error: Exception) {
        toast(context, (error as? MobileException.Failed)?.reason ?: error.message.orEmpty())
      }
    }
  }
  val names = state.spaces.sortedBy { model.repository.name(it.key) }
  Dialog(close, DialogProperties(usePlatformDefaultWidth = false)) {
    Scaffold(
      modifier = Modifier.imePadding(),
      topBar = {
        TopAppBar(
          title = { Text("Sync") },
          navigationIcon = { IconButton(onClick = close) { AppIcon(R.drawable.back, "Back") } }
        )
      }
    ) { padding ->
      val current = view
      Column(
        Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
          .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
      ) {
        if (current == null) {
          Section(
            "Sync isn't running",
            startError?.let { "Commonplace couldn't start sync: $it" } ?: "Starting sync…"
          )
          return@Column
        }
        Section("This device", "Enter this address on another device to pair the two")
        SelectionContainer {
          Text(
            current.address,
            Modifier.testTag("sync-address"),
            fontFamily = FontFamily.Monospace,
            style = MaterialTheme.typography.bodyMedium
          )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
          OutlinedButton(onClick = {
            val clipboard = context.getSystemService(ClipboardManager::class.java)
            clipboard.setPrimaryClip(ClipData.newPlainText("Commonplace address", current.address))
            // Android 13 and later confirm copies on their own.
            if (Build.VERSION.SDK_INT < 33) toast(context, "Address copied")
          }, colors = neutralButtonColors()) { Text("Copy") }
          OutlinedButton(onClick = {
            val send = Intent(Intent.ACTION_SEND).setType("text/plain")
              .putExtra(Intent.EXTRA_TEXT, current.address)
            context.startActivity(Intent.createChooser(send, "Share address"))
          }, colors = neutralButtonColors()) { Text("Share") }
        }
        HorizontalDivider()
        Column {
          ToggleRow("Sync automatically", current.config.automatic) { automatic ->
            act { core ->
              core.setSyncAutomatic(automatic)
              SyncWork.schedule(context, automatic)
            }
          }
          Text(
            "After each change, and every few minutes",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
          )
        }
        RelayField(current.config.relay.orEmpty()) { relay ->
          scope.launch {
            try {
              sync.call { it.setSyncRelay(relay) }
              sync.restart()
              toast(
                context,
                if (relay.isBlank()) "Sync uses the public relays" else "Sync uses this relay"
              )
            } catch (cancelled: CancellationException) {
              throw cancelled
            } catch (error: Exception) {
              toast(context, (error as? MobileException.Failed)?.reason ?: error.message.orEmpty())
            }
          }
        }
        Button(onClick = { act { it.syncNow() } }, Modifier.testTag("sync-now")) {
          Text("Sync now")
        }
        val config = current.config
        fun nameOf(device: String) =
          config.devices.find { it.address == device }?.name ?: device.take(8)
        if (config.offers.isNotEmpty()) {
          HorizontalDivider()
          Section("Offers", "Libraries that other devices share with this one")
          config.offers.forEach { offer ->
            OfferRow(
              nameOf(offer.device),
              config.shares.find { it.id == offer.share }?.library,
              names.map { it.key to model.repository.name(it.key) },
              accept = { library ->
                if (library == null) {
                  naming = offer
                } else {
                  act { it.acceptOffer(offer.share, offer.device, library) }
                }
              },
              decline = { act { it.declineOffer(offer.share, offer.device) } }
            )
          }
        }
        HorizontalDivider()
        Row(verticalAlignment = Alignment.CenterVertically) {
          Column(Modifier.weight(1f)) {
            Section(
              "Devices",
              "Devices that this one syncs with, once each has the other's address"
            )
          }
          IconButton(onClick = { adding = true }, Modifier.testTag("sync-add-device")) {
            AppIcon(R.drawable.add, "Add device")
          }
        }
        if (config.devices.isEmpty()) {
          Section("No devices yet", "Add another device by its address")
        }
        config.devices.forEach { device ->
          DeviceCard(
            device.name,
            describe(current.statuses[device.address]),
            rename = { name -> act { it.renameDevice(device.address, name) } },
            remove = { act { it.removeDevice(device.address) } }
          ) {
            names.forEach { space ->
              val shared = config.shares.any {
                it.library == space.key && device.address in it.devices
              }
              ToggleRow("Share ${model.repository.name(space.key)}", shared) { share ->
                act { core ->
                  if (share) {
                    core.shareLibrary(space.key, device.address)
                  } else {
                    core.unshareLibrary(space.key, device.address)
                  }
                }
              }
            }
          }
        }
      }
    }
    if (adding) {
      AddDeviceDialog({ adding = false }) { address, name ->
        act({ adding = false }) { it.addDevice(address, name) }
      }
    }
    naming?.let { offer ->
      TextInputDialog(
        "New library",
        "",
        "Name it for this device",
        { naming = null },
        valid = { it.isNotBlank() }
      ) { name ->
        naming = null
        scope.launch {
          val key = model.read { it.createSpace() } ?: return@launch
          model.repository.preferences.edit().putString("name.$key", name.trim()).apply()
          model.refresh()
          act { it.acceptOffer(offer.share, offer.device, key) }
        }
      }
    }
  }
}

@Composable
private fun Section(title: String, description: String) {
  Column {
    Text(title, style = MaterialTheme.typography.titleSmall)
    Text(
      description,
      style = MaterialTheme.typography.bodySmall,
      color = MaterialTheme.colorScheme.onSurfaceVariant
    )
  }
}

@Composable
private fun RelayField(saved: String, apply: (String) -> Unit) {
  var relay by remember(saved) { mutableStateOf(saved) }
  OutlinedTextField(
    relay,
    { relay = it },
    Modifier.fillMaxWidth().testTag("sync-relay"),
    singleLine = true,
    label = { Text("Relay server (empty for public relays)") },
    trailingIcon = {
      IconButton(enabled = relay.trim() != saved, onClick = { apply(relay.trim()) }) {
        AppIcon(R.drawable.actions, "Apply relay")
      }
    }
  )
}

@Composable
private fun OfferRow(
  device: String,
  syncedLibrary: String?,
  libraries: List<Pair<String, String>>,
  accept: (String?) -> Unit,
  decline: () -> Unit
) {
  var menu by remember { mutableStateOf(false) }
  Row(verticalAlignment = Alignment.CenterVertically) {
    Column(Modifier.weight(1f)) { Section(device, "Offers a library") }
    Box {
      Button(onClick = { menu = true }) { Text("Accept") }
      DropdownMenu(menu, onDismissRequest = { menu = false }) {
        val options = if (syncedLibrary != null) {
          val name = libraries.find { it.first == syncedLibrary }?.second
          listOf("Sync with $name" to syncedLibrary)
        } else {
          listOf("New library…" to null) +
            libraries.map { (key, name) -> "Merge into $name" to key }
        }
        options.forEach { (label, library) ->
          DropdownMenuItem(
            text = { Text(label) },
            onClick = {
              menu = false
              accept(library)
            }
          )
        }
      }
    }
    TextButton(onClick = decline) { Text("Decline") }
  }
}

@Composable
private fun DeviceCard(
  name: String,
  status: String,
  rename: (String) -> Unit,
  remove: () -> Unit,
  shares: @Composable () -> Unit
) {
  var expanded by remember { mutableStateOf(false) }
  Surface(
    color = MaterialTheme.colorScheme.surfaceContainerLow,
    shape = MaterialTheme.shapes.medium
  ) {
    Column(Modifier.fillMaxWidth()) {
      Row(
        Modifier.fillMaxWidth().clickable { expanded = !expanded }.padding(12.dp),
        verticalAlignment = Alignment.CenterVertically
      ) {
        Column(Modifier.weight(1f)) { Section(name, status) }
        AppIcon(if (expanded) R.drawable.up else R.drawable.down)
      }
      if (expanded) {
        Column(
          Modifier.padding(start = 12.dp, end = 12.dp, bottom = 12.dp),
          verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
          var edited by remember(name) { mutableStateOf(name) }
          OutlinedTextField(
            edited,
            { edited = it },
            Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("Name") },
            trailingIcon = {
              IconButton(
                enabled = edited.isNotBlank() && edited.trim() != name,
                onClick = { rename(edited.trim()) }
              ) { AppIcon(R.drawable.actions, "Apply name") }
            }
          )
          shares()
          TextButton(
            onClick = remove,
            colors = ButtonDefaults.textButtonColors(
              contentColor = MaterialTheme.colorScheme.error
            )
          ) { Text("Remove device") }
        }
      }
    }
  }
}

@Composable
private fun AddDeviceDialog(dismiss: () -> Unit, add: (String, String) -> Unit) {
  var address by remember { mutableStateOf("") }
  var name by remember { mutableStateOf("") }
  AlertDialog(
    onDismissRequest = dismiss,
    title = { Text("Add device") },
    text = {
      Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Enter the address that the other device's Sync screen shows, and a name for it")
        OutlinedTextField(
          address,
          { address = it },
          Modifier.testTag("sync-device-address"),
          label = { Text("Address") }
        )
        OutlinedTextField(
          name,
          { name = it },
          Modifier.testTag("sync-device-name"),
          singleLine = true,
          label = { Text("Name") }
        )
      }
    },
    confirmButton = {
      TextButton(
        enabled = address.isNotBlank() && name.isNotBlank(),
        onClick = { add(address.trim(), name.trim()) }
      ) { Text("Add") }
    },
    dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } }
  )
}

internal fun describe(status: MobileSyncStatus?): String = when {
  status == null -> "Not synced yet"

  status.syncing -> "Syncing…"

  status.error != null -> "Couldn't sync: ${status.error}"

  status.synced != null -> "Synced " + Instant.ofEpochMilli(status.synced)
    .atZone(ZoneId.systemDefault()).format(DateTimeFormatter.ofPattern("MMM d, HH:mm"))

  else -> "Not synced yet"
}

private fun toast(context: Context, message: String) {
  Toast.makeText(context, message, Toast.LENGTH_SHORT).show()
}
