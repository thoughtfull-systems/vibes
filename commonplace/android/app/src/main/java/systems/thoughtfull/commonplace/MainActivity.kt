// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color

class MainActivity : ComponentActivity() {
  private val model: NotebookModel by viewModels()
  private var incoming by mutableStateOf<Intent?>(null)

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    enableEdgeToEdge()
    publishQuickCapture(this)
    incoming = intent.takeIf { savedInstanceState == null }
    setContent {
      CommonplaceTheme { CommonplaceApp(model, incoming) { incoming = null } }
    }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    incoming = intent
  }

  override fun onPause() {
    model.flushSaves()
    super.onPause()
  }

  override fun onResume() {
    super.onResume()
    model.refresh()
  }
}

@androidx.compose.runtime.Composable
internal fun CommonplaceTheme(content: @androidx.compose.runtime.Composable () -> Unit) {
  val scheme = if (isSystemInDarkTheme()) {
    darkColorScheme(
      background = Color(0xFF242424),
      onPrimaryContainer = Color(0xFFDCE9F7),
      onSecondaryContainer = Color(0xFFFAFAFA),
      onSurface = Color(0xFFFAFAFA),
      onSurfaceVariant = Color(0xFFCDCDCD),
      outlineVariant = Color(0xFF4A4A4A),
      primary = Color(0xFF78AEED),
      primaryContainer = Color(0xFF204A87),
      secondaryContainer = Color(0xFF454545),
      surface = Color(0xFF1E1E1E),
      surfaceContainer = Color(0xFF303030),
      surfaceContainerHigh = Color(0xFF363636),
      surfaceContainerHighest = Color(0xFF3B3B3B),
      surfaceContainerLow = Color(0xFF292929),
      surfaceContainerLowest = Color(0xFF1E1E1E),
      surfaceTint = Color.Transparent
    )
  } else {
    lightColorScheme(
      background = Color(0xFFFAFAFA),
      onPrimaryContainer = Color(0xFF0B355F),
      onSecondaryContainer = Color(0xFF222222),
      onSurface = Color(0xFF222222),
      onSurfaceVariant = Color(0xFF5E5C64),
      outlineVariant = Color(0xFFD5D3CF),
      primary = Color(0xFF1C71D8),
      primaryContainer = Color(0xFFDCE9F7),
      secondaryContainer = Color(0xFFE2E1DE),
      surface = Color.White,
      surfaceContainer = Color(0xFFF0F0F0),
      surfaceContainerHigh = Color(0xFFEDEDED),
      surfaceContainerHighest = Color(0xFFE3E3E3),
      surfaceContainerLow = Color(0xFFF6F5F4),
      surfaceContainerLowest = Color.White,
      surfaceTint = Color.Transparent
    )
  }
  MaterialTheme(colorScheme = scheme, content = content)
}
