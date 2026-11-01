// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import android.graphics.Bitmap
import android.os.SystemClock
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.test.espresso.Espresso
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import systems.thoughtfull.commonplace.core.DayOffEffect
import systems.thoughtfull.commonplace.core.DaysOffUse
import systems.thoughtfull.commonplace.core.Schedule
import systems.thoughtfull.commonplace.core.Times
import systems.thoughtfull.commonplace.core.parseRule

class JournalInterfaceTest {
  @get:Rule
  val compose = createComposeRule()
  private val instrumentation = InstrumentationRegistry.getInstrumentation()
  private val context = instrumentation.targetContext
  private val application = context.applicationContext as CommonplaceApplication
  private val repository = application.repository
  private var space = ""
  private var hidden = emptySet<String>()

  private fun screenshot(name: String) {
    compose.waitForIdle()
    // Window transitions can outlast Compose's idle state.
    SystemClock.sleep(300)
    val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
    try {
      File(context.cacheDir, "ui-journals-$name.png").outputStream().use {
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
      }
    } finally {
      bitmap.recycle()
    }
  }

  private fun dot(entry: String, color: String) = compose.onNode(
    hasTestTag("journal-dot-$color") and hasAnyAncestor(hasTestTag("note-row-$entry")),
    useUnmergedTree = true
  )

  @After
  fun removeFixtures() {
    if (space.isNotBlank()) runBlocking { repository.use { it.removeSpace(space) } }
    repository.preferences.edit().putStringSet("journals.hidden", hidden).apply()
  }

  @Test
  fun journalMenusAndColoredEntriesFollowLinuxAcrossTheTimelineAndAllNotes() {
    check(context.packageName.startsWith("systems.thoughtfull.CommonplaceAgent."))
    hidden = repository.preferences.getStringSet("journals.hidden", emptySet())!!.toSet()
    val today = LocalDate.now()
    val zone = ZoneId.systemDefault()
    val at = today.atTime(9, 7).atZone(zone).toInstant().toEpochMilli()
    val yesterday = today.minusDays(1).atTime(9, 7).atZone(zone).toInstant().toEpochMilli()
    val (journal, entry, prior) = runBlocking {
      repository.use { core ->
        space = core.createSpace()
        val journal = core.createJournal(space, "Journal interface fixture")
        core.setJournalColor(journal, "purple")
        core.setSchedule(
          journal,
          Schedule(
            "fixed",
            checkNotNull(parseRule("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR")),
            LocalDate.now().toString(),
            DaysOffUse(DayOffEffect.IGNORE, DayOffEffect.IGNORE),
            Times.Fixed(listOf(540u))
          )
        )
        core.setSchedule(
          journal,
          Schedule(
            "random",
            checkNotNull(parseRule("FREQ=DAILY")),
            LocalDate.now().toString(),
            DaysOffUse(DayOffEffect.IGNORE, DayOffEffect.IGNORE),
            Times.Random(540u, 1020u, 60u, 15u)
          )
        )
        val entry = core.createEntry(
          journal,
          "Purple **entry** today\nA *rendered* body",
          at,
          null,
          emptyMap(),
          "$today"
        )
        val prior = core.createEntry(
          journal,
          "Purple entry yesterday",
          yesterday,
          null,
          emptyMap(),
          "$today"
        )
        val others = core.journals().filter { it.id != journal }.map { it.id }.toSet()
        repository.preferences.edit().putString("tab", "Journals").remove("selected")
          .putString("query", "").putInt("journals.range", 7)
          .putStringSet("journals.hidden", others).commit()
        Triple(journal, entry, prior)
      }
    }
    val model = NotebookModel(application)
    compose.setContent { CommonplaceTheme { CommonplaceApp(model, null) {} } }
    compose.waitUntil(10_000) { model.state.value.loaded }
    compose.onNodeWithTag("note-row-$entry").assertExists()
    compose.onNodeWithTag("note-row-$prior").assertExists()
    dot(entry, "purple").assertExists()
    compose.onNodeWithText("Purple entry today\nA rendered body").assertExists()
    compose.onAllNodesWithText("09:07 · Journal interface fixture").assertCountEquals(2)
    compose.onNodeWithTag("journals-range").performTouchInput { click(center) }
    compose.onNodeWithText("Today").performClick()
    compose.onNodeWithTag("note-row-$prior").assertDoesNotExist()
    assertEquals(1, repository.preferences.getInt("journals.range", -1))
    screenshot("timeline")
    compose.onNodeWithTag("journals-range").performTouchInput { click(center) }
    compose.onNodeWithText("All time").performClick()
    compose.onNodeWithTag("note-row-$prior").assertExists()
    compose.onNodeWithTag("journals-menu").performClick()
    compose.onNodeWithText("New journal…").assertExists()
    compose.onNodeWithText("Edit Journal interface fixture").performClick()
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithTag("note-editor").assertExists() }.isSuccess
    }
    compose.onNodeWithText("Details").performClick()
    compose.onNodeWithText("Every workday").performScrollTo().assertExists()
    compose.onNodeWithText("At 09:00").performScrollTo().assertExists()
    compose.onNodeWithText("Daily").performScrollTo().assertExists()
    compose.onNodeWithText("About every 60 minutes from 09:00 until 17:00")
      .performScrollTo().assertExists()
    compose.onNodeWithTag("journal-color").performScrollTo().performClick()
    listOf("Blue", "Brown", "Gray", "Green", "Orange", "Purple", "Red", "Yellow").forEach {
      compose.onAllNodesWithText(it).onLast().assertExists()
    }
    screenshot("colors")
    compose.onNodeWithText("Brown").performClick()
    compose.waitUntil(10_000) {
      model.state.value.journals.find { it.id == journal }?.journal?.color == "brown"
    }
    screenshot("details")
    Espresso.pressBack()
    compose.onNodeWithContentDescription("Back").performClick()
    compose.waitUntil(5_000) {
      runCatching { compose.onNodeWithTag("journals-menu").assertExists() }.isSuccess
    }
    dot(entry, "brown").assertExists()
    compose.onNodeWithText("All notes").performClick()
    compose.onNodeWithTag("note-row-$entry").performScrollTo()
    dot(entry, "brown").assertExists()
    screenshot("all-notes")
  }
}
