---
name: performance
description: Questions to ask about speed and memory layout whenever writing, changing or reviewing code in Liwan, especially anything that loops over events, report rows, properties or dashboard data. Covers data-oriented design, flat arrays and index handles, structure of arrays, bitsets, narrow DuckDB and SQLite reads, regexes compiled once, batching writes, and redesigning instead of caching. Use when adding or reworking a hot loop, a lookup table, a cache, a list of flags, a regular expression, an aggregation, or anything that reads from the database.
---

# Performance

A busy Liwan instance ingests events continuously and draws dashboards over a
year of them. Speed comes from how the data is laid out and how much of it is
read, so ask these questions while designing the code, not after it is slow.
Answer each one for the code in front of you, in a sentence, before you write or
approve it. "Does not apply here" is a fine answer when it is true (a settings
page, a one-shot CLI command); say why.

Measure before and after for any change made in the name of speed, with a
benchmark that stays in the repository (see Benchmarks). A layout change that
does not show up in a measurement is not worth its complexity; revert it.

## Benchmarks

- **Write it before the change.** Run it on the old code first and keep the
  number; a benchmark written after the rewrite has nothing to compare with. If
  a benchmark for the code already exists, use it and extend it.
- **Where it lives.** Rust: `crates/liwan/benches/`, one file per area
  (`reports.rs` today; add `ingest.rs`, `geoip.rs`, `parse.rs` and register the
  `[[bench]]` in `crates/liwan/Cargo.toml`). Criterion is already a dev
  dependency. Dashboard: `packages/screenshots` for a real page, profiled with
  `strobe`; a `bun test` timing is acceptable only for a pure function
  (`packages/web/src/**/*.test.ts`).
- **Real scale.** Use the seeded database (`cargo run -- dev seed`, ten million
  events) and the real functions, not a hand-built 1,000-row vector. A benchmark
  at a thousand rows hides the allocations and cache misses that matter.
  Generate synthetic data inside the benchmark with a fixed seed when the
  database is not the thing under test; never read the user's data.
- **Repeatable.** Clean up what you write. Keep the benchmark names stable
  across commits so results can be matched up.
- **Measure what changed.** Time the operation (a filter pass, a top-K, a mask
  build), not the setup around it. Criterion discards warm-up for you; add an
  assertion on the result so a faster-but-wrong rewrite fails instead of looking
  like a win.
- **Release build.** `cargo bench` builds release; numbers from debug builds
  mean nothing.
- **Record the result.** Put before and after numbers (and the machine) in the
  commit body, and when the benchmark guards a limit the README states, update
  that sentence in the same change.
- **Database queries** are measured with the `db-performance` skill instead.
- **A regression is a bug.** When a later change makes a benchmarked path slower,
  say so and either fix it or explain the trade in the commit body.

## The questions

1. **Data-oriented design.** How would we rewrite this to better use
   data-oriented design principles? Start from what the loop reads and writes,
   in what order and how often, then shape the data to fit it. Do not start from
   the structs the code happens to model. Event batching, report aggregation and
   the dashboard's data points are loops over many small records: that is where
   this pays.

2. **Flat arrays and index handles.** Can that `HashMap` be a `Vec`? Can the key
   be an index into it? Does the hot path need the whole value, or only a flag
   that could live in the upper bits of the index? A `String` key hashed per row
   is usually avoidable: intern the entity ids and visitor ids once per request
   and carry the index.

3. **No new caches.** Caching is cope for a slow design. Do not add one, and
   delete the ones you find in code you are changing. Redesign so the work is
   fast to begin with: aggregate once in the query, precompute at the place the
   data changes, not behind a lookup that can go stale. The two existing caches
   (`quick_cache` for parsed user agents and for entity lookups) are deliberate:
   leave them, and if you touch them say what was measured and who invalidates
   them.

4. **Structure of arrays.** Should this be a structure of arrays instead of an
   array of structures? If a loop reads one or two fields of many elements, give
   each field its own contiguous `Vec` so it streams through the cache and
   vectorizes. Keep a `Vec<Struct>` when the fields are always used together.

5. **Inline capacity.** Replace heap-allocated lists with `SmallVec` when the
   usual size is small and bounded (properties per event, filters per request,
   a few dimension values). Decide what happens past the capacity: spill to the
   heap, or reject.

6. **Bitsets.** Do not use a `Vec<bool>`. Use a bitset (`fixedbitset`,
   `roaring`, or a `u64` word set): one bit per flag, with and/or/count as
   single instructions. This covers selections, "seen" and "dirty" flags, and
   the set of entities a report is allowed to see.

7. **Narrow reads.** Does this load a whole row just to read a couple of
   columns? In DuckDB that is cheap per column only if you name the columns;
   `SELECT *` reads every property blob and every nullable text column, and the
   reports join it to a filter. Select the columns the caller uses. On SQLite,
   `SELECT *` on `users` or `projects` pulls password hashes and settings nobody
   needs. A count is `COUNT(*)`, not a load. Reading everything is right only
   when the caller really uses everything, as a full reload does.

8. **Regexes compiled once.** Is a regular expression built inside a function,
   a loop, a closure or an `init`? Compile it once at module level: a `static
   LazyLock<Regex>` in Rust, a module-level `const` regex in TypeScript (the
   dashboard already avoids one; do not add one in a render path). The user
   agent parser loads its patterns once into a `LazyLock` — keep it that way. A
   pattern built from data stays dynamic, and so does a one-shot script; say so
   rather than hoisting for its own sake.

9. **Batch the writes.** Ingestion arrives per request and must not write per
   request. The event worker (`crates/liwan/src/app/core/events.rs`) batches
   into one transaction; a new write path joins a batch or says in the commit
   why it cannot. Do not hold a database connection across an `.await`, and do
   not run blocking database work on the async runtime — use
   `spawn_blocking`, as the session and event code does.

10. **The dashboard is a loop too.** A dimension table can hold 1,000 rows and a
    graph 2,000 points, and both redraw on every range change. Sort once in a
    `useMemo`, key rows by id, and keep collators and formatters at module scope
    (`Intl.Collator` in `packages/web/src/hooks/api.ts` is the pattern). A sort
    or filter inside the render body runs on every render, including hover.

## Using them

- Ask these while designing, again when reviewing a diff, and whenever you are
  about to type `HashMap<String, ...>`, `Vec<bool>`, `clone()` in a loop,
  `collect()` over a borrowed collection you only read, `SELECT *`, a `Regex::new`
  inside a function, the word "cache", or `useMemo` with a dependency that
  changes every render.
- A finding is a concrete change ("the report clones every path string to build
  a filter map; intern the entity ids"), not a general remark.
- **A pass inside a pass is a bug until it is shown not to be.** Before adding a
  `map`, `filter`, `iter().any()` or scan inside a loop over the same
  collection, group once into a map or set, or let SQL/DuckDB do the join. A
  nested loop over events and entities is quadratic on a busy instance.
- Keep the rules in `AGENTS.md`: derived data is computed in one place, view
  bodies do no sorting or filtering, blocking work runs off the async runtime,
  and new logic goes where it can be tested.
- Add a test that pins the behavior before changing a layout, so the rewrite is
  provably the same.
- Say in the commit body when a change alters what is stored or how it is
  computed, and what the benchmark showed (before and after).
