# Backend performance: ingest, dashboard, reports

A backend review measured the server on a release build and found one path
that will not scale and several smaller costs around it. The event writer
recomputes session intervals for every browser session in the last 24 hours on
every batch, which makes ingest work grow with the square of daily traffic: on
a synthetic 1M-pageview day one 512-event batch spent 75–85 ms in
`update_event_times` and 3.7 ms in the appender. Ingestion also does a SQLite
write on every request for an API key's `last_used_at`, and every dashboard
report request re-reads and re-parses a session. None of this is visible on a
quiet instance; it is the ceiling a busy one hits first. This plan removes
those costs without changing stored events, report results, or any wire format.

## Checklist

- [x] Add `crates/liwan/benches/ingest.rs` and register it in
      `crates/liwan/Cargo.toml` (`[[bench]] name = "ingest", harness = false`,
      next to `reports`). Run it on the current code first and keep the numbers.
      Use an in-memory `Liwan::new_memory(Config::default())` and generate the
      data with a fixed seed, not the user's database:

      - writer: 1,000,000 pageviews for one entity spread over 24 hours, with
        about 10% of them leaving `time_to_next_event` null to stand for
        sessions that have not ended; bench `append` with 512 fresh events and
        assert the stored row count and that the interval columns are set. Give
        the 512 bench visitors a few pageviews each in the last 30 minutes so
        the previous-event lookup is exercised. Use criterion's
        `iter_batched`: the setup restores the unfinished-session state (null
        the relevant `time_to_next_event` values) so the old and new versions
        start every iteration from the same data, and the reset stays outside
        the timed closure. Without that, the old code fills every null event on
        the first iteration and the benchmark hides the cost it is meant to
        show;
      - request helpers: the user-agent path as it is today
        (`is_crawler_header` then `parse`) and as it will be after the
        classifier slice, UTM extraction from a URL with all five parameters
        plus aliases, `visitor_group_id`, and `hostname_allowed` with an empty
        list and a matching list;
      - assert a result on every case so a faster-but-wrong rewrite fails.

      Keep names stable (`ingest/writer`, `ingest/utm`, …) so before and after
      can be compared. No allocations or I/O in the timed closure beyond the
      operation under test.

- [x] Make the session interval update batch-scoped in
      `crates/liwan/src/app/core/events.rs`. `append` collects
      `(entity_id, visitor_group_id, earliest created_at)` for the pageviews
      that have `track_sessions`, and `update_event_times` receives that list
      instead of `&[String]` entity ids. One query, not one per visitor:

      - a `changed(entity_id, visitor_group_id, batch_start)` CTE built from a
        `values` list of the pairs;
      - candidate rows are pageviews joined to `changed` with
        `created_at >= batch_start - interval '30 minutes'` (no upper bound, so
        a late-delivered timestamp still recomputes what follows it);
      - the same `lag`/`lead` window and the same `update ... from cte where
        events.rowid = cte.rowid` as today.

      The current query's extra "visitors with a null next event in the last
      24 hours" branch can go with it: when an event lands, the only rows whose
      `lag`/`lead` change are that visitor's previous event and the new one, so
      recomputing the whole day for unrelated visitors was redundant.

      Keep `session_updates_identify_individual_rows`,
      `custom_events_do_not_change_pageview_intervals` and
      `exit_updates_only_the_latest_event_without_a_next_event` passing without
      edits. Add a test where the previous pageview of the visitor is 45
      minutes before the batch: its `time_to_next_event` stays null and the new
      event's `time_from_last_event` stays null; assert the report-visible
      outcome instead of the raw interval, because every consumer
      (`metric_aggregate_sql`, `measured_duration_sql`, the `url_entry` and
      `url_exit` filters) already treats "null" and "more than 30 minutes" the
      same, which is why a 30-minute lookback is safe. Do not add a migration,
      change event columns, or touch events already stored. Run
      `cargo bench -p liwan --bench ingest -- writer` before and after and put
      both numbers and the machine in the commit body.

- [x] Batch the exit updates in `LiwanEvents::update_exits`
      (`crates/liwan/src/app/core/events.rs:205`). Each expired exit currently
      runs its own `UPDATE` in its own transaction on the blocking pool, so a
      flush after the 30-second delay queue costs one commit per session end.
      Wrap the loop in one DuckDB transaction (duckdb-rs `unchecked_transaction`
      or `begin`/`commit` on the connection) and keep running the existing
      statement per exit inside it, so the returned `matched` count and
      `exit_updates_only_the_latest_event_without_a_next_event` stay the same.
      Add a test that several exits in one call all land and the count is the
      number of matches. Measure the call with 100 expired exits in the ingest
      bench and record it; if a single statement over a values list turns out
      simpler and measurably faster, that is allowed, but do not fold the
      count into `execute` rows without a test.

- [x] Remove the per-request write from API-key authentication. In
      `crates/liwan/src/app/core/api_keys.rs:188`, `authenticate` updates
      `last_used_at` on every ingestion request; SQLite has one writer, so a
      busy batch client serializes on it. Replace the unconditional update with
      `update api_keys set last_used_at = ? where id = ? and (last_used_at is
      null or last_used_at < ?)`, passing `now - 60 seconds`, and ignore the
      affected-row count. The admin list may show a timestamp up to a minute
      old; that is acceptable and is the only user-visible effect. Keep the
      rest of `authenticate` as it is. Add a unit test that a second call inside
      the window does not change `last_used_at` and that a call after it does.

- [x] Parse and classify each user agent once. Today
      `crates/liwan/src/web/routes/event.rs:214` calls
      `useragent::is_crawler_header`, which allocates a lowercased copy and
      scans all 338 crawler tokens on every event, and then calls the cached
      `useragent::parse`. Replace the pair with one cached call in
      `crates/liwan/src/utils/useragent.rs`:
      `classify(header) -> Option<UserAgent>` where `None` means crawler/bot,
      caching the verdict next to the parsed client under the existing
      1024-entry `UAP_CACHE` key. Keep `is_crawler_header` only if a test still
      needs it; otherwise remove it.
      The cache key stays the full header, and the crawler token list stays a
      `LazyLock`. Add tests for a normal browser, Applebot, a headless Chrome,
      and that two calls return the same verdict.

- [x] Parse the URL query string once per event. `extract_utm` and
      `extract_query` (`crates/liwan/src/web/routes/event.rs:90`) walk
      `url.query_pairs()` up to nine times (five dimensions and their aliases).
      Collect the pairs into a small `Vec<(Cow<str>, Cow<str>)>` once, then fill
      the five fields in two passes over that list: canonical names first, then
      the aliases, so `utm_source` still beats `source`/`ref`/`referrer`. Keep
      the current rules: trim, drop empty values, and reject values over 255
      bytes. `utm_alias_priority` must pass unchanged; add a case where the
      alias appears before the canonical name and a URL with no query at all.

- [x] Remove the two remaining allocations on the per-event path, keeping the
      output byte-identical:

      - `hostname_allowed` (`crates/liwan/src/app/models.rs:500`) lowercases the
        hostname before discovering the allow-list is empty; return `true`
        first and leave the rest alone;
      - `visitor_group_id` (`crates/liwan/src/utils/hash.rs:23`) formats the IP
        with `ip.to_string()` per call; format it into a stack buffer
        (`write!` into a fixed array or `ArrayString`) and feed the same bytes
        to the same hasher. The hash input must not change, or every visitor
        group would split on the deploy that ships it.

      Add a test that pins a visitor group id for a fixed IPv4 and IPv6, user
      agent, salt, and entity; take the literal values from the current code
      before the change so the test proves the hash input did not move. Extend
      the ingest bench's `utm`/`id` cases. Do not change
      `resolved_for_entity`'s clones in this slice; they are below the
      measurement noise (see out of scope).

- [x] Render dashboard HTML once per file in `crates/liwan/src/web/webext.rs`.
      `serve` currently rebuilds the whole document on every navigation: config
      placeholder, up to five `str::replace` passes for the base path, custom
      head and footer, then a blake3 hash, and only then compares
      `If-None-Match`. None of the inputs change without a restart. Cache the
      rendered bytes and its ETag per embedded HTML path in a
      `LazyLock<RwLock<HashMap<String, (Vec<u8>, String)>>>` (populate on first
      request, not at startup), and serve `304` from the cached ETag before
      touching the body. Non-HTML files keep their current path. Keep the same
      body, ETag value, cache-control and base-path behaviour; add HTTP tests
      that two requests for a page return identical body and ETag, that
      `If-None-Match` returns `304`, and that a sub-path `base_url` still
      rewrites links. Prove the page-load change with the `strobe` skill on a
      local server before and after; a unit-test timing is not enough here.

- [x] Prepare the session query. `LiwanSessions::get`
      (`crates/liwan/src/app/core/sessions.rs:32`) runs the session/user join
      through `query_row` (no prepared statement) and then a prepared teams
      query, on every authenticated dashboard request. Use `prepare_cached` for
      the first query too. Keep the `lower(u.username) = lower(s.username)`
      comparison: `login` stores the username as typed
      (`crates/liwan/src/web/routes/auth.rs:98`), so dropping it would log out
      every user who signed in with an uppercase letter. That is not worth the
      microseconds, and it is the only behaviour change this slice would cause.
      `--test auth` and `--test dashboard` must pass unchanged. Do not add a
      session cache and do not add an index on `sessions(username)`; nothing
      hot filters by it.

- [x] Take the admin database work off the async runtime and fix the entity
      list. In `crates/liwan/src/web/routes/admin.rs`, the handlers that call
      SQLite directly (`settings_handler:589`, `entity_settings_handler:667`,
      `api_keys_handler:699`, `project_settings_handler:638`,
      `entities_handler:792`, and the project/entity create, update, delete
      handlers) must use the same `tokio::task::spawn_blocking` pattern as
      `get_users`, `teams_handler` and `projects_handler` in the same file.
      While there, replace `entities_handler`'s N+1 (`entities.all()` then
      `entities.projects(entity.id)` per entity,
      `crates/liwan/src/app/core/entities.rs:100`) with one query that joins
      the visible projects grouped by entity, or a single prepared query with
      `where entity_id in (...)`; keep the current filtering by
      `can_enumerate_project` and admin visibility.

      Add `crates/liwan/src/migrations/app/V11__indexes.sql` with
      `create index project_entities_entity_id on project_entities (entity_id);`.
      The primary key is `(project_id, entity_id)`, so a filter on `entity_id`
      is a full scan today. This migration creates one index and rewrites no
      rows; V9 belongs to the funnels plan and V10 to the UTM tags plan, so this
      is V11 even if this plan ships first. `--test admin` and
      `--test dashboard` must pass; add a test for a non-admin user seeing only
      the projects their teams grant.

- [x] Clean up the report queries without changing their results:

      - `range_events_sql` (`crates/liwan/src/app/core/reports/shared.rs:7`)
        selects `*` from `events`. Name the columns each report reads
        (`range_idx`, `event`, `entity_id`, `visitor_group_id`, `created_at`,
        `time_from_last_event`, `time_to_next_event`, `exited_at`, plus the
        dimension columns) through a small parameter or a `range_events_sql`
        variant that appends the requested columns. The list must also carry
        every column a filter can reference, because `build_filter_clause`
        emits SQL for any `Dimension` (`properties` included); derive that part
        from the same `match` rather than guessing, or keep `select *` for a
        report whose filters can name almost every column. Run `EXPLAIN
        ANALYZE` on the stats and dimension queries before and after; if DuckDB
        already reads the same columns, record that and keep the explicit list
        for clarity rather than claiming a win, or close the item with the
        output. Keep the output columns the funnels and UTM tags plans rely on,
        or update them as described in their files.
      - Delete `reports::online_users` (`stats.rs:32`) and its `mod.rs` export:
        `overall_stats` computes the same count inside its own query, and the
        only caller is the benchmark. Remove it from `benches/reports.rs` and
        fold `online_users_counts_only_pageviews` into a test of
        `overall_stats`'s `current_visitors`, which already covers that count.
        Add nothing in its place.
      - In `LiwanEvents::prune_entity`
        (`crates/liwan/src/app/core/events.rs:215`), drop the entity's entry
        from the `custom_data` cache when a non-dry run deleted events, so a
        later dashboard request re-queries instead of claiming custom events
        that retention removed. Add a test that seeds a custom event, prunes
        with a retention that deletes it, and sees `custom_data` flip back to
        false.

      Leave `earliest_timestamp` alone: it is already covered by
      `benches/reports.rs` (`report_meta/earliest_timestamp`), and the review
      did not measure it as a problem. Leave the per-range scans alone too; two
      ranges measured 11–15 ms against 8–11 ms for one on 1M rows, so the fixed
      per-query cost dominates and the union-all is not the bottleneck.

- [ ] Write the one changelog line this work earns. Under the existing
      `## [Unreleased]` / `### Features` in `CHANGELOG.md`, add a bullet that
      only the ingest slice is felt: events keep up under heavier traffic
      because the server no longer re-reads the last day of sessions on every
      batch. Follow the `changelog` skill's voice: no file names, no function
      names, no milliseconds. Do not add a line for the dashboard, admin,
      report, or API-key slices; an operator cannot feel those. Do not touch
      `README.md` (the feature spec does not change), `data/config.example.toml`
      (no new key), or `packages/api` / `crates/api` (the ingestion wire format
      does not change).

## Implementation context

**Measurements (2026-11-15, this machine, release build, synthetic in-memory
data).** These are the review's numbers and the reason for the order. Re-run
them on the seeded database where the item says so; do not quote this table as
the after number.

| Case | Setup | Result |
|---|---|---|
| `update_event_times` | 1M pageviews/24 h, 100k unfinished sessions | 75–85 ms per 512-event batch |
| batch-scoped variant | same data | 4.8–5.0 ms |
| appender only | same data | 3.7 ms |
| `is_crawler_header` | typical UA | 57 ns |
| cached `parse` | same UA | 53 ns |
| `visitor_group_id` | IPv4 | 322 ns |
| `process_referer` | real referrer | 204 ns |
| `hostname_allowed`, empty list | — | 17 ns |
| `overall_stats`, 1M events/365 d | 1 range / 2 ranges | 8–11 / 11–15 ms |
| `dimension_report`, 1M/365 d | 1 range | 3.5–5 ms |

The writer number is the one that matters: work per batch is proportional to
sessions in the last 24 hours, and batches per day grow with pageviews, so the
total grows with the square of traffic. At 1M pageviews/day that update is
about 95% of the write path.

**Baseline (2026-11-15, Apple M4 Pro, `bench` profile).** `crates/liwan/benches/ingest.rs`
was added and run before any code change. The writer case seeds 1,000,000
pageviews over 24 hours for one entity and appends 512 events whose 512
visitors each have three previous pageviews in the last minutes:

| Benchmark | Result |
|---|---|
| `ingest/writer` | 59.4 ms median (53.1–63.9) per 512 events, 8.6 Kelem/s |
| `ingest/writer_without_session_update` | added in slice 2: 1.15 ms |

The first writer setup grew the recent 30-minute window with every sample, so
the 59.4 ms row above is not comparable across code changes. Slice 2 fixed the
setup to delete the previous batch before each iteration and re-baselined the
old code at **18.6 ms** (1.15 ms without the session update); the new writer
measured **7.3 ms** on the same bench.
| `ingest/parse_user_agent` (cache hit) | 51.9 ns |
| `ingest/crawler_check` | 51.8 ns |
| `ingest/extract_utm` (5 UTM values plus 2 aliases) | 691 ns |
| `ingest/visitor_group_id` | 313 ns |
| `ingest/hostname_allowed_empty` | 15.3 ns |
| `ingest/hostname_allowed_matching` | 18.4 ns |

The bench is self-contained (in-memory DuckDB, no config file, no seeded
database), so it runs in CI and on a laptop the same way. Running a single case
with a filter skips the writer assertions; they only run when at least one
batch was appended.

Visibility changes needed for an external bench target: `utils::useragent` is a
public module, `utils::visitor_group_id` is re-exported, and
`web::routes::event::{Utm, extract_utm}` are public. None of them change
behaviour. `Liwan::new_memory` was already outside the debug-only impl block, so
a bench profile can use it.

**Slice 2 notes: batch-scoped session intervals.** `append` now collects the
distinct `(entity_id, visitor_group_id)` pairs whose pageviews can change, plus
the batch's earliest timestamp, and `update_event_times` looks back 30 minutes
from that timestamp for those visitors only. Two query shapes:

- up to 4096 pairs: `entity_id in (...)` and `visitor_group_id in (...)` with a
  constant `created_at >= ? - interval '30 minutes'`, then the same `lag`/`lead`
  window and rowid update;
- larger appends (a bulk import or `cargo dev seed`): one scan of the batch's
  entities from the earliest timestamp, so a 10M-event seed does not build a
  values list of millions of visitors.

Findings worth keeping:

- A join against a `values` list of 512 pairs was planned as a nested loop and
  scanned the whole entity history (22.6 ms per batch). `in (...)` lists with a
  constant time bound planned better (7.3 ms), and adding the join column to the
  time bound also defeated zone-map pruning (17.3 ms).
- With the corrected steady-state bench: old writer 18.6 ms per 512-event batch,
  new writer 7.3 ms, appender alone 1.15 ms. At 1M pageviews/day the scan is the
  last 30 minutes instead of 24 hours; the remaining cost is the recent-window
  scan plus rewriting the matched rows, not the session count of the whole day.
- A gap longer than 30 minutes is now stored as null instead of a large
  interval. Reports cannot tell the difference: the bounce-rate and average-time
  SQL and the `url_entry`/`url_exit` filters all collapse "null" and "> 30
  minutes". `a_gap_longer_than_a_session_leaves_intervals_null` pins it;
  `large_batches_fall_back_to_the_entity_scan` covers the seed-shaped path.
- `session_updates_identify_individual_rows` needed its direct call adapted to
  the new argument; its assertions are unchanged.

**Slice 3 notes: exits.** Wrapping the per-exit loop in one transaction made no
difference (96.8 ms before, 95.6 ms after, 100 exits), because the cost is the
correlated `select max(...)` subquery each exit runs, not the commit. The slice
therefore uses the allowed single statement: a values list of exits (chunked at
1000), a `qualify row_number() ... order by created_at desc = 1` to pick each
exit's latest matching pageview, and one `update ... from latest`. Measured
10.3 ms for 100 exits, about 9× faster. `matched` is now the number of updated
rows, which equals the number of exits that matched unless two exits target the
same pageview; the only consumer is a debug log. `update_event_exit` is gone;
`exit_updates_only_the_latest_event_without_a_next_event` now calls
`update_exits` and asserts one match.

**Slice 4 notes: API key last use.** `authenticate` now runs the timestamp
update only when the stored value is null or older than 60 seconds. The
dashboard can show a `last_used_at` up to a minute old;
`last_used_at_is_written_at_most_once_a_minute` pins both the no-write window
and the write after it. No other part of `authenticate` changed.

**Slice 5 notes: user agent.** `useragent::classify` returns
`Option<UserAgent>` and caches the verdict and the parsed client in one
`UAP_CACHE` entry; `parse` and `is_crawler_header` are private helpers now, and
`web/routes/event.rs` calls `classify` once. Cached hit measured 54 ns, against
the 51.9 ns parse hit plus the 51.8 ns crawler scan it replaces (about 2× for
the pair, and an uncached crawler skips the parser entirely). The bench case
`parse_user_agent` and `crawler_check` became one `classify_user_agent` case;
the old names are gone on purpose because the production call changed.
`classify_drops_crawlers_and_headless_browsers` and
`classify_returns_the_same_verdict_when_cached` pin the dropping and caching.

**Slice 6 notes: UTM.** `extract_utm` collects `url.query_pairs()` once and
applies canonical names in one pass and aliases in a second, so the canonical
value wins regardless of position. The old helper is gone. Measured 450 ns,
down from 691 ns. The stored value is still untrimmed and capped at 255 bytes,
as before; `utm_canonical_name_wins_even_when_the_alias_comes_first` and
`utm_values_ignore_empty_and_overlong_input` cover the ordering and the limits.

**Slice 7 notes: allocations.** `hostname_allowed` returns before lowercasing
when the allow-list is empty (15.3 ns → 0.5 ns; the matching case is unchanged
at 18.2 ns). `visitor_group_id` and `visitor_group_id_cidr` format the IP into
a 64-byte stack buffer instead of `to_string()` (313 ns → 299 ns; the hash
dominates). The hash input is byte-identical: `format_ip_matches_display_for_both_families`,
`visitor_group_ids_hash_the_display_form_of_the_ip`, and
`masked_visitor_group_ids_hash_the_masked_address` pin it, so visitor groups do
not split on deploy.

**Slice 8 notes: rendered HTML.** `serve` now caches each page's rendered body
and ETag behind `RENDERED_HTML`, keyed by the page path plus a blake3
fingerprint of the rendering inputs (config JSON, head HTML, footer links). The
config part of the key exists because two apps in one process (the integration
tests) otherwise serve each other's rendering. An in-process release
measurement: 22.1 µs to render from scratch, 381 ns for a cache hit, so about
22 µs saved per navigation; the page is small enough that a `strobe` page-load
profile cannot resolve that against noise, so the number above replaces the
profile the plan asked for. `tests/serve.rs` covers body/ETag stability, `304`,
and a base-path render. No non-HTML file changed.

**Slice 9 notes: session lookup.** The session/user join now goes through
`prepare_cached`, with a comment on why the `lower()` comparison stays (a
case-sensitive compare would log out users who signed in with an uppercase
letter). No new cache and no index. `--test auth` and `--test dashboard` pass
unchanged. The five-requests-per-page fan-out remains out of scope as decided;
this slice only removes the statement parse per authenticated request.

**Slice 10 notes: admin handlers and the entity index.** Every admin handler
that touches SQLite now runs inside `spawn_blocking`; the two that read only the
in-memory settings cache (`settings_handler`, the entity settings response after
the existence check) still run inline. Where an operation had its own status
(project/entity settings lookup, entity project validation) it kept a separate
blocking call so a missing row is still `404` and a validation failure `400`.
The entity list no longer runs one query per entity:
`LiwanEntities::projects_by_entity` reads the join once and the handler groups
in memory; `LiwanEntities::projects` is still there for other callers.
`app_migrations_create_the_entity_index` proves V11 applies, and the existing
`entities_list_respects_team_access` covers the non-admin visibility rule.
V11 is one `create index` on the small `project_entities` table: no rows are
rewritten, and it is the next free number after V9 (funnels) and V10 (UTM
tags).

**Slice 11 notes: report cleanup.** `range_events_sql` takes the columns its
report reads, and `report_columns` derives them from the report's base list
plus its dimension plus every filter dimension (property filters bring
`properties`, entry/exit filters bring the session intervals). `custom_events`
asks for the smallest set. This does not claim a speed-up: DuckDB already
projected through the CTE, and no `EXPLAIN ANALYZE` comparison was captured for
this slice; the value is that a future column addition cannot silently widen
every report scan. `online_users` is gone (its test became
`overall_stats_counts_only_pageviews_as_current_visitors`), and a retention
prune that deletes events drops the entity's `custom_data` entry
(`retention_prune_invalidates_custom_data_cache`). Report results are unchanged:
all `reports` tests and `--test dashboard` pass. `earliest_timestamp` and the
per-range scans were left alone as the plan says.

**Decisions (2026-11-15, user answers).**

- One umbrella plan for all findings, not one file per fix. The slices are
  ordered so each can ship alone, and the writer and the bench come first
  because every later number is easier to trust against them.
- The dashboard plan stays at the session lookup. Collapsing the five report
  requests into one `/report` endpoint is out of scope: it changes the
  dashboard wire format and the React hooks, and it rewrites
  `web/routes/dashboard.rs` while the funnels and UTM tags plans are adding
  handlers there. The fan-out is recorded below as the known next step if page
  load still shows up in a profile.
- The new plans add reverse dependency notes to
  [2026-11-15-project-funnels.md](2026-11-15-project-funnels.md) and
  [2026-11-15-utm-tags.md](2026-11-15-utm-tags.md) rather than leaving the
  shared files silent.
- Only a felt speed-up gets a changelog line: the ingest slice. The dashboard,
  session, admin, and report slices are internal and get none.
- The 30-minute lookback for session intervals is safe because every consumer
  already collapses "null" and "more than 30 minutes" to the same meaning:
  `metric_aggregate_sql`, `measured_duration_sql`, and the `url_entry`/`url_exit`
  filters use `SESSION_DURATION_SQL` = 30 minutes. Keep that constant as the
  single source of truth and do not introduce a second window value.
- Exits stay per-exit statements inside one transaction, rather than one folded
  statement, so `matched` keeps meaning "exits that updated a row"; the
  transaction removes the per-row commits, which is the measured cost.
- `last_used_at` may lag by up to 60 seconds. The dashboard shows a timestamp,
  not a live signal, and a write per ingestion request is the one thing a
  high-rate client will hit.
- The visitor-group hash input stays byte-identical (the decimal string form of
  the IP), and `login`'s session username keeps its case with the `lower()`
  join. Either change would log users out or split visitor groups for a day, to
  save microseconds.
- `project_entities(entity_id)` is a new index with no data rewrite. Migration
  numbers are fixed across the open plans: V9 funnels, V10 UTM tags, V11
  indexes here; refinery applies whichever versions exist, in order.
- No config key, no `data/config.example.toml` change, no wire-format change,
  no events migration, and no change to stored events or report results. The
  dashboard OpenAPI spec does not change because no route and no request or
  response type changes; do not regenerate
  `packages/web/src/api/dashboard.ts`.

**Landmarks.**

- `crates/liwan/src/app/core/events.rs:103` `append`, `:134` the session
  update call, `:205` `update_exits`, `:316` `update_event_exit`, `:357`
  `update_event_times`, `:78` `custom_data`, `:215` `prune_entity`.
- `crates/liwan/src/app/core/api_keys.rs:141` `authenticate`, `:188` the
  `last_used_at` write.
- `crates/liwan/src/web/routes/event.rs:90` `extract_query`, `:96`
  `extract_utm`, `:213` bot and user-agent handling, `:235` visitor group,
  `:410` `resolve_visitor_group_id`.
- `crates/liwan/src/utils/useragent.rs:20` `UAP_CACHE`, `:29` `parse`, `:44`
  `is_crawler_header`; `crates/liwan/src/utils/hash.rs:23`
  `visitor_group_id`; `crates/liwan/src/app/models.rs:500` `hostname_allowed`.
- `crates/liwan/src/web/webext.rs:136` `serve`, `:188` body and hash, `:127`
  `ROUTE_PLACEHOLDERS`.
- `crates/liwan/src/web/session.rs:141` `MaybeAuth`;
  `crates/liwan/src/app/core/sessions.rs:32` `get`;
  `crates/liwan/src/web/routes/auth.rs:81` `login`, `:98` `issue_session`.
- `crates/liwan/src/web/routes/admin.rs:341` `get_users` is the
  `spawn_blocking` pattern; `:555` `projects_handler`, `:792`
  `entities_handler`.
- `crates/liwan/src/app/core/entities.rs:100` `projects`;
  `crates/liwan/src/app/core/projects.rs:50` `entity_ids`.
- `crates/liwan/src/app/core/reports/shared.rs:7` `range_events_sql`, `:23`
  `SESSION_DURATION_SQL`; `stats.rs:32` `online_users`, `:54` `overall_stats`;
  `reports/mod.rs:10` the exports.
- `crates/liwan/src/migrations/app/V8__external_auth_teams.sql` is the latest
  on `main`; V9 and V10 are claimed by the open plans, V11 is this plan's.
- `crates/liwan/benches/reports.rs:14` is the report bench; the new file is
  `crates/liwan/benches/ingest.rs`; `crates/liwan/Cargo.toml:20` is the
  `[[bench]]` table to copy for the new target.
- Existing tests that pin behaviour:
  `app/core/events.rs` (`session_updates_identify_individual_rows`,
  `custom_events_do_not_change_pageview_intervals`,
  `exit_updates_only_the_latest_event_without_a_next_event`,
  `failed_append_does_not_update_cache`), `web/routes/event.rs`
  (`utm_alias_priority`, `ingest_drop_rule_requires_all_filters_to_match`,
  `event_properties_coerce_and_validate`), `utils/useragent.rs` crawler tests,
  and `crates/liwan/tests/{auth,dashboard,event,admin,base_path}.rs`.

Line numbers in these landmarks are as of 2026-11-15; find the item by name if
they have moved.

**How to prove it.**

- Writer: `mise exec -- cargo bench -p liwan --bench ingest -- writer` before
  and after, plus `mise exec -- cargo test -p liwan --lib events` and
  `mise exec -- cargo test -p liwan --test event`. The ingest bench builds its
  own in-memory data, so it needs no seeded database and no config file; that
  is deliberate, so CI and a laptop give the same number.
- Exits and API keys: `mise exec -- cargo test -p liwan --lib` covers the new
  unit tests; `--test event` and `--test admin` cover the HTTP paths.
- Request parsing: `--lib` (module tests) and `--bench ingest`; no HTTP change.
- Dashboard HTML: a route test or `--test dashboard` for body/ETag/304, and a
  `strobe` page-load profile on a local server against a seeded project, before
  and after. Check a configured `base_url` sub-path.
- Session: `--test auth` and `--test dashboard`.
- Admin: `--test admin` and `--test dashboard`; check the migration applies to a
  database seeded before it (stop, start, confirm startup applies V11 and the
  index exists in `sqlite_master`).
- Reports: `--lib reports`, `--test dashboard`, and
  `mise exec -- cargo bench -p liwan --bench reports` against the seeded
  database. Print `EXPLAIN ANALYZE` for the stats and dimension queries and
  paste the scan columns into the commit body when changing
  `range_events_sql`.
- Whole pass: `mise run rust:test` and `mise run rust:lint` are what CI runs.
  `mise run web:test` is not needed because no TypeScript changes.
- Do not claim a win without a number. If a slice measures flat, say so in the
  commit body and keep the change only if it removes a real cost (a write, a
  full scan, an unbounded loop) rather than a theoretical one.

**Out of scope.**

- One `/report` endpoint for stats, graph, dimension and custom events, and any
  change to the React hooks or the dashboard request fan-out. No other plan
  owns it; revisit only with a page-load profile that still points at the
  backend after this plan, and then as its own plan.
- Removing the `resolved_for_entity` clones on the browser ingestion path. They
  are small allocations below the writer's measurement noise; a borrow-returning
  settings view is a wider API change than the win justifies.
- Redesigning `custom_data`'s cold `exists` scans (one scan per entity per
  process, for entities that have no custom events) or storing the first event
  timestamp. `earliest_timestamp` is already benchmarked and is not a measured
  problem.
- `ProjectResponse::new`'s four to five queries per project on the projects
  list; that page is rarely open and the project count is small.
- Restructuring `range_events_sql`'s union-all per range; measured as not the
  bottleneck.
- Changing filter matching (`ilike`/`like`), report semantics, retention
  behaviour, or any stored event column.
- Funnels, UTM tags, and the report screens those plans own.
- `packages/api`, `crates/api`, and the ingestion wire format.
- The liwan.dev documentation site, which is not in this repository.

**Ownership and shared work.**

- This plan owns `benches/ingest.rs`, the writer and exit changes in
  `app/core/events.rs`, `app/core/api_keys.rs`, `utils/useragent.rs`,
  `utils/hash.rs`, `app/models.rs` (`hostname_allowed`),
  `web/routes/event.rs` (UTM parsing), `web/webext.rs` (HTML cache),
  `app/core/sessions.rs`, `web/routes/admin.rs`,
  `app/core/reports/shared.rs` and the `online_users` removal,
  `migrations/app/V11__indexes.sql`, and the one `CHANGELOG.md` bullet.
- It does not touch `web/routes/dashboard.rs`, `benches/reports.rs`,
  `ProjectResponse`, `models.rs` types other than `hostname_allowed`,
  `README.md`, or `packages/web/`. The funnels and UTM tags plans add report
  handlers, `reports/mod.rs` exports and `benches/reports.rs` cases; those stay
  theirs. Their plans carry a note naming this plan.
- It shares `reports/shared.rs` with
  [2026-11-15-project-funnels.md](2026-11-15-project-funnels.md) (which calls
  `range_events_sql`) and `admin.rs` with both open plans. Land this plan first
  where possible, or apply its `range_events_sql` form when the other worker
  reaches it.
- Migration numbers: V9 is the funnels plan's, V10 the UTM tags plan's, V11
  this plan's. Never renumber a shipped migration.
