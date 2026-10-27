// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package systems.thoughtfull.commonplace

import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.IconButton
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.launch
import systems.thoughtfull.commonplace.core.MobileNote
import systems.thoughtfull.commonplace.core.parseNoteUri

// The navigation order follows the Linux app's tabs.
internal val destinations = listOf("Memos", "Actions", "Journals", "All Notes", "Trash")

@Composable
fun CommonplaceApp(model: NotebookModel, incoming: Intent?, consumed: () -> Unit) {
  val state by model.state.collectAsState()
  val preferences = model.repository.preferences
  var navigation by rememberSaveable(stateSaver = navigationSaver) {
    mutableStateOf(
      Navigation(
        Location(
          preferences.getString("tab", "Memos")!!.replace("All notes", "All Notes"),
          preferences.getString("selected", null)
        )
      )
    )
  }
  val tab = navigation.current.tab
  val selected = navigation.current.note
  val screens = rememberSaveableStateHolder()
  val scope = rememberCoroutineScope()
  var query by rememberSaveable { mutableStateOf(preferences.getString("query", "")!!) }
  var capture by rememberSaveable(stateSaver = captureSaver) {
    mutableStateOf<CaptureRequest?>(null)
  }
  var libraries by remember { mutableStateOf(false) }
  var inboxEditing by remember { mutableStateOf(false) }
  val focusManager = LocalFocusManager.current
  val keyboard = LocalSoftwareKeyboardController.current
  val open: (String) -> Unit = {
    focusManager.clearFocus()
    keyboard?.hide()
    navigation = navigation.go(Location(tab, it))
  }

  fun goBack() {
    focusManager.clearFocus()
    keyboard?.hide()
    val from = navigation
    scope.launch {
      model.read { core -> from.back { core.note(it) != null } }?.let {
        if (navigation == from) navigation = it
      }
    }
  }

  LaunchedEffect(tab, selected, query) {
    preferences.edit().putString("query", query).putString("selected", selected)
      .putString("tab", tab).apply()
  }
  LaunchedEffect(incoming, state.loaded) {
    if (state.loaded && incoming != null) {
      when {
        incoming.action == CAPTURE_ACTION -> capture = CaptureRequest()

        incoming.action == Intent.ACTION_SEND -> capture = CaptureRequest(
          body = listOfNotNull(
            incoming.getStringExtra(Intent.EXTRA_SUBJECT),
            incoming.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
          ).distinct().joinToString("\n\n")
        )

        incoming.action == Intent.ACTION_VIEW -> {
          val target = parseNoteUri(incoming.dataString.orEmpty())
          if (target ==
            null
          ) {
            model.report(Exception("This note link isn't valid"))
          } else {
            open(target)
          }
        }

        incoming.hasExtra("journal") -> capture = CaptureRequest(
          kind = "Entry",
          journal = incoming.getStringExtra("journal"),
          prompt = incoming.getStringExtra("prompt"),
          prompted = incoming.getLongExtra("prompted", 0)
        )
      }
      consumed()
    }
  }
  BackHandler(navigation.canGoBack || capture != null) {
    if (capture != null) capture = null else goBack()
  }

  if (selected != null) {
    screens.SaveableStateProvider("note:$selected") {
      NoteScreen(model, selected, ::goBack, open)
    }
  } else {
    Scaffold(
      modifier = Modifier.imePadding(),
      topBar = {
        TopAppBar(title = { Text(tab) }, actions = {
          TextButton(onClick = { libraries = true }) { Text("Libraries") }
        })
      },
      bottomBar = {
        if (!inboxEditing) {
          NavigationBar {
            destinations.forEach { destination ->
              NavigationBarItem(
                icon = {
                  AppIcon(
                    when (destination) {
                      "Actions" -> R.drawable.actions
                      "All Notes" -> R.drawable.all_notes
                      "Journals" -> R.drawable.journal
                      "Trash" -> R.drawable.trash
                      else -> R.drawable.memo
                    }
                  )
                },
                label = { Text(destination, maxLines = 1) },
                onClick = {
                  focusManager.clearFocus()
                  keyboard?.hide()
                  navigation = navigation.go(Location(destination))
                },
                selected = tab == destination
              )
            }
          }
        }
      }
    ) { padding ->
      Column(Modifier.padding(padding).fillMaxSize()) {
        if (state.spaces.any { it.readOnly }) ReadOnlyBanner()
        if (state.spaces.size > 1 && !inboxEditing) {
          ChipRow {
            state.spaces.sortedBy { model.repository.name(it.key) }.forEach { space ->
              FilterChip(selected = space.shown, onClick = {
                preferences.edit().putBoolean(
                  "shown.${space.key}",
                  !space.shown
                ).apply()
                model.refresh()
              }, label = { Text(model.repository.name(space.key)) })
            }
          }
        }
        if (!inboxEditing) {
          OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            placeholder = { Text("Search notes") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp)
              .testTag("search"),
            trailingIcon = {
              if (query.isNotEmpty()) {
                IconButton(onClick = { query = "" }) {
                  AppIcon(R.drawable.clear_search, "Clear search")
                }
              }
            }
          )
        }
        screens.SaveableStateProvider("list:$tab") {
          when (tab) {
            "Actions" -> ActionsScreen(model, state, query, open) { inboxEditing = it }
            "Journals" -> JournalsScreen(model, state, query, open)
            else -> BrowseScreen(model, state, tab, query, open)
          }
        }
        if (!state.loaded && state.error == null) CircularProgressIndicator()
      }
    }
  }
  capture?.let { request ->
    CaptureDialog(model, state, request, { capture = null }) { capture = null }
  }
  if (libraries) LibrariesDialog(model, state) { libraries = false }
  NameUnnamedLibrary(model, state)
  state.error?.let { error ->
    AlertDialog(
      onDismissRequest = { model.clearError() },
      confirmButton = {
        TextButton(onClick = {
          model.clearError()
          model.refresh()
        }) { Text("Retry") }
      },
      dismissButton = { TextButton(onClick = { model.clearError() }) { Text("Close") } },
      text = { Text(error) },
      title = { Text("Commonplace couldn't save or load") }
    )
  }
}

@Composable
internal fun ChipRow(content: @Composable () -> Unit) {
  Row(
    Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
    horizontalArrangement = Arrangement.spacedBy(8.dp)
  ) { content() }
}

internal fun MobileNote.matches(query: String) = query.trim().split(Regex("\\s+"))
  .all { body.contains(it, ignoreCase = true) }

internal fun dateLabel(timestamp: Long): String = Instant.ofEpochMilli(timestamp)
  .atZone(ZoneId.systemDefault()).format(DateTimeFormatter.ofPattern("MMM d, yyyy · HH:mm"))

/** Asks for the name of a library this device hasn't named yet, once there's more than one. */
@Composable
private fun NameUnnamedLibrary(model: NotebookModel, state: NotebookState) {
  var declined by remember { mutableStateOf(emptySet<String>()) }
  val space = state.spaces.takeIf { it.size > 1 }
    ?.firstOrNull { !model.repository.named(it.key) && it.key !in declined } ?: return
  val count = state.notes.count { it.space == space.key }
  TextInputDialog(
    "Name this library",
    "",
    "Name on this device",
    { declined = declined + space.key },
    valid = { it.isNotBlank() },
    message = "This device has a library with ${if (count == 1) "1 note" else "$count notes"} " +
      "and no name yet"
  ) { name ->
    model.repository.preferences.edit().putString("name.${space.key}", name.trim()).apply()
    model.refresh()
  }
}
