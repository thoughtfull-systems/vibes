// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

package systems.thoughtfull.commonplace

import systems.thoughtfull.commonplace.core.Answer
import systems.thoughtfull.commonplace.core.Question
import systems.thoughtfull.commonplace.core.QuestionKind

internal fun entryPreview(body: String): String = body.lines().map { it.trim() }
  .filter { it.isNotEmpty() && !it.startsWith('#') }.joinToString("\n")

internal fun answerSummary(questions: List<Question>, answers: Map<String, Answer>): String =
  questions.mapNotNull { question ->
    val answer = answers[question.id] ?: return@mapNotNull null
    val kind = question.kind
    val value = when {
      kind is QuestionKind.Number && answer is Answer.Number ->
        answer.v1.toString().removeSuffix(".0") + (kind.unit?.let { " $it" } ?: "")

      kind == QuestionKind.Duration && answer is Answer.Number -> {
        val minutes = answer.v1.toLong()
        when {
          minutes < 60 -> "$minutes min"
          minutes % 60 == 0L -> "${minutes / 60} h"
          else -> "${minutes / 60} h ${minutes % 60} min"
        }
      }

      kind is QuestionKind.Choice && answer is Answer.Choices -> answer.v1.mapNotNull { id ->
        kind.options.find { it.id == id }?.label
      }.joinToString(", ")

      answer is Answer.Number -> answer.v1.toString().removeSuffix(".0")

      answer is Answer.Bool -> if (answer.v1) "Yes" else "No"

      answer is Answer.Text -> answer.v1

      else -> ""
    }
    "${question.prompt.trim()} $value"
  }.joinToString(" · ")
