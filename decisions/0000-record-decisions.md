# 0000 Record design and architectural decisions

## Context

The apps in this repository share data formats, protocols, and guidelines for languages and
technologies. Choices about these shape every app and every implementation, and they're expensive to
reverse. Without a written record, the reasons behind a choice live only in commit messages or in
memory. A later contributor, human or agent, then can't tell whether a constraint still matters, and
either preserves it out of caution or breaks it by accident.

The repository needs a lightweight way to record each significant decision, the problem behind it,
the options it weighed, and the reasons for the outcome.

This document records that decision. It also describes the process, and its structure serves as the
example and template for later decisions.

## Criteria

- **Central location.** A reader finds every decision in one place, without knowing in advance which
  commit, app, or document holds it.
- **Close to the code.** The record lives in the repository, changes through the same commits and
  reviews as the code, and stays in sync with it.
- **Compares alternatives consistently.** A record names the criteria up front, judges the chosen
  option against them, and explains the rejection of other options in the same terms.
- **Low ceremony.** Writing or revising a record takes minutes. The format has few required parts
  and needs no bookkeeping beyond the text itself.
- **Stable references.** Code, documentation, and other records can point to a decision by a name
  that never changes.

## Decision

Record each significant design or architectural decision as a markdown file in the `decisions`
directory. The format follows the architecture decision records that Michael Nygard
[described][nygard], adapted to put the criteria and the rejected alternatives on the record.

### When to write a decision

Write a decision when a choice affects more than one app or implementation, defines a shared data
format or protocol, sets a guideline for languages or technologies, or would be hard to reverse.
Write one too when a reader of the code would reasonably ask why it works this way and the answer
doesn't fit in a source comment.

### Files and numbering

Name each file with the next unused four-digit number, zero-padded, followed by a short title in
lowercase words joined by hyphens: `0001-sync-protocol.md`. The number identifies the decision, so
never reuse or change it. Start the document with a level-one heading that repeats the number and
gives the title in sentence case: `# 0001 Sync protocol`.

### Sections

Each decision has these sections, as level-two headings, in this order. Every decision needs Context
and Decision. The other three are optional, so leave out any that has nothing to say.

- **Context:** the background a reader needs, and the problem the decision solves.
- **Criteria:** the qualities the decision weighs each alternative against.
- **Decision:** what the decision is and why. If the record lists criteria, evaluate the chosen
  option against each of them.
- **Rejected alternatives:** each option considered and not chosen, and why it falls short. Give as
  much or as little detail as the option needs. If the record lists criteria, a couple of sentences
  that name the criteria the option fails and explain why can suffice. A full evaluation against
  every criterion is fine too, but not required.
- **Consequences:** decisions that this one forces or rules out, follow-on work it requires, and
  costs it accepts.

Use this skeleton for a new decision:

```markdown
# NNNN Title in sentence case

## Context

## Criteria

## Decision

## Rejected alternatives

## Consequences
```

### Process

Propose a decision in the same commit or pull request as the work it governs, or in its own if it
comes first. Review the decision with the change. Merging it into `main` accepts it.

Edit an accepted decision in place when its context and criteria still hold. A revision can be
minor, or it can choose a different alternative. Move the old choice to the rejected alternatives
and explain why it no longer wins.

When the context or the criteria need major changes, write a new decision instead, and explain what
changed. Leave the old decision's text alone, and add a line under its title that links to the new
one, such as `Superseded by [0007](0007-sync-protocol-v2.md).`

### Evaluation

- **Central location:** every decision lives in the `decisions` directory, in numeric order.
- **Close to the code:** the records live in the repository and change through its commits and
  reviews.
- **Compares alternatives consistently:** the Criteria section sets the measure before the Decision
  section names a winner. The Decision section evaluates the winner against every criterion, and
  each rejected alternative explains its shortfall against the criteria it fails.
- **Low ceremony:** only two sections are mandatory, a short decision fits on one screen, and
  merging a decision accepts it without a separate status to track.
- **Stable references:** the number never changes. A decision keeps its number through revisions,
  and when it no longer applies, it links to the decision that supersedes it.

## Rejected alternatives

- **Commit messages and pull request descriptions only.** These live close to the code and add no
  ceremony, but they scatter decisions across the history, so they fail the central location
  criterion. A later commit that revises a choice doesn't link back to the original, so they fail
  the stable references criterion too. They also have no structure for weighing options, so they
  fail the compares alternatives consistently criterion.
- **Records beside the code they govern.** A folder of decisions in each app keeps the records close
  to the code, but a decision that spans apps has no natural home, and a reader has to search every
  app to find them all. This fails the central location criterion.
- **An external wiki or document service.** A wiki offers a central location, but it drifts out of
  sync with the code and skips the review that code changes get. It fails the close to the code
  criterion.
- **Nygard's format as published.** Its records meet the central location, close to the code, and
  stable references criteria. Its Status field fails the low ceremony criterion: a merged decision
  is either active or superseded, and the supersession link already marks the second case. The
  format also has no place for criteria or rejected alternatives, so it fails the compares
  alternatives consistently criterion.
- **Markdown Architectural Decision Records (MADR).** MADR compares alternatives consistently, but
  its template has more sections and metadata than a personal repository needs. It fails the low
  ceremony criterion.

## Consequences

- Each significant decision from now on needs a record in `decisions`, including the guidelines for
  languages and technologies that the README hasn't written yet.
- Choices made before this decision have no record. Write one for an earlier choice when it comes up
  for review or someone questions it.
- AGENTS.md points contributors to this decision, so agents follow the same process.

[nygard]: https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions
