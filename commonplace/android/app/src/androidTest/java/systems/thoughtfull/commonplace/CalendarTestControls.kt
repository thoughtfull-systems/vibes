// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.performClick
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Locale

internal fun tapToday(compose: ComposeTestRule) {
  val day = LocalDate.now().format(DateTimeFormatter.ofPattern("MMMM d", Locale.US))
  compose.onNode(hasText(day, substring = true) and hasClickAction()).performClick()
}
