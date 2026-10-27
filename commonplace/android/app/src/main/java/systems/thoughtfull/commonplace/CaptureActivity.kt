// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue

class CaptureActivity : ComponentActivity() {
  private val model: NotebookModel by viewModels()

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    setContent {
      val state by model.state.collectAsState()
      CommonplaceTheme {
        BackHandler { finishAndRemoveTask() }
        if (state.loaded) {
          CaptureDialog(model, state, CaptureRequest(), { finishAndRemoveTask() }) {
            finishAndRemoveTask()
          }
        }
        state.error?.let { error ->
          AlertDialog(
            onDismissRequest = { model.clearError() },
            title = { Text("Couldn't save capture") },
            text = { Text(error) },
            confirmButton = { TextButton(onClick = { model.clearError() }) { Text("Close") } }
          )
        }
      }
    }
  }
}
