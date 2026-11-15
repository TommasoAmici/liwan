---
name: add-todo
description: Adds or changes a plan in ROADMAP/TODO, after checking it against every other planned and shipped roadmap item, naming shared dependencies in both files, and settling contradictions, edge cases and open questions with the user before writing so the agent that picks the work up never has to stop and ask. Use when the user asks to add a TODO or roadmap item, to write a plan for later, or to change, split or retire one.
---

# Add TODO

`ROADMAP/TODO/` holds planned work as checklists and `ROADMAP/DONE/` holds what
shipped. Each file is picked up later by an agent that reads it, its links and
the `work` skill — not the other plans. So a new file must stand alone, answer
the questions its worker would otherwise have to ask, agree with everything else
already planned or shipped, and say what it shares with other plans.

## 1. Read the roadmap first

1. `ls ROADMAP/TODO ROADMAP/DONE`, oldest first. Read every TODO file's title,
   checklist and `## Implementation context`; read the DONE files that touch the
   same feature, files or decisions.
2. For a large roadmap, read the small files whole and grep the big ones for the
   words the new item uses: the feature, handler, config key, table, script,
   page or package it touches.
3. Note the shape every file follows: `YYYY-MM-DD-topic.md` (the date it was
   added, not a schedule), a plain title, a short paragraph saying why, a
   `- [ ]` checklist where each item is a slice someone can finish and verify,
   then `## Implementation context` with the decisions, landmarks and
   validation.

## 2. Check that it is aligned

Answer these before writing a line:

- **Planned twice?** The same work in two files is a bug: merge it into the
  older plan, or keep one file and link the other. Never leave two checklists
  that both claim the same file, query, migration or page.
- **Undone by a decision?** Check `ROADMAP/DONE/`, the `README.md` section for
  the feature (the spec), `CHANGELOG.md`, `data/config.example.toml` and the
  invariants in `AGENTS.md`. A new item that reverses shipped work, a published
  config key, a released changelog line or an invariant is a contradiction, not
  a plan (step 3).
- **Needs something that is not planned?** Either it is already planned
  somewhere, or the prerequisite becomes an item in this file, or it belongs in
  another plan. Say where it lives; do not leave a dangling "after X" that
  nobody owns.
- **Shares work with another plan?** Two items that change the same query, the
  same migration number, the same handler, config key, CSS token or dashboard
  page are not a problem, but the agent picking up either one must know. Write
  the dependency in **both** files, naming the other file and what each side
  owns:
  `- [ ] Uses the `reports/` query change that [2026-11-02-report-ranges.md](2026-11-02-report-ranges.md) makes; this plan owns the dashboard side.`
  The note belongs in the checklist item that needs it, not only in the context
  at the bottom, because that is what the worker reads first. Add the reverse
  line in the other file too, so either order works.
- **Same file, different owners?** Say which plan owns which part, so two
  workers do not both rewrite the same function.

## 3. Settle contradictions and open questions before writing

Two kinds of question have to be finished here, because the agent that picks the
work up starts from its own file and should never have to stop and ask:
contradictions today, and every decision it would otherwise have to make
tomorrow.

**Contradictions.** Never commit a plan that contradicts another plan, a DONE
file, the README, an example config key or an `AGENTS.md` invariant and leave
the conflict for the next agent to trip over. Ask the user with the question
tool, in one batch:

- what contradicts what, quoting both sides and where each lives;
- the two or three readings you can see, with what each would mean for the work;
- which one you recommend, and why.

Wait for the answer, then write the file so the decision is recorded (a dated
line in `## Implementation context`, as the other plans do). If the answer
changes another plan, edit that plan in the same commit and say so.

**Everything the worker would otherwise ask.** Walk the work as if you were
doing it now, slice by slice, and write down every decision it cannot take on
its own:

- **What the user sees**: wording, labels, where a control lives, defaults, and
  what happens when something is canceled, fails, returns nothing, or meets a
  project with two years of events and ten million rows.
- **What touches the user's data**: whether a migration runs, whether old data
  is rewritten, whether an old index or export keeps working, and how an
  operator finds out before it happens. A shipped migration is never edited;
  say which new `V<N>` other plans already claim.
- **What an operator must configure**: a new config key needs a default that
  keeps existing installs working, a line in `data/config.example.toml`, and a
  changelog entry; a dashboard setting belongs in the `app` database instead.
- **The API and clients**: a changed request or response changes
  `packages/api` and `crates/api` too, plus their changelogs. Say whether the
  plan changes the wire format.
- **Scope**: what this plan deliberately leaves out, and which other plan owns
  it.
- **How it is proved**: the test, command or check that shows each slice is
  done (`cargo test -p liwan <name>`, `mise run web:test`, a screenshot from
  `packages/screenshots`, a benchmark number).

Answer each one from the repository where the answer already exists, which is
most of them: the `README.md` section is the spec and already decides far more
than it seems, `data/config.example.toml` shows what an operator is told, a
shipped migration and an existing pattern settle the rest. Ask the user only
what the repo genuinely cannot answer, in the same batch as the contradictions,
and say which questions you answered yourself and from where, so the user can
correct you. Then wait before writing.

Record the answers as dated decisions in `## Implementation context`, with the
reason, not only the outcome, so the next agent does not quietly undo them. A
checklist item that says "ask the user" is not ready to commit. If something
truly cannot be decided yet, say so, say why, and say what the worker should do
in the meantime; the worker only interrupts the user for something genuinely
unexpected, so the plan has to be complete enough that "unexpected" means what
it says.

## 4. Write the file

- Say why in the opening paragraph, in plain American English: what is wrong
  today or what cannot be done yet.
- One checklist item per slice: a change that can be built, checked and
  committed on its own. Name the files, types, commands and migrations a worker
  needs; a plan that says "improve the dashboard" is not a slice.
- Say what the worker may decide alone and what it must not: code layout, names
  and test structure are theirs; user-visible behavior, user data, config keys,
  the wire format and anything the plan already decided are not. A worker that
  hits something genuinely unexpected stops and asks; anything else is answered
  here.
- Put decisions, landmarks (paths) and how to validate the work in
  `## Implementation context`. Link related plans by relative path. Keep the
  file readable in one screen for small work; a big feature earns the length.
- Do not tick items and do not move a file to `ROADMAP/DONE/`: those happen with
  the work that ships them.
- A new script, type, config key or command the plan adds is described in
  `AGENTS.md` when it ships, not now.
- Read it back top to bottom as the worker: if you would have to ask anything,
  go back to step 3 before committing.

## 5. Commit

1. `git status --short` and `git diff --cached` so only roadmap files go in.
   Never commit code, scratch files or a plan that is still waiting for an
   answer.
2. One commit for the roadmap change: `docs(roadmap): <what the plan is>`, at
   most 70 characters, with a body of 72-column lines saying why the item is
   needed and what it depends on.
3. Do not push. A push to `main` runs the release workflow, and the user
   decides when.

If the user asked for the TODO as part of a code change, still commit the plan
on its own: one logical change per commit.
