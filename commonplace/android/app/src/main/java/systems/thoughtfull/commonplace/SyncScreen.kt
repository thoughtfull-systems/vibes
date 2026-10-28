// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package systems.thoughtfull.commonplace

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.Build
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeEncoder
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import systems.thoughtfull.commonplace.core.MobileDevice
import systems.thoughtfull.commonplace.core.MobileException
import systems.thoughtfull.commonplace.core.MobileNotebook
import systems.thoughtfull.commonplace.core.MobileOffer
import systems.thoughtfull.commonplace.core.MobileOutgoingPairing
import systems.thoughtfull.commonplace.core.MobilePairingRequest
import systems.thoughtfull.commonplace.core.MobilePairingState
import systems.thoughtfull.commonplace.core.MobileSpace
import systems.thoughtfull.commonplace.core.MobileSyncConfig
import systems.thoughtfull.commonplace.core.MobileSyncStatus
import systems.thoughtfull.commonplace.core.isSyncInvite

private data class SyncView(
  val address: String,
  val config: MobileSyncConfig,
  val name: String,
  val nearby: List<MobileDevice>,
  val outgoing: MobileOutgoingPairing?,
  val requests: List<MobilePairingRequest>,
  val statuses: Map<String, MobileSyncStatus>
)

/** Sets up sync with other devices, as the Linux app's Sync dialog does. */
@Composable
internal fun SyncScreen(
  model: NotebookModel,
  state: NotebookState,
  invite: String?,
  inviteOpened: () -> Unit,
  close: () -> Unit
) {
  val sync = model.repository.sync
  val revision by sync.revision.collectAsState()
  val startError by sync.error.collectAsState()
  val context = LocalContext.current
  val scope = rememberCoroutineScope()
  var view by remember { mutableStateOf<SyncView?>(null) }
  var adding by remember { mutableStateOf(false) }
  var naming by remember { mutableStateOf<MobileOffer?>(null) }
  var opening by remember { mutableStateOf(false) }
  var inviting by remember { mutableStateOf(false) }
  var hiddenCode by remember { mutableStateOf<String?>(null) }
  LaunchedEffect(revision, state.revision) {
    view = try {
      sync.call { core ->
        val config = core.syncConfig()
        SyncView(
          core.syncAddress(),
          config,
          core.syncName(),
          core.nearbyDevices(),
          core.outgoingPairing(),
          core.pairingRequests(),
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
  DisposableEffect(Unit) {
    // Android drops multicast packets for apps that don't hold this lock, so nearby devices
    // would never show.
    val lock = context.applicationContext.getSystemService(WifiManager::class.java)
      ?.createMulticastLock("commonplace-pairing")
      ?.apply {
        setReferenceCounted(false)
        acquire()
      }
    act { it.startNearby() }
    onDispose {
      lock?.release()
      sync.stopNearby()
    }
  }
  LaunchedEffect(invite) {
    if (invite != null) {
      act { it.openSyncInvite(invite) }
      inviteOpened()
    }
  }
  val outgoing = view?.outgoing
  LaunchedEffect(outgoing) {
    when (val pairing = outgoing?.state) {
      is MobilePairingState.Paired -> toast(context, "Paired with ${pairing.name}")
      is MobilePairingState.Failed -> toast(context, "Couldn't pair: ${pairing.reason}")
      else -> return@LaunchedEffect
    }
    act { it.clearOutgoingPairing() }
  }
  val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
    val link = result.contents ?: return@rememberLauncherForActivityResult
    if (isSyncInvite(link)) {
      act { it.openSyncInvite(link) }
    } else {
      toast(context, "That code isn't a Commonplace invite")
    }
  }
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
        Section("This device", "Devices that pair with this one know it by this name")
        NameField(current.name) { name -> act { it.setSyncName(name) } }
        HorizontalDivider()
        Section(
          "Pair a device",
          "Devices on this network show here while their Sync screen is open"
        )
        current.nearby.forEach { device ->
          Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) { Section(device.name, "On this network") }
            Button(onClick = { act { it.pairNearby(device.address) } }) { Text("Pair") }
          }
        }
        if (current.nearby.isEmpty()) {
          Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)
          ) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            Section("Looking for devices on this network…", "Open Sync on the other device too")
          }
        }
        Button(
          onClick = {
            scanner.launch(
              ScanOptions()
                .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                .setPrompt("Scan the invite that the other device shows")
                .setBeepEnabled(false)
                .setOrientationLocked(false)
            )
          },
          Modifier.fillMaxWidth().testTag("sync-scan-invite")
        ) { Text("Scan invite") }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
          OutlinedButton(
            onClick = { inviting = true },
            Modifier.weight(1f).testTag("sync-show-invite"),
            colors = neutralButtonColors()
          ) { Text("Show invite") }
          OutlinedButton(
            onClick = { opening = true },
            Modifier.weight(1f),
            colors = neutralButtonColors()
          ) { Text("Open invite") }
        }
        HorizontalDivider()
        Row(verticalAlignment = Alignment.CenterVertically) {
          Column(Modifier.weight(1f)) {
            Section("Address, for pairing by hand", "Enter it on the other device")
          }
          IconButton(onClick = { adding = true }, Modifier.testTag("sync-add-device")) {
            AppIcon(R.drawable.add, "Enter another device's address")
          }
        }
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
        Section("Devices", "Devices that this one syncs with")
        if (config.devices.isEmpty()) {
          Section("No devices yet", "Pair one above")
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
    view?.requests?.firstOrNull()?.let { request ->
      AlertDialog(
        onDismissRequest = { act { it.answerPairing(request.address, false) } },
        title = { Text("Pair with ${request.name}?") },
        text = {
          Text("Pair only if ${request.name} shows the code ${spaced(request.code)}.")
        },
        confirmButton = {
          TextButton(onClick = { act { it.answerPairing(request.address, true) } }) {
            Text("Pair")
          }
        },
        dismissButton = {
          TextButton(onClick = { act { it.answerPairing(request.address, false) } }) {
            Text("Decline")
          }
        }
      )
    }
    val code = outgoing?.code
    if (outgoing?.state is MobilePairingState.Waiting && code != null && code != hiddenCode) {
      AlertDialog(
        onDismissRequest = { hiddenCode = code },
        title = { Text("Accept on the other device") },
        text = { Text("Check that it shows the code ${spaced(code)}.") },
        confirmButton = { TextButton(onClick = { hiddenCode = code }) { Text("Hide") } }
      )
    }
    if (inviting) InviteDialog(model, names) { inviting = false }
    if (opening) {
      TextInputDialog(
        "Open invite",
        "",
        "Paste the link that the other device's Show invite gives",
        { opening = false },
        valid = { isSyncInvite(it) }
      ) { link ->
        opening = false
        act { it.openSyncInvite(link.trim()) }
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

/** Shows an invite as a QR code and a link, with a switch for each library to share. */
@Composable
private fun InviteDialog(model: NotebookModel, spaces: List<MobileSpace>, close: () -> Unit) {
  val context = LocalContext.current
  var shared by remember { mutableStateOf(emptySet<String>()) }
  var link by remember { mutableStateOf<String?>(null) }
  LaunchedEffect(shared) {
    link = try {
      model.repository.sync.call { core ->
        core.syncInvite(spaces.map { it.key }.filter { it in shared })
      }
    } catch (cancelled: CancellationException) {
      throw cancelled
    } catch (error: Exception) {
      toast(context, (error as? MobileException.Failed)?.reason ?: error.message.orEmpty())
      null
    }
  }
  val code = remember(link) {
    link?.let { BarcodeEncoder().encodeBitmap(it, BarcodeFormat.QR_CODE, 768, 768).asImageBitmap() }
  }
  Dialog(close, DialogProperties(usePlatformDefaultWidth = false)) {
    Scaffold(
      topBar = {
        TopAppBar(
          title = { Text("Invite a device") },
          navigationIcon = { IconButton(onClick = close) { AppIcon(R.drawable.back, "Back") } }
        )
      }
    ) { padding ->
      Column(
        Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())
          .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        horizontalAlignment = Alignment.CenterHorizontally
      ) {
        Text(
          "Scan this code with a phone, or open the link on the other device. It works once, " +
            "for 15 minutes.",
          textAlign = TextAlign.Center
        )
        code?.let {
          // Scanners need dark modules on a light background, whatever the theme.
          Image(
            it,
            null,
            Modifier.size(240.dp).background(Color.White).padding(12.dp)
              .testTag("sync-invite-code")
          )
        }
        link?.let { current ->
          SelectionContainer {
            Text(
              current,
              Modifier.testTag("sync-invite-link"),
              fontFamily = FontFamily.Monospace,
              style = MaterialTheme.typography.bodySmall
            )
          }
          Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = {
              context.getSystemService(ClipboardManager::class.java)
                .setPrimaryClip(ClipData.newPlainText("Commonplace invite", current))
              // Android 13 and later confirm copies on their own.
              if (Build.VERSION.SDK_INT < 33) toast(context, "Invite link copied")
            }, colors = neutralButtonColors()) { Text("Copy link") }
            OutlinedButton(onClick = {
              val send = Intent(Intent.ACTION_SEND).setType("text/plain")
                .putExtra(Intent.EXTRA_TEXT, current)
              context.startActivity(Intent.createChooser(send, "Share link"))
            }, colors = neutralButtonColors()) { Text("Share link") }
          }
        }
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
          Section("Share", "Libraries to offer the device that uses this invite")
          spaces.forEach { space ->
            ToggleRow(model.repository.name(space.key), space.key in shared) { on ->
              shared = if (on) shared + space.key else shared - space.key
            }
          }
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
private fun NameField(saved: String, apply: (String) -> Unit) {
  var name by remember(saved) { mutableStateOf(saved) }
  OutlinedTextField(
    name,
    { name = it },
    Modifier.fillMaxWidth().testTag("sync-name"),
    singleLine = true,
    label = { Text("Name") },
    trailingIcon = {
      IconButton(
        enabled = name.isNotBlank() && name.trim() != saved,
        onClick = { apply(name.trim()) }
      ) { AppIcon(R.drawable.actions, "Apply name") }
    }
  )
}

/** A six-digit code split in two halves, as people read it aloud. */
private fun spaced(code: String) = if (code.length == 6) "${code.take(3)} ${code.drop(3)}" else code

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
