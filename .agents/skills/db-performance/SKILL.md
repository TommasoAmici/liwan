---
name: db-performance
description: Load-tests Liwan's DuckDB event store and SQLite app store with the server's real report functions, finds the queries that break at millions of events, and proves a fix with a before/after benchmark. Use when a report query, schema change or index may not scale, when someone reports a slow dashboard, when adding a report or migration, or before claiming a database change made anything faster.
---

# Database performance

A busy Liwan instance holds millions of events; the demo has years of them. A
query that costs nothing at ten thousand rows can cost seconds at ten million,
and DuckDB will happily scan a whole table for a report. This skill finds the
queries that will cross the time a site owner will wait, and proves a fix with
numbers. For layout questions (flat arrays, narrow reads, allocations) read the
`performance` skill; this one is about the database.

Everything here runs through Cargo and the CLI the server already has:

- the report functions in `crates/liwan/src/app/core/reports/`;
- `crates/liwan/benches/reports.rs` (criterion), which calls those functions on a
  seeded database;
- `liwan dev seed` (`cargo run -- dev seed`, debug builds only), which inserts
  ten million events, and `liwan dev reset-db`;
- a `liwan.config.toml` in the repository root, which is gitignored, pointing
  `data_dir` at `./liwan-data`.

Two different databases, two different rules:

- **DuckDB** (`events`, `event_exits`) is the analytical store. It is columnar
  and reads in blocks; B-tree indexes buy little for a report and cost on every
  insert. V2 of the event migrations drops every index V1 created, on purpose.
  A missing index is almost never the diagnosis here.
- **SQLite** (`app`) is the operational store: users, projects, entities,
  sessions, settings, teams, API keys, onboarding. It has indexes and they
  matter; this is where `EXPLAIN QUERY PLAN` and a covering index are the fix.

Both are migrated by `refinery` from `crates/liwan/src/migrations/{app,events}/`.
Never edit a migration that shipped.

## What to measure

- **Real functions.** Benchmarks call `reports::overall_report`,
  `overall_stats`, `dimension_report`, `custom_events_report`,
  `earliest_timestamp` and `online_users` — never a copy of their SQL. A change
  to a query then changes what is measured.
- **What the person is doing**, and a budget:

  | Workload | Budget | Why |
  |---|---|---|
  | `earliest_timestamp`, `online_users` | 20 ms | Runs on every dashboard load |
  | `overall_report` (graph and metric) | 100 ms | The graph redraws on every range change |
  | `overall_stats`, `dimension_report` | 100 ms | Same page, several of them at once |
  | `custom_events_report` | 100 ms | The custom events card |
  | CSV or export of a long range | 5 s | A background action, still waited on |
  | Pruning old data (`liwan prune`) | 30 s | A cron job |

  The budget is a judgment; change one in `benches/reports.rs` when the
  product's answer changes, and say so in the commit.
- **Scale in events.** The seed command inserts 10,000,000 events spread over a
  year with realistic entities, visitors, countries, browsers and properties.
  That is the target: a fix must hold at ten million, not at the seed's default.
- **Query plans.** Run `EXPLAIN ANALYZE` on the exact SQL a report builds (copy
  it out of the Rust string, or print it while iterating). A `SEQ_SCAN` of
  `events` for one project's week is normal; a scan for a single row, or a hash
  join that builds the whole table, is not. `PRAGMA`-style statistics do not
  exist in DuckDB the way they do in SQLite, so it plans from the statement, not
  from table stats: two queries that look the same can plan differently.

## The loop

1. **Get a quiet machine.** On power, other apps closed, not hot. Do not build
   or run tests while a benchmark runs.
2. **Baseline, before touching code.** Create `liwan.config.toml` with
   `data_dir = "./liwan-data"`, then `cargo run -- dev seed` once (it inserts
   ten million events; expect minutes and a multi-gigabyte directory). Then:

   ```sh
   cargo bench -p liwan --bench reports -- --save-baseline before
   ```

   Criterion prints a time and an interval per benchmark. Read the ones that
   call the code you are about to change, and the neighbors that share its SQL.
3. **Diagnose from the plan, not from a guess.** Name the cause in a sentence:
   the report reads every column of every event instead of four; the filter is
   applied after the aggregation; a `LIKE` pattern defeats the prefix; the
   query scans the year before bounding by entity; a per-row subquery runs for
   each result. Then prove it with `EXPLAIN ANALYZE` and its row counts.
4. **Fix the cause.** Apply the `performance` skill: narrow the read, push the
   filter into the scan, aggregate once, precompute where the data changes.
   - **A new column, table or pre-aggregation** is a new event migration
     `V<N>__name.sql` plus the code that fills it, and a sentence on what the
     migration costs on ten million rows (DuckDB rewrites the table on some
     `ALTER`s).
   - **An index on SQLite** goes in a new app migration, with the query it
     serves named in the commit body.
   - **Never edit a shipped migration**, and never renumber one another plan
     already claims.
5. **Pin the answer first.** Add or extend a test in the reports module
   (`#[cfg(test)] mod tests`) or `crates/liwan/tests/dashboard.rs` so a faster
   but wrong query fails. Benchmarks assert their own result where they can
   (`precondition!` or a debug assert on a count).
6. **Measure again the same way.**

   ```sh
   cargo bench -p liwan --bench reports -- --save-baseline after
   ```

   Compare against the saved baseline — `cargo bench ... -- --baseline before`
   for the raw comparison, or `critcmp before after` if it is installed.
   Criterion reports the change and its interval; a move whose interval crosses
   zero is noise, not a win. Report as "at ten million events, `dimension_report`
   on URL went from 240 ms to 31 ms (criterion: -87%, interval -89% to -84%)",
   never "faster".
7. **Land it.** Commit body: the before and after numbers for each workload you
   touched, the event count, the machine, and any workload that got slower. A
   `CHANGELOG.md` line under `### Features` only if a site owner will feel it.
   If a number backs a `README.md` claim, update that sentence in the same
   change. One logical change per commit.

## Rules for the claim

- A benchmark on a debug build means nothing. `cargo bench` builds release; do
  not "fix" that.
- One run is an anecdote. Criterion's own warm-up and sample count are the
  minimum; for a claim you will put in a commit, run the baseline and the
  comparison in the same session on the same machine, and never compare against
  a saved baseline from another day or another power state.
- Check the database before you believe the number: the bench expects an
  already-seeded database and uses the first project it finds. If `dev seed` was
  not run, or `liwan-data` was reset, the numbers describe an empty table.
- Scales need not all improve. A fix that makes a one-day range slightly slower
  and a year-long one much faster is a good fix; say both.
- A regression is a bug. When your change makes another workload slower, say so
  and either fix it or explain the trade in the commit body.

## Extending the harness

The repository has criterion benchmarks but no multi-scale load test, no shape
dimensions and no charts. When a query needs that, plan it (the `add-todo`
skill) rather than hand-rolling one in a scratch file:

- add a benchmark group to `crates/liwan/benches/reports.rs` for the report's
  area, calling the real function with a budget comment;
- keep the seed command as the fixture, and add a `--filter` argument to
  criterion so one group can run while iterating;
- if you add a dimension to the seed data (more entities, many properties, long
  paths), extend `crates/liwan/src/utils/seed.rs` and say in the commit how the
  fixture changed, because old numbers stop being comparable;
- keep `benches/reports.rs` under 400 lines; split along the report areas.

## Not covered

Concurrent readers while events are ingested, a cold page cache after a
reboot, disk speed on a network volume, WAL or checkpoint growth during a long
import, and everything above the database: HTTP handling, JSON serialization,
and the React dashboard. Use `strobe` or a browser profile for the dashboard,
and say when a slow report is probably there instead.
