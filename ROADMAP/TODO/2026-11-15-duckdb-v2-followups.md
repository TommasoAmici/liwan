# DuckDB 2.0 follow-ups: converted storage, verified copies, VARIANT

Liwan bundles DuckDB 2.0.0, and the engine-level gains of 2.0 already apply:
partial aggregates below joins, row-group pruning for `IN` and function
predicates, and aggregation spill to disk. What an existing install does not get
is the v2 storage format, because `upgrade-duckdb` is explicit and most
operators have not run it. The format enables `DICT_FSST` compression by
default, loads column metadata lazily, stores deletes compactly, and validates
data pages more strongly on read. Nobody has measured whether that is worth a
conversion on a real events database, and the command's "checks it" only checks
the storage version and table names, never a data page.

The other applicable 2.0 feature is `VARIANT`, which could replace the
`map(varchar, varchar)` behind custom event properties. The Rust driver turns
that into a write-path question rather than a type swap, so this plan measures
it before committing to a migration. It also picks up the 2.0.1 patch that
DuckDB recommends for production.

## Checklist

- [x] Measure the v2 storage format against v1 on the same data. Use a
      checkout-local `data_dir` and `mise exec -- cargo dev seed` for ten
      million events over a year. Stop the server, record the events file size,
      then run `mise exec -- cargo run -- upgrade-duckdb` with that config,
      which leaves the v1 file as `liwan-events.duckdb.old`. Keep both:

      - copy the whole `data_dir` to a second directory (the reports bench opens
        the app database too, to resolve the project) and replace its
        `liwan-events.duckdb` with the `.old` file, so each version opens under
        the expected name;
      - create a temporary `crates/liwan/liwan.config.toml` with
        `data_dir = "../../liwan-data"` (or the v1 directory) and remove it
        afterwards, because `cargo bench -p liwan` runs with `crates/liwan` as
        the working directory and does not read the repository root config;
      - run `mise exec -- cargo bench -p liwan --bench reports` for both files
        and record every group's median (`report_meta`, `comparison_reports`,
        `overall_report`, `overall_stats`, `custom_events_report`,
        `utm_tag_stats`, `dimension_report`);
      - compare `ls -l`, `pragma_database_size`, `count(*)` of `events`, and a
        checksum of `properties` (for example
        `select count(*), sum(hash(properties::varchar)) from events`) between
        the two files;
      - time a cold start (`liwan users` with the config, twice, once to warm
        the page cache) and record it.

      Put the machine, the event count, both file sizes, every report median,
      and the startup times in the commit body. If the converted file is
      smaller or the reports are faster by more than run-to-run noise, record
      the numbers for item 3 and recommend the conversion there. If not, say so
      plainly and skip the recommendation. Do not claim a win without a number.

- [ ] Make `upgrade_db` read the copy back and report the size change. In
      `crates/liwan/src/cli.rs`, inside the attached block after the existing
      storage-version and table-name checks, compare the rows of every table
      listed by `duckdb_tables()` between `liwan_source` and `liwan_upgraded`
      before any rename:

      - compare `count(*)` first and `bail!` naming the `.upgrade` file and
        both counts on a difference;
      - then require zero rows from both directions, comparing
        `liwan_source.<table>` with `liwan_upgraded.<table>` using
        `except all`, unioned with the reverse. That is the exact comparison
        and it reads every row, so a separate checksum would only add a second
        scan; if DuckDB cannot compare the `properties` map in it, compare
        `properties::varchar` instead and keep the map fixture in the test.
        Quote the table identifier (double quotes, doubled inside) because it
        comes from the catalog; do not interpolate any other value.
      - on a mismatch, `bail!` with the `.upgrade` path and say the original
        was not changed. Leave the partial copy in place for inspection, as
        today, and keep the existing refusals (symlink, non-file, WAL, existing
        `.old` or `.upgrade`).

      In `handle_command`'s `UpgradeDuckDb` arm, print the original and
      converted file sizes next to the paths, for example
      `Upgraded ... (1.2 GiB -> 780 MiB; old file saved to ...)`, using
      `std::fs::metadata` and a small byte formatter. No dependency.

      Tests: extend `upgrades_database_and_keeps_backup` with a non-empty
      `properties` map and assert the converted row still reads back. Add a
      unit test for the comparison helper on two in-memory databases: one
      equal, one with a missing row, one with a changed map value, each
      producing a failure that names the table. The happy path is the existing
      end-to-end test. Measure the added time on the ten-million-event file and
      record it; this is a manual command, not a startup check.

- [ ] Document the measured conversion and the verification. If item 1 found a
      real size or speed effect, add one paragraph to `## Upgrading` in
      `README.md` after the existing text, in operator language, with the
      measured numbers and the event count they came from, for example "On a
      database with ten million events, converting reduced the file from 1.2 GB
      to 780 MB and left report times unchanged." Do not generalize the number
      to every install, do not promise a speed-up, and do not hint that
      updating converts anything by itself. If the measurement was flat, add
      one sentence saying the file size and report times did not change in that
      test, so the section still answers "is it worth it?". If item 2 changed
      the command's output, say that it checks every row and reports the size
      change.

      Add one `### Other` bullet under `## [Unreleased]` in `CHANGELOG.md` per
      the `changelog` skill: `upgrade-duckdb` now reads the converted database
      back and reports how the file size changed. Do not add a second Features
      bullet; the existing one already describes the command. No config key, no
      `data/config.example.toml` entry, no API or wire change.

- [ ] Prove and document that reports survive a small memory limit. On the
      ten-million-event seed, start a report run with `[duckdb] memory_limit`
      set to `256MB` (then `128MB`) and execute the heaviest dashboard queries
      over a year: `overall_stats` for all metrics and a `dimension_report` on
      `path`, plus a property dimension if custom properties exist. Compare the
      values with the unlimited run (they must be identical) and confirm each
      finishes inside the configured `report_timeout_seconds`. DuckDB 2.0
      spills aggregation state to disk instead of failing when it exceeds the
      limit; the spill directory is `liwan-events.duckdb.tmp` next to the
      database, capped by `max_temp_directory_size`. Record the timings and the
      peak spill size if it is visible.

      If a query still fails or times out at 128MB, record that and keep the
      documentation conservative; do not change the default limit or add a
      config key. If it passes, add a comment to the `[duckdb]` section of
      `data/config.example.toml` above `memory_limit`: the limit bounds the
      buffer manager, not the largest report; large reports spill to a
      temporary directory beside the events database and need free disk space;
      the spill cap defaults to 90% of free space. No README change and no
      changelog line: the setting already exists and its behavior is what
      changed.

- [ ] Spike `VARIANT` against the current map before changing anything. This is
      a decision gate, not production code. Use a throwaway test (removed
      before the commit) on a copy of the seeded data and measure:

      - write path: the current appender with `map(varchar, varchar)`, versus a
        `variant` column written (a) by appending a JSON string, (b) by a
        cast-based `INSERT ... SELECT ?::JSON::VARIANT`, and (c) by an append
        followed by an `UPDATE`. The driver facts are: `duckdb` 1.20000.2
        cannot decode a `VARIANT` result column (`Type::Variant` returns
        `FromSqlConversionFailure`), and appending a plain string to a
        `VARIANT` column stores a string variant, not an object. Every read
        would cast to `VARCHAR`, and the write must produce a real object.
      - query path: a property equality filter and a property dimension (with
        and without a key) over one million events with three to five keys per
        event, using the same SQL shapes as `reports/shared.rs` and
        `reports/dimension.rs` (`variant_extract(...)::varchar` and
        `variant_keys`), against the map versions.
      - storage: file size for the same rows, with the default
        `variant_minimum_shredding_size` (30,000 rows per row group). Note that
        shredding only helps when row groups share structure; property keys are
        user-defined and may defeat it.
      - conversion: prototype the migration SQL from the next item on a copy of
        the ten-million-event table and time it, including the free disk it
        needs.

      Proceed to the next item only if all three hold: a single write path
      stores an object variant and stays within 10% of the current writer
      (6.4 ms per 512-event batch on the M4 Pro), a property query is at least
      15% faster or the events file is at least 10% smaller, and the conversion
      finishes in under two minutes while needing less than twice the table
      size in free disk. Otherwise record the numbers in the commit body (or in
      the closing note), keep the map, skip the next item, and do not add an
      events migration or a changelog line.

- [ ] If the spike passes, move `properties` to `VARIANT`. This is an events
      migration,
      `crates/liwan/src/migrations/events/V9__variant_properties.sql` (V8 is the
      latest; no other plan claims an events migration). Do not edit a shipped
      migration:

      ```sql
      alter table events add column properties_variant variant;
      update events
      set properties_variant = properties::json::variant
      where properties is not null;
      alter table events drop column properties;
      alter table events rename column properties_variant to properties;
      ```

      Use the cast that the spike proved (`properties::json::variant` is the
      shape; pin it in the migration test). This rewrites the table on the
      first start after updating, needs free disk space of about the table
      size, and can take minutes on ten million events. Measure it on the seed
      and put the numbers in the commit body. Document the first-start cost in
      `README.md` under `## Upgrading` and in one `### Other` changelog bullet
      (allow free disk space and some startup time); do not add a background
      job, a flag, or a second conversion command.

      Update every reader and writer:

      - `crates/liwan/src/app/models.rs`: `Event::properties()` binds the value
        the spike proved the appender accepts. If that is not an object variant
        without an extra write, stop: the spike's first condition failed.
      - `crates/liwan/src/app/core/reports/shared.rs`: the property filter uses
        `variant_extract(properties, ?)::varchar {filter_value}` and the
        key-presence check uses the variant equivalent of
        `map_contains(properties, ?)`. Pin a missing key and an empty-string
        value; both must behave as they do today.
      - `crates/liwan/src/app/core/reports/dimension.rs`: no key becomes
        `unnest(variant_keys(properties))` with `properties is not null`; with a
        key, `variant_extract(properties, ?)::varchar` and the same
        key-presence scope.
      - `crates/liwan/src/app/core/events.rs` keeps `properties is not null` for
        `custom_data`; the `properties_round_trip_through_appender` test and
        the property filter and dimension tests move to the new shape without
        weakening what they assert.

      The request and response format does not change: a client still sends and
      receives a JSON object of string keys and values. Do not change
      `packages/api`, `crates/api`, or `packages/web/src/api/dashboard.ts`; no
      route or schema changed, so do not regenerate the OpenAPI client. Do not
      add a config key for versioning the layout.

- [ ] Move to DuckDB 2.0.1 when its bindings are published. The release is
      planned for 2026-11-18 and DuckDB recommends it for production. The crate
      version encodes the engine version, so the 2.0.1 line should be
      `1.20001.x`; check crates.io and do not guess. Change the `duckdb`
      requirement in `crates/liwan/Cargo.toml` from `1.20000` to that line,
      keeping the same loose patch policy, then
      `cargo update -p duckdb -p libduckdb-sys`.

      Run `mise exec -- cargo test -p liwan --lib` (the fresh-database test
      still asserts a `v2.` storage tag), the full `mise run rust:test` and
      `mise run rust:lint`, and both benches once (`--bench ingest` and
      `--bench reports` against the seed). Read the 2.0.1 release notes for
      breaking changes that touch Liwan; the lambda, division, and parser
      changes do not. Re-run item 1's measurement only if 2.0.1 changes
      compression or storage behavior.

      If 2.0.1 is not published when the worker starts, leave this item open
      and say so; do not pin a pre-release or a git revision. No changelog line:
      an engine patch is not something an operator can see.

## Implementation context

**Decisions (2026-11-15, user answers).**

- The plan covers the four slices the user picked: the storage-format
  measurement and copy verification, the low-memory report proof, the DuckDB
  2.0.1 patch, and `VARIANT` for properties. The first three ship as written;
  `VARIANT` is gated by the spike and may correctly end with no code change.
- Conversion stays explicit and manual. Updating Liwan must never convert
  anything; the measurement changes the documentation, not the behavior. This
  preserves the shipped decision in
  [2026-11-15-duckdb-v2.md](../DONE/2026-11-15-duckdb-v2.md).
- The converted copy is verified by reading every row back, not by a second
  `liwan check-duckdb` command. The command that makes the claim is the command
  that proves it, and a separate read-only verification command remains out of
  scope until someone needs it outside an upgrade.
- When the measurement finds a win, the README recommends conversion with the
  measured numbers. When it is flat, the README says so; either answer is
  better than the current silence.
- The `VARIANT` decision is the spike's thresholds, not optimism. The writer is
  the constraint: replacing the appender with cast-based inserts or adding a
  second write per batch would undo the ingest work in
  [2026-11-15-backend-performance.md](../DONE/2026-11-15-backend-performance.md).

**Why these and not the other 2.0 features (2026-11-15, repo facts).**

- Quack and `CONNECT` would add a listening server and a second way into the
  event database. That contradicts the embedded, no-required-network-service
  invariant and the shipped decision to stop Liwan before converting.
- Asynchronous I/O and its settings (`async_threads`, `read_ahead_depth`)
  target object storage and HTTP reads. Liwan reads and writes one local file;
  the docs say local storage benefits only a little, so there is nothing to
  tune.
- Triggers would duplicate the batch-scoped session interval update that the
  backend-performance plan just settled, and run per row or per statement on
  the ingest path.
- Data-modifying CTEs, `NEAREST`, the PEG parser, `SHOW`, secure views, and
  extension repositories have no Liwan query or operator surface that needs
  them.
- `preserve_insertion_order=false`, `checkpoint_threshold`, `fsync_mode`, and
  `force_compression` are pre-2.0 tuning knobs. No measurement points at them,
  and forcing a compression method would hide what the v2 default does.

**Facts that shape the work (2026-11-15).**

- A file keeps its old storage format when a 2.0 engine writes to it. The new
  format's benefits therefore arrive only through `upgrade-duckdb`, which copies
  with `STORAGE_VERSION 'latest'` and rewrites every segment.
- `pragma_database_size` reports the file, block, and WAL sizes; the attached
  database list reports `storage_version`. No new dependency or config key is
  needed for the measurement.
- `temp_directory` defaults to `database_name.tmp`, so the spill directory is
  `liwan-events.duckdb.tmp` beside the events file. `max_temp_directory_size`
  defaults to 90% of free disk space. That is what the config comment should say.
- The Rust driver cannot decode a `VARIANT` result: `duckdb` 1.20000.2 returns
  `FromSqlConversionFailure` for `Type::Variant`. Appending a string to a
  `VARIANT` column stores a string variant, not an object (the crate's appender
  test does exactly that). Cast every production read to `VARCHAR`, and treat a
  real object write as the spike's first condition.
- `VARIANT` shredding only applies to row groups with at least
  `variant_minimum_shredding_size` rows (30,000 by default). Property keys are
  user-defined and may not share structure, so do not assume shredding is the
  win.
- `cargo bench -p liwan` runs with `crates/liwan` as the working directory, so
  the temporary config lives there. Remove it before committing.
- `upgrade_db` refuses when `.old` or `.upgrade` already exists, and the WAL
  refusal exists because the copy attaches the source read-only. Keep both; the
  measurement workflow must start from a clean `data_dir`.
- The read-back adds one full scan of both files to a manual command. That is
  acceptable for an upgrade but must never run at startup or per report.

**Measurement: v1 and v2 storage (2026-11-15, Apple M4 Pro, 48 GiB, macOS 27.2).**

`cargo dev seed` was asked for ten million events and stored 7,101,811 over the
last year; the generator stops when the year is used up. A database seeded by
this revision is already `v2.0.0+`, so the v1 file for the comparison was made
from the same rows with `COPY FROM DATABASE` into a `STORAGE_VERSION 'v1.5.0'`
file. Both files are compact copies of the same 7,101,811 events, so the numbers
below isolate the storage format.

| | v1.5.0 | v2.0.0 |
|---|---|---|
| file size (`ls -l`) | 353,906,688 B (337.5 MiB) | 354,430,976 B (338.0 MiB) |
| `pragma_database_size` | 337.5 MiB, 0 free blocks | 338.0 MiB, 0 free blocks |
| `count(*)` of `events` | 7,101,811 | 7,101,811 |
| `sum(hash(properties::varchar))` | 70419795626878733462827723 | identical |
| `liwan users`, twice | 0.03 s, 0.03 s | 0.03 s, 0.03 s |

The v2 file was 524,288 bytes (0.15%) larger. Report medians from
`--bench reports` on the same machine; the third column is a second v1 run made
after the v2 run, because two neighbouring runs are not a stable baseline:

| Benchmark | v1 | v2 | v1 again |
|---|---|---|---|
| `report_meta/earliest_timestamp` | 108.91 µs | 107.77 µs | 115.51 µs |
| `comparison_reports/graph` | 3.43 ms | 3.46 ms | 3.67 ms |
| `comparison_reports/stats` | 5.21 ms | 5.17 ms | 5.44 ms |
| `comparison_reports/dimension` | 2.90 ms | 2.87 ms | 2.89 ms |
| `comparison_reports/custom_events` | 3.58 ms | 3.56 ms | 3.45 ms |
| `overall_report/metric/Views` | 74.72 ms | 71.25 ms | 72.30 ms |
| `overall_report/metric/UniqueVisitors` | 109.69 ms | 105.50 ms | 103.44 ms |
| `overall_report/metric/BounceRate` | 101.29 ms | 97.63 ms | 93.42 ms |
| `overall_report/metric/AvgTimeOnSite` | 100.72 ms | 94.68 ms | 92.69 ms |
| `overall_stats/all_metrics` | 43.26 ms | 40.58 ms | 40.05 ms |
| `custom_events_report/counts` | 14.17 ms | 14.47 ms | 13.01 ms |
| `utm_tag_stats/20_tags` | 256.37 ms | 247.41 ms | 238.78 ms |
| `funnel_report/two_step` | 21.55 ms | 22.09 ms | 20.58 ms |
| `funnel_report/three_step` | 28.98 ms | 28.75 ms | 26.30 ms |
| `dimension_report/Url/Views` | 22.26 ms | 22.29 ms | 21.43 ms |
| `dimension_report/Url/UniqueVisitors` | 48.72 ms | 48.10 ms | 44.95 ms |
| `dimension_report/Url/BounceRate` | 34.89 ms | 34.48 ms | 34.28 ms |
| `dimension_report/Url/AvgTimeOnSite` | 35.33 ms | 33.58 ms | 34.75 ms |

The differences change sign between groups and the second v1 run beats both, so
the format is below run-to-run noise on this data and the file is not smaller.
There is no size or speed win to recommend; the README says so instead.

Conversion cost on the same table: `upgrade-duckdb` took 99.13 s, of which the
`COPY FROM DATABASE` is about 2.0 s and the row-by-row read-back the rest (the
comparison SQL alone measured 120 s cold). It needs free disk for a second copy
of the file. The literal seed-then-upgrade run is different in kind: a fresh
seed file is an uncompact v2 file (592,195,584 B with 860 free blocks), and
`upgrade-duckdb` rewrote it to 355,479,552 B in 101.06 s. That is compaction of
free space, not a property of the new format; do not present it as a v2 win.

**Landmarks.**

- `crates/liwan/src/cli.rs:50` `UpgradeDuckDb`, `:212` dispatch, `:255`
  `upgrade_db`, `:352` `upgrades_database_and_keeps_backup`.
- `crates/liwan/src/app/db.rs:11` `init_duckdb`, `:74`
  `warn_on_outdated_storage_version`, `:190`
  `fresh_database_is_written_in_the_v2_storage_format`.
- `crates/liwan/src/app/models.rs:29` `Event.properties`, `:33`
  `EventProperties`, `:37` `Event::properties()`;
  `crates/liwan/src/app/core/reports/shared.rs:69` `report_columns`, `:178` the
  property filter; `reports/dimension.rs:92` the property dimension;
  `app/core/events.rs:79` `custom_data`.
- `crates/liwan/benches/reports.rs` for the report groups;
  `crates/liwan/benches/ingest.rs` for the writer; `crates/liwan/Cargo.toml:100`
  the `duckdb` requirement.
- `README.md:49` `## Upgrading`; `CHANGELOG.md:19` `## [Unreleased]`;
  `data/config.example.toml:52` `[duckdb]`.
- The driver files with the facts above are outside this repository, under the
  local cargo registry: `duckdb-1.20000.2/src/statement.rs` and
  `src/appender/mod.rs:464`. Cite them in the commit body if the spike fails
  there.

**How to prove it.**

- Storage measurement: the two file sizes, `pragma_database_size`, the checksum
  comparison, the cold-start times, and the full `--bench reports` medians, all
  in the commit body.
- Copy verification: `mise exec -- cargo test -p liwan cli` for the unit and
  fixture tests, plus one real upgrade of the seeded file showing the printed
  sizes and a successful read-back.
- Low memory: the seeded run at 256MB and 128MB, identical values, each inside
  the timeout.
- `VARIANT`: the spike's three numbers (writer per batch, property query,
  file size) and the migration time, in the commit body before any production
  code is written.
- After a `VARIANT` migration: `mise exec -- cargo test -p liwan --lib`
  (property round trip, filters, dimension), `--test dashboard`, and a live
  property filter on the seeded project before and after.
- 2.0.1: `mise run rust:test`, `mise run rust:lint`, both benches.
- Whole pass: `mise run check` once before the handoff. `mise run web:test` is
  only needed if a web file changes; no route or schema changes here, so the
  OpenAPI client is not regenerated.

**Out of scope.**

- Quack/server mode, remote databases, asynchronous I/O tuning, and any
  required network access.
- `VARIANT` if the spike fails; the map stays and the numbers close the idea.
- Triggers, data-modifying CTEs, `NEAREST`, the PEG parser, secure views,
  extension repositories, and the agent-mode CLI.
- Changing `memory_limit` defaults or adding `temp_directory`,
  `max_temp_directory_size`, `preserve_insertion_order`, `checkpoint_threshold`,
  `fsync_mode`, or `force_compression` as config keys.
- A `liwan check-duckdb` command, automatic conversion on startup, a background
  backfill, or a second conversion command.
- Dashboard UI work, `packages/api`, `crates/api`, and the ingestion wire
  format.
- The liwan.dev documentation site, which is not in this repository.

**Ownership and shared work.**

- This plan owns `upgrade_db` and its output in `crates/liwan/src/cli.rs`, the
  `## Upgrading` section of `README.md`, the `[duckdb]` comment in
  `data/config.example.toml`, the property column type and its queries if the
  spike passes, events migration `V9__variant_properties.sql`, and its
  `CHANGELOG.md` `### Other` bullets.
- Events migrations are at V8, so V9 is free. App migrations are separate and
  not touched: V9 belongs to
  [2026-11-15-project-funnels.md](2026-11-15-project-funnels.md), V10 and V11
  shipped, and a shipped migration is never edited.
- [2026-11-15-project-funnels.md](2026-11-15-project-funnels.md) owns app V9,
  the funnel store, query, routes, and page, its README paragraph, and its
  `### Features` bullet. It also reads `reports/shared.rs` and can apply
  property filters through `build_filter_clause`; if this plan changes the
  property column, the funnel report inherits the new SQL without a second
  implementation. Neither plan may assume the map type.
  Both plans append to `CHANGELOG.md` and touch `README.md`; keep every entry,
  whichever ships first, and never add a second `## [Unreleased]` or
  `### Features` heading.
- The shipped plan
  [2026-11-15-duckdb-v2.md](../DONE/2026-11-15-duckdb-v2.md) owns the storage
  warning, the original `upgrade_db`, and the `### Other` engine bullet. This
  plan is a follow-up that completes the command's verification; do not rewrite
  the warning or re-bump the engine outside item 7.

Do not tick items or move this file to `ROADMAP/DONE/`; the work that ships them
does.
