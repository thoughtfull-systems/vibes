// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

class CaptureTileService : TileService() {
  override fun onStartListening() {
    super.onStartListening()
    qsTile?.apply {
      label = getString(R.string.quick_capture)
      state = Tile.STATE_INACTIVE
      updateTile()
    }
  }

  override fun onClick() {
    super.onClick()
    if (isLocked) unlockAndRun { capture() } else capture()
  }

  // The tile lint rule ignores the version guard for the pre-34 overload.
  @SuppressLint("StartActivityAndCollapseDeprecated")
  @Suppress("DEPRECATION")
  private fun capture() {
    if (Build.VERSION.SDK_INT >= 34) {
      startActivityAndCollapse(
        PendingIntent.getActivity(
          this,
          2001,
          captureIntent(this),
          PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
      )
    } else {
      startActivityAndCollapse(captureIntent(this))
    }
  }
}
