# Reorder projects on the home page

The home page lists one card per project (the websites Liwan tracks), and today
the order is whatever SQLite returns: insertion order, with no control anywhere
in the dashboard. Someone running several projects has to scan every card to
find the one they watch, and a quiet project can sit above a busy one forever.
This plan gives administrators explicit up/down controls in Settings > Projects
and stores one order in the app database, so the home page and every other
project list follow it.

## Checklist

- [x] Add the order column and its initial values. Create
      `crates/liwan/src/migrations/app/V12__project_order.sql`:

  ```sql
  alter table projects add column sort_order integer not null default 0;

  update projects
  set sort_order = (
      select count(*) from projects as earlier where earlier.rowid < projects.rowid
  );
  ```

  V11 (`2026-11-15-backend-performance.md`) is the latest app migration; no
  other plan in `ROADMAP/TODO` claims V12. The backfill runs once over a table
  with one row per project, never over events, and keeps the order existing
  installs show today (rowid order, which is insertion order). A fresh database
  starts empty, so the update touches no rows. Do not add an index: the projects
  table has tens of rows, `order by` sorts them in memory, and the list is not
  on an ingest path. Do not edit any shipped migration.

  Then make the list order real in `crates/liwan/src/app/core/projects.rs`:

  - `create` inserts `sort_order = (select coalesce(max(sort_order) + 1, 0) from
    projects)` so a new project is appended at the end. It runs in the existing
    transaction, which serializes concurrent creates.
  - `all` becomes `select ... from projects order by sort_order, display_name
    collate nocase, id`. The name and id are only tie-breakers for rows that
    share a position.

  In `crates/liwan/src/app/core/entities.rs`, order both project lists the same
  way: `entities.projects` and `projects_by_entity` append `order by p.sort_order,
  p.display_name collate nocase, p.id` to their joins. This is what makes the
  Settings > Entities screens agree with the home page.

  Prove it with a unit test in `projects.rs` that creates three projects and
  asserts `all()` returns them in creation order. In `crates/liwan/src/app/db.rs`,
  add a test beside `app_migrations_create_the_entity_index` that opens an
  in-memory SQLite connection, creates a minimal old `projects` table, inserts
  `a`, `b`, `c`, executes `include_str!("../migrations/app/V12__project_order.sql")`,
  and asserts `select id from projects order by sort_order` returns `a, b, c`.
  That is the proof the backfill preserves the old order without running the
  whole app.

- [ ] Add the store method that writes an order. In `projects.rs`, add
      `pub fn reorder(&self, project_ids: &[String]) -> Result<()>` that runs in
      one transaction and writes `sort_order = index` for each ID. It must
      accept exactly the current set of projects: reject an unknown ID, a
      duplicate, or a list that is missing a project with an error such as
      `project order must list every project exactly once`. On rejection the
      stored order must be unchanged, so validate the whole list before the
      first `update`. Gaps after a delete are fine and need no renumbering.

  Unit tests in `projects.rs`: reorder three projects and assert `all()` follows
  the new order; reject a missing ID, an extra ID and a duplicate, asserting the
  order is untouched each time. Build the app the way the funnels tests do
  (`Liwan::new_memory(Config::default())`, then `app.projects`). Do not make
  this method public through the server yet; the endpoint is the next slice.

- [ ] Add the admin endpoint. In `crates/liwan/src/web/routes/admin.rs`:

  - Register `put(reorder_projects_handler)` at `/projects/order`, next to the
    existing `/projects` route.
  - Add `ReorderProjectsRequest { project_ids: Vec<String> }` with
    `#[serde(rename_all = "camelCase")]` and `JsonSchema`, so the wire shape is
    `{ "projectIds": ["...", "..."] }`.
  - The handler takes `Admin(_)` and calls `app.projects.reorder` on
    `spawn_blocking`, returning `empty_response()`. A validation failure is a
    400 (`http_err("Failed to update project order", StatusCode::BAD_REQUEST)`),
    not a 500.

  The `ProjectResponse` and `ProjectsResponse` shapes do not change; the order is
  carried by the array. There is no config key and no change to `packages/api` or
  `crates/api`; they are ingest clients with no dashboard endpoints. Run
  `cargo run -- dev generate-openapi` (or let a debug server start) to regenerate
  `packages/web/src/api/dashboard.ts`, and commit the regenerated file.

  Integration tests in `crates/liwan/tests/admin.rs`, named `project_order_...`
  so they can be run together: create three projects, log in as admin,
  `PUT /api/dashboard/projects/order` with the IDs in a new order, and assert
  `GET /api/dashboard/projects` returns that order; assert a signed-in non-admin
  gets 403; assert a payload missing a project, naming an unknown project and
  repeating an ID each get 400 and leave the previous order in place; create a
  fourth project after reordering and assert it appears last.

- [ ] Add the controls in Settings > Projects. In
      `packages/web/src/components/settings/projects/index.tsx`, the existing
      `ProjectsTable` already knows `role`; when the viewer is an admin and
      there are at least two projects, add an `Order` column before `Name` with
      an up and a down button per row. Use `ArrowUpIcon`/`ArrowDownIcon` from
      `lucide-react`, the `button-ghost` class, `type="button"`, and
      `aria-label={`Move ${row.displayName} up`}` / `... down`. Disable up on the
      first row and down on the last row.

  The click sends the whole new order immediately with
  `api["/api/dashboard/projects/order"].put({ json: { projectIds } })`. Do not
  optimistically reorder the table: keep the server order until
  `invalidateProjects()` refetches it, which updates the home page's
  `["projects"]` query too. Disable the Order buttons while the request is in
  flight and re-enable them only after the invalidation's refetch settles
  (`invalidateProjects()` returns a promise; wait for it), never as soon as the
  PUT returns. Otherwise a quick second click builds its request from the old
  array and silently undoes the first move. On failure show
  `createToast("Failed to update project order", "error")` and invalidate again
  to resync. No success toast: one per arrow click would be noise. Clear the
  saving flag in the same `finally` after either refetch settles, so the buttons
  stay disabled until the table holds the server order. The `Table`
  render callback receives only the row today; computing the row's index with
  `projects.findIndex` is fine at this size, and a backwards-compatible optional
  index argument on `Column.render` is also allowed. Do not restructure the
  shared table otherwise.

  Put the move arithmetic in a small exported helper beside the table (take an
  array, an index and `-1`/`1`, return the new array) and test it with
  `packages/web/src/components/settings/projects/order.test.ts` for the first,
  middle, last and out-of-range cases. In `packages/web/src/pages/settings/projects.astro`,
  extend the description to say that administrators choose the order projects
  appear on the home page, so non-admins understand why the list has this order.

- [ ] Tell the people who use it. In `CHANGELOG.md` under `## [Unreleased]` /
      `### Features`, add a short plain-American-English line: administrators
      can change the order of projects on the home page, and the order applies
      to every project list. In `README.md`, extend the "Understand your
      traffic" bullet with one sentence saying administrators can put the
      projects they follow first. No `data/config.example.toml` change, because
      this adds no operator setting. No `AGENTS.md` change, because it adds no
      script, config key or command.

## Implementation context

**Decisions (2026-11-16).**

- The order belongs to the whole instance, not to a user or a browser. An
  administrator sets it once; signed-in users, anonymous visitors of public
  projects, and every other project list see the same order. A per-user order
  would need a second table and endpoint and would not help the public home
  page; a localStorage order would be lost on the next device.
- The order lives in a new `projects.sort_order` column in the SQLite app
  database. It is project data, like the display name and visibility, not a
  config file setting, and it survives a browser or server restart.
- Administrators reorder with up/down buttons in Settings > Projects, following
  the funnel step editor's pattern. This is keyboard accessible and needs no
  drag-and-drop dependency. The home page itself stays read-only.
- Existing installs keep the order they show today, because the migration
  backfills positions from rowid order. A new project is appended at the end.
- The `/api/dashboard/projects` response is unchanged. The endpoint's `GET`
  order becomes the configured order, and the new `PUT` takes the full ordered
  ID list. A stale or partial list is rejected rather than guessed at, so two
  administrators cannot silently lose a project from the order.
- `ProjectResponse` does not expose `sortOrder`. The array position is the only
  thing the dashboard needs, and leaving the field out keeps the generated
  client and every existing consumer unchanged.
- The dashboard setting has no config key, so `data/config.example.toml` is
  untouched. `packages/api` and `crates/api` are not changed: neither client has
  dashboard endpoints, and the ingest wire format is untouched.
- Nothing in this plan changes collected data, events, reports or retention.

**Storage and read order (2026-11-16).**

- `projects.sort_order` is an `integer not null default 0` column added by
  `crates/liwan/src/migrations/app/V12__project_order.sql`. The migration
  backfills positions from rowid order once, so an existing install keeps the
  order it showed before the upgrade.
- `LiwanProjects::all` sorts by `sort_order, display_name collate nocase, id`;
  `create` appends with `(select coalesce(max(sort_order) + 1, 0) from projects)`
  inside its existing transaction. `LiwanEntities::projects` and
  `projects_by_entity` use the same ordering, so the entity screens agree with
  the home page. `projects_by_entity` scans globally ordered rows into a
  per-entity map, which still leaves each entity's vector in project order.
- Tests: `app::core::projects::tests` covers creation order, a configured order,
  the name tie-break and appending a new project.
  `app::db::tests::project_order_migration_preserves_the_existing_order` runs the
  migration SQL against a minimal old `projects` table, without the full
  migration runner.

**Landmarks.**

- `crates/liwan/src/migrations/app/`: V11 is `V11__indexes.sql`; V12 is free.
  The DuckDB migrations in `migrations/events/` are separate and not involved.
- `crates/liwan/src/app/core/projects.rs`: `create` around line 90, `all`
  around line 74, `get` around line 58.
- `crates/liwan/src/app/core/entities.rs`: `projects` around line 102,
  `projects_by_entity` around line 119.
- `crates/liwan/src/web/routes/admin.rs`: router around lines 43-49, handlers
  around lines 512-584, `ProjectResponse` around line 157.
- `crates/liwan/src/app/db.rs`: migration test around line 212.
- `packages/web/src/components/settings/projects/index.tsx`: `ProjectsTable`
  around line 63, `useProjects` around line 64, `role` around line 65.
- `packages/web/src/components/ui/table.tsx`: `Column.render` around line 8 and
  the row map around line 52.
- `packages/web/src/pages/settings/projects.astro`: description around line 11.
- `packages/web/src/hooks/api.ts`: `useProjects` around line 45,
  `invalidateProjects` around line 406.
- `packages/web/src/components/dashboard/projects/index.tsx`: maps
  `data.projects` in order around line 111; it needs no code change.
- `packages/web/src/api/dashboard.ts` is the generated spec; it is written by
  `web::save_spec`.
- `crates/liwan/tests/admin.rs`: project creation around lines 18-39 and the
  project list assertions around lines 128-164.

**How to prove it.**

- `mise exec -- cargo test -p liwan app::core::projects` runs the store unit
  tests; `mise exec -- cargo test -p liwan app::db::tests` runs the migration
  backfill test; `mise exec -- cargo test -p liwan --test admin project_order`
  runs the HTTP tests. Run `mise run rust:test` once before committing.
- `mise run web:test` typechecks the dashboard and runs the helper test.
  `mise run rust:lint` catches clippy and formatting.
- Manual check: create three projects, reorder them in Settings > Projects,
  open the home page in another tab and as an anonymous visitor, then restart
  the server and confirm the order persists. To see the migration on an old
  database, copy an existing `liwan-app.sqlite`, start the server on it, and
  confirm the home page order is the same before and after the upgrade; the
  `cargo dev seed` database has two projects.

**What the worker may decide.** Helper names, the exact icon size, CSS class
names, the test module layout, and whether the order column reads the index from
`findIndex` or an optional `render` argument. It may not change the instance-wide
scope, the SQLite storage, the Settings > Projects location, the up/down pattern,
the full-list request shape, the error behavior, or the fact that the home page
stays read-only.

**Out of scope.** Per-user or per-browser orders; drag-and-drop; pinning,
favorites or hiding projects; sorting by traffic; ordering entities, funnels or
UTM tags; renumbering positions after deletes; live updates between two open
sessions; a settings page for the order outside the project table; and any
change to `packages/api` or `crates/api`.
