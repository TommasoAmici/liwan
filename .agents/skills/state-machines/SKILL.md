---
name: state-machines
description: Questions to ask about state whenever writing, changing or reviewing code in Liwan that has phases, modes, queues or "is it busy" flags, such as an SSO login in progress, an event or exit worker, a prune run, a dashboard filter while a report loads, a setup wizard, or any group of booleans, optionals and task handles that must agree. Covers finding an implicit state machine, putting its states in an enum or a small struct with transition methods, refusing illegal moves, testing the transitions without the UI, and guarding an old task that finishes after a new one started. Use when adding a Bool or optional that means "running", "open" or "waiting", when adding a second flag next to one, or when fixing a bug where two things were on at once.
---

# State machines

Behavior that depends on a state, where the state is spread over booleans,
optionals, counters and task handles that are kept in step by hand, is a state
machine nobody wrote down. Only some combinations are legal, but the types allow
all of them, so the bugs show up as "the login silently dropped", "Stop did
nothing", "two prunes ran at once", "the old report overwrote the new range", or
"the worker cleared the flag that belonged to its replacement".

Answer each question below in a sentence while designing, not after the bug.
"Does not apply" is a fine answer when it is true (one independent flag with no
partner); say why.

## Questions to ask

1. **Is there a second flag next to it?** A new `is_x` beside `is_y`, a
   `JoinHandle` field, a counter, an `Option` that is only set while it runs, or
   a `Mutex<HashMap>` of in-flight work means the states are `idle`, `running`
   and more. Count the legal combinations; if they are fewer than the types
   allow (`2^n`), it is a state machine.

2. **What are the real states, and what moves between them?** Write the table
   first: state, event, next state, side effect. Include the events that are
   easy to forget: cancel, a second request while busy, the thing finishing
   after it was canceled, the client disconnecting, the server shutting down,
   and the flow expiring.

3. **What happens to a request that arrives in a busy state?** Pick one on
   purpose: refused (and the control is off or the API returns `409`), queued
   (run after), or replaces the current one. Silently returning `Ok(())` is how
   work gets dropped.

4. **What does each state own?** What only one state has (an authorization
   state string, an expiry, a progress value, the sender side of a job, the
   range being dragged) goes in that case's associated value, so it cannot
   outlive the state. The SSO flow keeps its pending flows in one map keyed by
   the state string, with an expiry, for exactly this reason.

5. **Can an old task finish after a new one started?** Give each run a token or
   generation number, and have the finishing code do nothing when its number is
   no longer the current one. An OAuth `state` is that token; a task handle that
   is aborted on the way out of the state is another. A `defer` that resets a
   shared flag, or a `JoinHandle` that is dropped without cancelling, is the
   usual culprit.

6. **Is "running" stored in two places?** Keep one source of truth and derive
   the other. A copy inside a helper object that can disagree with the model is
   a bug waiting. On the dashboard, `isLoading` from the query is the truth;
   a second local `isRefreshing` that the component sets by hand will lie.

7. **Is a dialog, wizard or redirect part of the sequence?** A React dialog can
   close by a route you did not plan (Escape, click outside, a navigation), and
   a browser can return from an external identity provider to a page that was
   reloaded. Own the next step in the state, and let the dismissal only report
   which route was taken. Do not leave the next step in a `then` that only runs
   on the expected path.

## Shape

- **Rust** types live in `crates/liwan/src/app/core/`, next to the feature
  (`LiwanExternalAuth`, `LiwanOnboarding`, `LiwanEvents`, `LiwanSessions`), and
  hold no tasks. Use an `enum` with associated values when the states are
  exclusive, or a small `struct` with `mutating` transition methods when several
  things are independent. Do not add a state-machine crate.
- Transition methods refuse illegal moves by returning `Err`, `false` or an
  `Option`, and the caller acts on the answer (start the task, return `409`,
  show the message). The type never starts tasks itself.
- The async shell (`crates/liwan/src/web/`, `main.rs`) owns the runtime side: it
  stores the state, spawns or cancels the task, and cancels the previous run
  before starting a replacement. Cancelling means `handle.abort()` or closing
  the channel, not dropping the handle and hoping.
- **TypeScript** types live next to the component or hook that owns them, in
  `packages/web/src/`, as a discriminated union (`{ status: "idle" } | {
  status: "loading", range } | { status: "error", message }`). Use `useReducer`
  when a component needs three or more related pieces of state; never three
  `useState`s that must agree. Views read derived booleans; they never set the
  union directly.
- A control that a refused move would ignore is disabled, so the refusal is
  visible; where it is not, return a message the UI shows instead of failing
  silently.
- Do not convert a flag that is only a guard (a batch-size constant, a
  `Mutex` that only serializes), or panels meant to be open together. Record
  why in the review or the commit body instead.

## Tests

Test the transitions before wiring the runtime or UI, with names that say the
behavior (`a_second_prune_while_one_runs_is_refused`,
`a_login_for_an_expired_state_is_rejected`) and a message saying why. Cover:
every legal move, each illegal move being refused and leaving the state
unchanged, the busy-state rule from question 3, the finish-after-cancel case
from question 5, and "two things at once is impossible" where the type allows it
to be asked.

- Rust: `#[cfg(test)] mod tests` in the same file for a pure type; an HTTP or
  database-level behavior goes in `crates/liwan/tests/`. Tests run with
  `cargo test -p liwan <name>`.
- TypeScript: keep the machine in a plain module (`x-state.ts`), not inside the
  component, and test it with `bun test path/to/x-state.test.ts`. The web
  package has no component test setup; do not invent one for a transition test.

When part of the machine needs the network or a database, test the pure decision
rule and say in the notes what was not exercised.

## Verify

- Rust: `mise run rust:lint` and the full `mise run rust:test`.
- Dashboard: `mise run web:test` (typecheck and tests), then load the page and
  walk each state. `bun run screenshot` in `packages/screenshots` captures the
  dashboard against a running server; dialogs and native pickers may not appear
  in a screenshot, so say what was not seen.
- Record the table in the module's doc comment, or in the roadmap file for the
  feature: state, event, next state, side effect, and the bug that motivated it.
  That is what the next person reads before changing it.
- Run the `changelog` skill when a person would notice the behavior; a dropped
  login or a double prune is a `### Bug Fixes` line.
