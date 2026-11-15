# Project funnels

Liwan can report pages and custom events separately, but it cannot show whether
the visitor group that opened `/pricing` went on to sign up and then purchase.
Site owners have to reconstruct that journey from several tables. This plan adds
a Funnels screen to each project where an administrator saves named funnels and
anyone who can view the project sees how many visitor groups reach each step,
with conversion and drop-off for the selected date range and filters.

The screen is part of the project dashboard, not project settings:
`/p/<project>/funnels`. It uses the same date range and filter controls as the
overview, so the funnel is a report, not a separate configuration page.

## Checklist

- [ ] Add the app storage for saved funnels. Create
      `crates/liwan/src/migrations/app/V9__funnels.sql`:

  ```sql
  create table funnels (
      id text primary key not null,
      project_id text not null,
      display_name text not null,
      steps_json text not null,
      foreign key (project_id) references projects(id)
  );

  create index funnels_project_id on funnels (project_id);
  ```

  This is the next app migration after V8; it creates an empty table and an
  index and rewrites no event data. Do not add an events migration: a funnel
  reads the existing `events` table and adds no collected field.
  [2026-11-15-utm-tags.md](2026-11-15-utm-tags.md) owns the following app
  migration, `V10__utm_tags.sql`, even if it ships first; do not renumber this
  plan to V10 or let that plan take V9.

  Add `Funnel`, `FunnelStep`, and `FunnelMatch` to
  `crates/liwan/src/app/models.rs`. A funnel is `{ id, project_id,
  display_name, steps }`. Use a tagged step so the stored JSON and the API
  schema cannot disagree about which fields exist:

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
  #[serde(tag = "type", rename_all = "snake_case")]
  pub enum FunnelStep {
      /// A pageview whose path matches `path`.
      Page {
          path: String,
          #[serde(rename = "matchType", default)]
          match_type: FunnelMatch,
      },
      /// A custom event with this exact name.
      Event { name: String },
  }

  #[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
  #[serde(rename_all = "snake_case")]
  pub enum FunnelMatch {
      #[default]
      Equal,
      Contains,
      StartsWith,
  }
  ```

  Add `crates/liwan/src/app/core/funnels.rs` with `LiwanFunnels`, following the
  shape of `LiwanTeams` and `LiwanProjects`. It stores and reads JSON and never
  touches DuckDB. Methods: `list(project_id)`, `get(project_id, funnel_id)`,
  `create(project_id, display_name, steps)`, `update(project_id, funnel_id,
  display_name, steps) -> bool`, and `delete(project_id, funnel_id) -> bool`.
  Create funnel IDs with `Alphanumeric.sample_string(&mut rand::rng(), 16)`, as
  teams and API keys do. List funnels ordered by `display_name collate nocase,
  id`.

  Validation belongs in the service, not only in the HTTP handlers:

  - Display name is trimmed; it must be 1 to 100 characters.
  - A funnel has 2 to 10 steps. One step is not a funnel; ten is the limit this
    plan supports.
  - A page step's path is trimmed; it must be 1 to 2048 bytes. `match_type` is
    one of the three enum values.
  - An event step's name is trimmed; it must be 1 to 255 bytes and may not be
    `pageview` (use a page step for pageviews).
  - Reject the funnel if any step is invalid; do not save a partially valid
    funnel. Trim names and paths before storing.

  Register the service in `crates/liwan/src/app/core.rs` and
  `crates/liwan/src/app/mod.rs`. In `LiwanProjects::delete`, delete the
  project's funnels in the same transaction that already deletes
  `project_settings`, API-key and team grants. Add unit tests in `funnels.rs`
  for a create/list/update/delete round trip, the minimum and maximum step
  counts, blank values, an event named `pageview`, a funnel from another
  project not being updated or deleted, and project deletion removing its
  funnels.

- [ ] Add the funnel query in
      `crates/liwan/src/app/core/reports/funnels.rs` and export it from
      `reports/mod.rs`. Signature:

  ```rust
  pub fn funnel_report(
      conn: &DuckDBConn,
      entities: &[String],
      steps: &[FunnelStep],
      ranges: &[DateRange],
      filters: &[DimensionFilter],
  ) -> Result<Vec<FunnelReport>>;
  ```

  `FunnelReport` is `{ counts: Vec<u64> }`, one count per step and one report per
  range in request order. Return all zeros for an empty entity list or an empty
  range; never return fewer than `steps.len()` counts.

  Semantics, which the tests must pin:

  - A visitor counts as one `visitor_group_id`. Counts are distinct groups, not
    pageviews, sessions, or people.
  - Steps must occur in array order. Each step matches the first event after
    the previous step's matched event; an event at the exact same timestamp
    does not satisfy the next step. A group can be counted once per step, not
    once per matching event.
  - Counts are cumulative: a group that reaches step 3 is included in steps 1,
    2 and 3. A group that only reaches step 1 is included only in step 1.
  - All events of a funnel must be inside the requested range. A journey that
    starts before the range or finishes after it is not counted. That keeps the
    query to one range scan and matches how every other report defines a range.
  - The selected filters apply to every event in every step, including custom
    events. A `country` filter therefore segments the whole funnel, not only
    the first step.
  - Page steps match `path` (the path after the host, already stripped of a
    trailing slash at ingest), not the full URL. Use the same `ilike`
    comparison as dashboard filters: case-insensitive equality, substring, or
    prefix. Event steps match the `event` column exactly and case-sensitively.
  - Custom event steps and page steps may be mixed freely. A page step must
    also require `event = 'pageview'`; an event step does not look at `path`.

  Implement it with a materialized `candidates` CTE built from
  `range_events_sql`, `entity_id in (...)`, and `build_filter_clause`, followed
  by one `union all` branch per step that selects matching events from
  `candidates`. Then generate one CTE per step:

  ```sql
  step_1 as (
      select range_idx, visitor_group_id, min(created_at) as matched_at
      from step_matches where step_idx = 1
      group by range_idx, visitor_group_id
  ),
  step_2 as (
      select s.range_idx, s.visitor_group_id, min(m.created_at) as matched_at
      from step_1 s
      join step_matches m
        on m.range_idx = s.range_idx
       and m.visitor_group_id = s.visitor_group_id
       and m.step_idx = 2
       and m.created_at > s.matched_at
      group by s.range_idx, s.visitor_group_id
  ),
  ...
  ```

  Count each `step_N` CTE per `range_idx` and fill a zeroed
  `Vec<FunnelReport>` from the rows. Materialize `candidates` and
  `step_matches`; the union must scan the filtered candidates once per step, not
  rescan the base table. Keep parameter order explicit: range bounds, entities,
  filter parameters, then the path or event parameters for each step branch.

  Tests in the module must cover: a three-step page -> event -> event funnel
  with groups that stop at each step; out-of-order events being ignored; a
  group with several matching step-1 events counted once; two independent
  ranges; filters applied to all steps; all three page match types; exact
  event names; an empty range and an empty entity list; and a step pattern
  that also matches a later step (for example a broad page step before a
  narrower one) still requiring two separate events.

- [ ] Prove the funnel query at scale before building the UI. Add a
      `funnel_report` group to `crates/liwan/benches/reports.rs` with a
      two-step page -> event funnel and a three-step page -> event -> event
      funnel, using the seeded paths and event names (`/pricing`, `signup`,
      `purchase`). Run it against a `cargo dev seed` database with its ten
      million events, for example
      `mise exec -- cargo bench -p liwan --bench reports -- funnel`, and record
      the measured times and event count in the commit body. If a three-step
      funnel does not comfortably fit inside the default report timeout, fix
      the query before continuing; do not raise a limit or cache the result as
      a substitute for a query that scales. The result should be a normal
      report, not a background job.

- [ ] Add the admin endpoints to `crates/liwan/src/web/routes/dashboard.rs`.
      They are project-scoped because a funnel belongs to one project:

  - `GET /project/{project_id}/funnels` — any caller who can view the project,
    including anonymous callers on a public project. Returns
    `{ funnels: [...] }` with each funnel's `id`, `displayName`, and `steps`.
    Use `MaybeAuth`, `report_project` or the same `can_view_project` check, and
    return 404 for a project the caller may not see.
  - `POST /project/{project_id}/funnels` — `Admin` only. Body
    `{ displayName, steps }`; returns the created funnel. Reject an event step
    with 400 when the project's `custom_events_display()` is `Hide`.
  - `PUT /project/{project_id}/funnels/{funnel_id}` — `Admin` only. Body
    `{ displayName, steps }`; 404 when the funnel is not in this project.
    Reject an event step with 400 when `custom_events_display()` is `Hide`.
  - `DELETE /project/{project_id}/funnels/{funnel_id}` — `Admin` only; 404
    when the funnel is not in this project.

  Add `custom_events_display: DisplayOverride` to `ProjectResponse` in
  `crates/liwan/src/web/routes/admin.rs` alongside the existing
  `custom_events_hidden` flag. The editor needs the raw override so it can
  disable event steps only when an administrator has explicitly hidden custom
  events, not when the project simply has no events yet. Do not change the
  meaning of `custom_events_hidden`.

  Wire the routes into `router()` and add schemas
  (`FunnelResponse`, `FunnelsResponse`, `CreateFunnelRequest`,
  `UpdateFunnelRequest`, `FunnelReportRequest`, `FunnelReportResponse`). Use
  camelCase request/response fields like the rest of the dashboard API. Empty
  responses for update and delete follow the existing `empty_response()`
  pattern.

  HTTP tests in `crates/liwan/tests/admin.rs` and
  `crates/liwan/tests/dashboard.rs`:

  - An admin creates, lists, updates, and deletes a funnel.
  - A signed-in non-admin can list and report on a project they can view but
    gets 403 from create, update, and delete.
  - An anonymous caller can list and report on a public project, and gets 404
    from the list of a private one.
  - A caller without project access gets 404 from the list.
  - Invalid funnels (one step, eleven steps, blank name, blank path, event
    named `pageview`) are rejected with a 400.
  - Creating or updating an event-step funnel while custom events are
    explicitly hidden is rejected with a 400; a page-only funnel is accepted.
  - Updating or deleting a funnel through another project's URL is a 404.
  - Deleting the project removes its funnels.

  Run `mise exec -- cargo run -- dev generate-openapi` after the handlers and
  schemas exist and commit the regenerated
  `packages/web/src/api/dashboard.ts`. `packages/api` and `crates/api` are not
  touched: this is dashboard-only API, not the ingestion or public read API.

- [ ] Add the report endpoint and its access rules. In the same dashboard
      router add `POST /project/{project_id}/funnels/{funnel_id}/report`. Body
      `{ ranges, filters }` with one to four ranges, matching the existing
      report endpoints. It resolves the saved funnel, so the request cannot
      change the steps. Return `{ reports: [{ counts: [...] }] }` in range
      order.

  Use the same `validate_ranges` limits as the other report handlers. Fetch
  the funnel by both `project_id` and `funnel_id`; a funnel from another
  project is a 404, not a way to report on it. Reject entry and exit page
  filters (the existing `DimensionFilter::is_session_page_filter`) with a 400,
  because those filters depend on pageview session boundaries and are not
  meaningful for an event step. Do not accept an `event` field or a `metric`
  field; a funnel report has no single event or metric.

  Respect the project's custom-event visibility: if
  `display.custom_events_display() == DisplayOverride::Hide` and any step is
  an event step, return 400 with a message saying custom events are hidden for
  this project. Do not silently drop the event steps. Page-only funnels keep
  working when custom events are hidden.

  Run through `run_report` so the query gets the existing concurrency permit,
  connection timeout and interrupt handling. Tests in
  `crates/liwan/tests/dashboard.rs` must cover the counts, range order, a
  filter applied to every step, entry/exit filter rejection, a nonexistent
  funnel, a funnel from another project, a page-only funnel with custom events
  hidden, and an event-step funnel with custom events hidden.

- [ ] Add the Funnels page, project navigation, list, and editor.

  - `packages/web/src/pages/p/[project]/funnels.astro` is a second dynamic
    project page with `getStaticPaths` returning `{ params: { project:
    "project" } }`, rendering `<Funnels client:only="react" />` inside the
    existing `Layout` with the title "Funnels". Add the file to the
    `setPrerender` list in
    `packages/web/astro.config.ts` so dev mode serves arbitrary project IDs.
    No `ROUTE_PLACEHOLDERS` change is needed: the existing `p/` mapping
    already turns `/p/<id>/funnels` into `p/project/funnels`.
  - Use the `getDashboardProjectId()` helper and the `ProjectNav` row that
    [2026-11-15-utm-tags.md](2026-11-15-utm-tags.md) owns; that plan also moves
    `components/dashboard/project/index.tsx` onto the helper. Add only the
    Funnels link to `ProjectNav` when this page ships. The shared row always
    links to Overview (`/p/<id>`) and links to UTM tags (`/p/<id>/utm-tags`)
    only when that screen exists; never link to a page that has not shipped.
    It stays visible on public projects, uses `appPath`, and marks the current
    page with `aria-current="page"`. Do not add navigation to the project list
    on the home page. If the UTM plan has not shipped yet, create the same
    helper and row from its spec (strip `basePath`, parse `/p/<id>` and ignore a
    trailing screen segment, links to Overview and Funnels) rather than a second
    implementation; the UTM plan still owns later changes to the shell.
  - Add types to `packages/web/src/constants.ts` from the generated
    `DashboardSpec` (`Funnel`, `FunnelStep`, `FunnelReport`), not hand-written
    duplicates. Add `useFunnels(projectId)` and `invalidateFunnels(projectId)`
    to `hooks/api.ts`.
  - The page uses `useProject` for access and display settings, `useRange` for
    the persisted range, and `useFunnels` for the list. Show the project header
    without the live stats, the range selector, and a wrapping row of saved
    funnel buttons. The selected funnel is visible in the row; if there are no
    funnels, show "No funnels yet. Create a funnel to see how visitors move
    from one step to the next." to an administrator and "No funnels have been
    set up for this project." to everyone else. Show "Failed to load funnels."
    on a list error and the usual loading spinner. Do not invent a separate
    mobile control; the button row wraps, and may scroll horizontally at 320 px
    instead of clipping long funnel names.
  - The editor is a `Dialog` available only to `role === "admin"` (the server
    enforces this too). It has a required name field and a list of steps. Each
    step chooses Page or Custom event; Page shows a path input and a match
    select with "is exactly", "contains", and "starts with"; Custom event shows
    an event name input. Add Step is disabled at ten steps, Remove Step is
    disabled at two, and up/down buttons reorder steps with accessible labels.
    Help text for page steps: "Use the path after the domain, without a
    trailing slash." If `customEventsDisplay === "hide"`, do not offer Custom
    event in the selector and show "Custom events are hidden for this
    project."; existing event steps stay visible, but the editor marks them and
    blocks Save until the administrator converts or removes them, because the
    server rejects new event steps in that state. Save calls create or update,
    invalidates the funnel list and any report query for that funnel, closes
    the dialog, and selects the saved funnel. Delete opens a confirmation dialog
    naming the funnel and saying that event data is not deleted. After deleting
    the selected funnel, select the first remaining funnel or clear the
    selection.
  - Use the existing `Dialog`, `LoadingSpinner`, toast helpers, theme tokens,
    and CSS modules. Read the `designing-websites` skill before building the
    screen. Check 320 px and 1440 px in both themes, keyboard operation of the
    editor, and the browser console.

- [ ] Add the report visualization to the same page. Add
      `useFunnelReport({ projectId, funnelId, range, filters })` to
      `hooks/api.ts`, posting to the report endpoint and refetching every 60
      seconds when the range ends today, like `useCustomEvents`. The page gets
      its own filter state through `SelectFilters`, using the project's visible
      dimensions and excluding `url_entry` and `url_exit`; the range is the
      shared persisted range.

  Render one row per step in order: step number, a Page or Custom event icon,
  the path/event and match label, visitor count, conversion from step 1, and
  drop-off from the previous step. Draw a bar whose width is relative to the
  step-1 count. Show the overall completion in the report header: "X% of
  visitor groups completed all N steps" with the final count. Use
  `formatMetricVal(count, "views")` for counts and `formatPercent` for
  percentages. When the previous or first step is zero, show "—" for the
  percentages instead of dividing by zero.

  Empty and failure states: "No visitor groups matched the first step in this
  range." when `counts[0]` is zero; "This funnel uses custom events, which are
  hidden for this project." when the report endpoint returns the hidden-events
  400; "Failed to load the funnel report." for other errors. The legend under
  the report says: "Visitor groups, not people. Groups reset once a day, so
  steps must happen before that reset to count." Loading keeps the previous
  rows visible with the existing updating pattern where practical.

  No time-series graph, dimension breakdown, comparison period, or per-step
  property filter in this plan. Those are separate work.

- [ ] Update the user-facing documentation. Add a paragraph to the `## Features`
      section of `README.md` after "Understand your traffic":

  ```markdown
  **See where visitors drop off**\
  Build funnels from pages and custom events and see how many visitor groups
  reach each step. Administrators define funnels per project, and anyone who can
  view the project sees the results for the selected date range and filters.
  Funnel counts are visitor groups, which reset once a day.
  ```

  Append one bullet under the existing `## [Unreleased]` / `### Features`
  headings in `CHANGELOG.md` per the `changelog` skill, for example: "Added
  per-project funnels that show how visitor groups move through ordered pages
  and custom events, with conversion and drop-off at each step." Do not add a
  second `## [Unreleased]` or `### Features` heading, and do not mention
  tables, endpoints, SQL, or internal names. The shipped DuckDB plan
  ([2026-11-15-duckdb-v2.md](../DONE/2026-11-15-duckdb-v2.md)) already created
  those headings and its own `### Other` bullet; this plan only adds the
  funnel bullet. This plan owns the Features paragraph in `README.md`; the
  DuckDB plan owns the separate `## Upgrading` section that now follows it.
  [2026-11-15-utm-tags.md](2026-11-15-utm-tags.md) adds its own Features
  paragraph and its own `### Features` bullet; keep both entries whichever plan
  ships first, and never replace one with the other.

## Implementation context

**Decisions (2026-11-15, user answers).**

- The screen lives in the project dashboard at `/p/<project>/funnels`, linked
  from the project navigation row, not in project settings and not as a card on
  the overview. Funnels are reports and need the range and filters that already
  live on the project screen.
- A project keeps multiple named funnels. Users select one to report on. This
  matches how projects keep more than one question to answer; a single mutable
  funnel would lose the previous definition every time it changed.
- Steps can be pages or custom events, mixed in one funnel. Page-only funnels
  would exclude signup and purchase events; event-only funnels would require
  the site to emit an event for every navigation step. Both types already exist
  in the `events` table and share `visitor_group_id`.
- A visitor is an ordered `visitor_group_id`. Liwan has no person or session
  identifier that survives the daily visitor-group rotation, so this is the
  only honest unit. There is no configurable 1/7/30-day conversion window and
  no cross-day stitching: `visitor_group_id` is
  `hash(IP, user agent, daily salt, entity)` and the salt rotates at
  `visitor_group_rotation_hour`. A step after that rotation belongs to a
  different group and cannot match. The UI must say groups reset once a day.
  Do not add a window setting that suggests otherwise.
- Only admins create, edit, and delete funnels, matching project settings,
  teams, and API keys. Everyone who can view the project, including anonymous
  visitors on a public project, can list funnels and see their reports.
- No new config key. Two to ten steps and the name/path/event length limits are
  product limits in the service, not operator settings. No
  `data/config.example.toml` change.
- No events migration and no new collected field. The funnel reads existing
  events. The app migration is V9 and only creates a table and an index; on an
  existing database with millions of events it is effectively free because
  nothing is rewritten and no event table is scanned at migration time. The UTM
  tags plan owns the following app migration, V10; the two are independent and
  refinery applies whichever versions are present in order.
- No change to `packages/api` or `crates/api`. The new routes are
  `/api/dashboard/...`; the public ingestion and read API and their wire
  formats are untouched.

**Report semantics worth preserving.**

- Counts are cumulative distinct visitor groups per step. Step 1 counts every
  group that matched step 1, even if it never reached step 2. Conversion from
  step 1 is `count_i / count_1`; drop-off from the previous step is
  `count_{i-1} - count_i`.
- A group that completes steps 1, 2, 2, 3 is counted once at step 2 and once at
  step 3. A group that completes step 2 before step 1 and step 2 again after
  step 1 uses the later step-2 event.
- The chain uses `min(created_at)` per step and `created_at >` the previous
  match, so same-timestamp events do not satisfy consecutive steps. This is a
  deliberate conservative rule; ties at sub-microsecond resolution are not
  ordered in the database.
- All step events must be inside the requested range. A funnel that starts
  before the range or finishes after it is not counted. This avoids a second
  range scan and keeps the result consistent with the dashboard's "views in
  this range" model. It also means a funnel at the end of a range can look
  worse than the same funnel queried a day later.
- Filters apply to every event of every step. Entry and exit page filters are
  rejected because they are pageview-session conditions and an event step has
  no session boundary. The funnel page omits those two dimensions from its
  filter control.
- Page steps match `path` (no host, no query, no trailing slash) with the same
  case-insensitive `ilike` rules as dashboard filters. Event steps match the
  exact event name and are case-sensitive, like the custom-events report. A
  page step also requires `event = 'pageview'`.

**Custom events and display settings.**

- `ProjectDisplaySettings::custom_events_display()` returns the raw override
  (`Auto`, `Show`, `Hide`). `custom_events_hidden` is the resolved flag and is
  also true in `Auto` when no custom events exist. The editor needs the raw
  override so an administrator can create an event-step funnel before the
  first event arrives; do not gate the editor on `custom_events_hidden`.
- When the override is `Hide`, create, update, and report reject an event-step
  funnel with 400 rather than leaking a hidden event name or silently dropping
  the step. Page-only funnels continue to work. The page shows the
  hidden-events message for an existing event-step funnel. The editor does not
  offer the Custom event type in that state.

**Landmarks.**

- `crates/liwan/src/migrations/app/V8__external_auth_teams.sql` is the latest
  app migration on `main`; this plan owns `V9__funnels.sql` and the UTM tags
  plan owns `V10__utm_tags.sql`. `crates/liwan/src/app/mod.rs` embeds migrations
  and constructs every service in `from_pools`.
- `crates/liwan/src/app/core/reports/shared.rs` has `range_events_sql`,
  `build_filter_clause` and `ParamVec`; `reports/custom_events.rs` is the best
  small example of a report that builds SQL from a range and filters.
- `crates/liwan/src/web/routes/dashboard.rs` has `report_project`,
  `validate_ranges`, `run_report`, and the existing report handlers.
  `crates/liwan/src/web/routes/admin.rs` has `ProjectResponse::new`, the
  `Admin` extractor patterns, and the project settings handlers.
- `crates/liwan/src/web/webext.rs` maps dynamic-looking paths to the exported
  placeholder (`p/` -> `project`) for the embedded dashboard. A nested
  `p/<id>/funnels` path is already handled by the existing prefix mapping.
- `packages/web/src/pages/p/[...project].astro` is the current project page.
  `packages/web/src/components/dashboard/project/index.tsx` resolves the
  project ID from `window.location.pathname`; the UTM plan moves that to
  `getDashboardProjectId()`. This plan uses the helper for `/p/<id>/funnels` and
  must not add a second parser.
- `packages/web/src/api/dashboard.ts` is generated by
  `liwan dev generate-openapi`; edit handlers and schemas first, then regenerate
  and commit it. `constants.ts` derives API types from that spec.
- `packages/web/src/hooks/api.ts` has `useCustomEvents` as the model for a
  range/filter report hook, including the 60-second refetch for ranges ending
  today. `components/dashboard/project/custom-events.tsx` shows the existing
  card, loading, empty, and dialog patterns.
- `packages/web/src/components/settings/dialogs.tsx` has a reusable
  `DeleteDialog`, but its union only covers project/entity/user/team and its
  endpoint does not carry a funnel ID. Either extend it carefully or write the
  funnel delete dialog in the funnels component with the same wording and
  `Dialog` primitive. Do not make it a project-level delete.
- `README.md` Features is the feature spec; the DuckDB plan already added the
  `## Upgrading` section after it. `CHANGELOG.md` already has `## [Unreleased]`
  and `### Features` from the DuckDB work; append the funnel bullet there. The
  UTM tags plan adds its own Features paragraph and its own bullet; keep both.
  `ProjectNav` and `getDashboardProjectId()` are owned by the UTM plan; this
  plan adds only the Funnels link and uses the helper.

**How to prove it.**

- `mise exec -- cargo test -p liwan funnels` covers the service and report
  module.
- `mise exec -- cargo test -p liwan --test dashboard` and `--test admin` cover
  access, validation, counts and the hidden-events rule.
- `mise run web:test` typechecks the generated spec and runs the web tests,
  including the shared path helper tests owned by the UTM plan.
- `mise run web:build`, `mise run rust:lint`, and the full
  `mise run rust:test` are what CI runs; run them once before the handoff.
- Performance: `mise exec -- cargo dev seed` then
  `mise exec -- cargo bench -p liwan --bench reports -- funnel`. Record the
  event count, machine, and two- and three-step timings in the commit body.
- End to end: run the server against a seeded checkout-local `data_dir`, open
  `/p/public-project/funnels`, create a page -> event funnel, select ranges and
  a filter, and compare the step-1 count with the matching custom-events card
  or dimension card. Check 320 px and 1440 px, light and dark, and the browser
  console. Confirm the static page works under a `base_url` with a sub-path by
  checking the link generated by `appPath` and the server's `p/` placeholder.

**Out of scope.** A settings-tab placement, a funnel card on the overview, a
time-series funnel graph, dimension breakdowns, per-step property filters,
comparison periods, configurable conversion windows, cross-day visitor
stitching, saved segments, exports, alerts, scheduled reports, funnels in
`packages/api` or `crates/api`, an events migration, and any new config key.
Deleting a funnel never deletes event data; deleting a project deletes its
funnel definitions because the project owns them.

**Ownership and shared work.** This plan owns app migration V9, the funnel
types in `models.rs`, the new `core/funnels.rs` and `reports/funnels.rs`, the
funnel routes in `web/routes/dashboard.rs`, the `ProjectResponse` display field,
the Funnels page, its Features paragraph in `README.md`, and its funnel bullet
in `CHANGELOG.md`. The shipped DuckDB plan
([2026-11-15-duckdb-v2.md](../DONE/2026-11-15-duckdb-v2.md)) owns `app/db.rs`,
`upgrade_db`, the README `## Upgrading` section, and the `### Other` changelog
bullet. It already added the `## [Unreleased]` and `### Features` headings, so
this plan appends one bullet under them and never adds a second heading.
[2026-11-15-utm-tags.md](2026-11-15-utm-tags.md) owns app migration V10,
`ProjectNav`, `getDashboardProjectId()`, the report `filters` URL parameter,
the UTM screen, and its own README and changelog entries. This plan adds only
the Funnels link to the shared navigation and uses the helper; it must not
create a second nav, a second helper, or a second project-id refactor. The two
plans also add entries to `constants.ts`, `hooks/api.ts`, `models.rs`,
`app/mod.rs`, `LiwanProjects::delete`, `benches/reports.rs`,
`astro.config.ts`, `README.md`, and `CHANGELOG.md`; those additions are
additive and should be kept together.
