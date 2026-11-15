---
name: work
description: Works through ROADMAP/TODO with parallel agents, one worktree per feature, TDD and atomic commits, moving finished features to ROADMAP/DONE.
disable-model-invocation: true
---

# Work

Work on all planned features in `ROADMAP/TODO` using parallel agents. You are the
coordinator: give each feature its own branch, worktree and worker, then integrate
verified results one at a time. Workers practice TDD and create atomic commits.

## Worktree rule

Every agent works in a worktree, without exception. That includes a single
feature, a run with one worker, and a fallback that processes features
sequentially without agent tools. The original checkout is read-only while work
is in progress; the only change an agent may make there is the verified
fast-forward in Integrate after the checks passed. Never edit, build, test or
commit in the original checkout. Before starting a slice, a worker confirms its
working directory is the absolute worktree path it was given. If it is not, the
worker stops and reports instead of working in the wrong place.

Create a worktree with `git worktree add <path> -b <branch> <base-sha>`, beside
the original checkout (`../liwan-<name>`). The first command inside it is
`mise install` (the worktree has its own `mise.toml` entry point but shares the
tool cache), then `bun install --frozen-lockfile` and `mise run web:build`, so
the `packages/web/dist` that `rust-embed` requires exists before any `cargo`
command. A fresh checkout has no `node_modules` and no `dist`, and
`cargo build` fails without them.

## Prepare and dispatch

A **feature** is one file in `ROADMAP/TODO/`; its `- [ ]` items are slices built
sequentially by that feature's worker. Parallelism is between files.

1. Record the original checkout, branch, `HEAD` and working-tree status. Read
   `AGENTS.md` and list all TODO files oldest first (`YYYY-MM-DD-topic.md`). If
   there are none, stop and report. Preserve existing local changes; worktrees
   start from committed history, so flag any feature that needs uncommitted
   work.
2. Read the features and identify dependencies. Queue independent features
   oldest first; start a dependent feature after its prerequisites have been
   integrated. Allocate unique branch names such as `work/<run>/<topic>` and put
   the checkouts beside the original one.
3. Create an integration worktree and branch from the recorded `HEAD` when two or
   more features will be integrated. With a single ready feature, skip it: its
   branch is merged straight into the original branch once verified (see
   Integrate). Create one worktree and branch per TODO file from the same base,
   always, even when the run has one feature or one worker. Record the mapping
   from roadmap file to branch, worktree and worker. Reuse existing work only
   after inspecting its status and ownership.
4. Spawn one worker per ready feature using the available agent tools. Run as
   many independent workers concurrently as capacity allows, reserving the
   coordinator's slot; queue the rest and dispatch when slots become available.
   Before dispatching a queued feature, advance its untouched branch to the
   current integration head so it includes completed prerequisites. If agent
   tools are unavailable, report the limitation and keep the same worktree
   isolation while processing features sequentially.
5. Give each worker its absolute worktree path, assigned roadmap file, base SHA,
   dependency context, and the worker loop and feature context guidance below.
   Workers read their assigned roadmap file, their worktree's `AGENTS.md` and
   relevant `README.md` section. Explicitly direct every command, edit, build and
   commit to that worktree, and tell the worker to verify its working directory
   matches before the first command: agents may share a default working
   directory. Each worker owns only its assigned feature and branch; the
   coordinator alone edits the integration worktree.

Keep dispatching ready features while other workers run or results are
integrated. Each worktree keeps its own `target/` and `node_modules`, so builds
do not collide; DuckDB is expensive to compile, and the first build in a new
worktree takes a long time. Coordinate anything that binds port 9042 or 4321 one
at a time: two `cargo run` processes cannot share the port or the default data
directory, so run with `--config <worktree>/liwan.config.toml` pointing at a
`data_dir` inside that worktree.

## Worker loop

0. **Enter the worktree** before anything else: `pwd` must be the absolute worktree
   path from the coordinator. If it is not, stop and report; never run a command,
   edit, build, test or commit in the original checkout.
1. **Clarify** before writing code. Read `AGENTS.md`, the feature's `README.md`
   section and the code it touches (`crates/liwan/src/app/core/` for services,
   `crates/liwan/src/web/routes/` for HTTP, `packages/web/src/` for the
   dashboard). Send ambiguities about visible behavior (wording, where a control
   lives, what happens to existing data) to the coordinator, who batches them
   into a question using the available user-input tool. Continue independent
   work while awaiting an answer; settle implementation details from `AGENTS.md`
   and existing patterns.
2. **Build each item as a slice**, respecting dependencies and taking the
   smallest ready item first, each ending in its own commit:
    1. **Red**: write a failing test that pins the behavior. Unit test in the
       module (`#[cfg(test)] mod tests`), HTTP/database test in
       `crates/liwan/tests/` using `common::app()` and `common::TestClient`, or a
       `*.test.ts` next to dashboard logic. Run `cargo test -p liwan <name>` (or
       `bun test <path>`) and confirm it fails for the expected reason.
    2. **Green**: write the least code that passes it, then wire the route or the
       UI. A migration is a new `V<N>__name.sql`; never edit a shipped one.
    3. **Refactor** with the tests green. If the slice added a flag, optional or
       task handle that must agree with another, read the `state-machines` skill
       and model it as one type instead.
    4. **Verify**: `mise run rust:lint` (clippy with warnings as errors, and
       rustfmt), `mise run web:test`, then the full `mise run rust:test`. For a
       UI change, run the server and the dashboard dev server and look at the
       page at 320 px and 1440 px in both themes. `packages/screenshots` captures
       the public demo for the README, not a local page; do not use it as a
       check.
    5. **Document** useful findings and decisions in the assigned roadmap file
       using Feature context below, while the reasoning is still fresh. Update
       `AGENTS.md` when the slice added, moved, renamed or removed a file,
       script, config key or command that it lists.
    6. **Commit** the slice: code, tests, the `README.md` update if behavior
       changed, the `AGENTS.md` update if the layout changed, and the roadmap
       file with its context updated and the item ticked `- [x]`.
3. **Close** the feature once every item is ticked: `git mv ROADMAP/TODO/<file>
   ROADMAP/DONE/<file>` (create `ROADMAP/DONE/` if missing) and commit the move
   with a subject such as `docs(roadmap): <topic> shipped`.
4. **Hand off** the branch and commit SHAs, checks run and their results,
   screenshots or profile numbers, decisions and blockers, and the roadmap file
   containing durable context. Leave the worktree clean and stop editing it. Do
   not merge, push, remove worktrees or pick another feature yourself.

UI-only items with no testable logic still go through Verify, with the rendered
page and the browser console as the check.

## Feature context

Each worker maintains a `## Implementation context` section in its assigned
feature's roadmap Markdown file, alongside the original brief and checklist.
Read existing notes before choosing an approach, and update them with each slice
and before a blocked handoff. This context travels with the file into
`ROADMAP/DONE/`, so a future maintainer can understand and revisit the work.

Record information that would otherwise require rediscovering the reasoning.
Use short paragraphs or bullets, with subheadings only when they help:

- Decisions and their reasons: constraints, tradeoffs, user clarifications, and
  alternatives actually considered or tried, including why they were set aside.
- Implementation landmarks: repository-relative paths and key symbols, how the
  relevant pieces interact, and invariants a refactor must preserve.
- Surprising findings: DuckDB and SQLite limitations, migration costs, failed
  approaches, and workarounds, with the evidence or conditions that explain
  them.
- Validation that matters later: relevant test names, reproduction commands,
  benchmark numbers with the event count and machine, and gaps such as a browser
  or an SSO provider that was not available.
- Remaining limitations, deferred work, and open questions, including what would
  justify revisiting a decision or unblock the next slice.

Keep notes specific to the feature and proportional to what was learned; include
only applicable points. Distinguish verified findings from assumptions and
unresolved questions. Update superseded notes to reflect the current approach,
retaining earlier decisions when their rationale still explains the design. Use
repository-relative links that remain valid after the move to DONE. Keep
user-facing behavior in `README.md`; link to it instead of duplicating it.
Capture durable knowledge rather than a transcript of commands or progress.

## Integrate

1. Review each handoff against its roadmap checklist and inspect the diff for
   unrelated changes. Integrate ready branches sequentially in the integration
   worktree with `git merge --no-ff --no-commit <feature-branch>`. With exactly
   one feature branch there is nothing to combine: skip the integration worktree
   and use the single-feature shortcut below.
2. Resolve conflicts with the feature worker's input. Check `README.md`,
   `AGENTS.md` and roadmap changes and shared code even when Git reports no
   conflict. For concurrent migrations, keep the integration branch's numbering
   and renumber incoming migrations so each database's `V<N>` stays contiguous;
   renaming a migration file is safe only before it ships.
3. Before committing a merge, run `mise run web:build`, `mise run rust:lint` and
   the full `mise run rust:test` on the combined result, then the UI check again
   for anything that changed during integration. This pass is for work that was
   combined or changed; a single feature branch already passed at the exact
   commits being merged, so do not run them again.
4. After all available features are integrated, fast-forward the original branch
   to the integration branch only if the original checkout is clean and its
   branch and `HEAD` still match the recorded state. Otherwise leave the tested
   result on the integration branch and report how to bring it in after local
   work is settled.
5. Remove only clean worker worktrees whose commits are reachable from the branch
   now on the original checkout (`git worktree remove <path>`, which keeps an
   unmerged branch). Retain branches and any blocked or unintegrated worktrees,
   and report their locations. Keep the integration worktree when its result has
   not been brought into the original branch.

### Single feature

When the run has exactly one feature branch, its commits are the ones the worker
built and tested. Review the handoff and diff, confirm the original checkout is
clean and still at the recorded `HEAD`, then fast-forward it directly:

```sh
git merge --ff-only work/<run>/<topic>
```

Do not create an integration worktree, a merge commit, or a second build and
test pass: the tree is identical to what the worker verified. If the original
branch moved in the meantime, make an integration worktree from the current
original `HEAD`, merge the feature there, and run the combined checks as usual.
The fast-forward is safe because the feature branch started from the recorded
`HEAD`; `--ff-only` refuses instead of inventing a merge if that is no longer
true.

## Commits

Follow `AGENTS.md`: Conventional Commits, `type(scope): subject` of at most 70
characters, and a 72-column body saying what changed and why for `feat`, `fix`,
`perf` and `refactor`, with measurements where the change is about speed. Every
commit builds and passes the full test suite. Leave pushing to the user: pushes
to `main` and tags start the release and container workflows.

## When blocked

If a slice can't go green (a missing service for SSO, a browser that is not
installed, a DuckDB feature the bundled version does not have, a conflict with an
invariant in `AGENTS.md`), the worker stops on that feature and reports to the
coordinator. Leave it in `ROADMAP/TODO/` with finished items ticked and preserve
its branch and worktree. Record the blocker, evidence, attempted approaches, and
what is needed to resume in the feature context before handing it off. The
coordinator explains the blocker and asks how to continue while other independent
workers carry on. Integrate verified partial slices only when their dependencies
and the combined checks pass; the feature stays in TODO until all its items are
done.

## Report

Report each integrated feature moved to `ROADMAP/DONE/` with its commits and
checks, anything left in `ROADMAP/TODO/` and why, and decisions the user may want
to revisit. Distinguish integrated work from completed branches awaiting
integration. Include the integration branch when one was used, whether the
original branch was advanced, and retained branches and worktree paths so the run
can be resumed.
