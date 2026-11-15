# Working in this repository

Liwan is a self-hosted, privacy-first web analytics server: one Rust binary that
serves an Astro/React dashboard, a tracking script, and both of its databases.

These instructions are for agents and humans working on the code. A skill in
`.agents/skills/` goes into more depth for its own area; read the skill before
doing that kind of work.

## Toolchain

Tool versions are pinned in `mise.toml`: Rust matches `rust-version` in
`Cargo.toml`, Bun matches `packageManager` in `package.json`.

```sh
mise install            # once per machine, or after a version bump
mise run web:build      # tracker script + dashboard (cargo needs the dist)
mise run web:test       # typecheck every package, then bun test
mise run rust:test      # what CI runs: cargo test --all --all-features
mise run rust:lint      # clippy -W warnings, then cargo fmt --check
mise run rust:build     # release binary
mise run check          # all of the above, in one pass
```

`mise exec -- <command>` runs a pinned tool without activating the environment.
CI (`test.yaml`, `test-web.yaml`) runs the same commands; when a command here
changes, change the workflow too.

## Running locally

`cargo run` serves the built dashboard on <http://localhost:9042>; `bun run dev`
serves the dashboard with hot reload on 4321 and proxies `/api` to 9042. Use a
`liwan.config.toml` (gitignored) with `data_dir = "./liwan-data"` so development
data stays in the checkout. `cargo dev seed` (debug builds only) fills it with
10,000,000 events over the last year, users `admin`/`admin` and `user`/`user`,
and two projects; `cargo dev reset-db` starts over.

## Layout

| Path | What lives there |
|---|---|
| `crates/liwan` | The server: `app/` (models, core services, reports), `web/` (Axum routes, embedded dashboard), `migrations/`, `config.rs`, `cli.rs` |
| `crates/liwan/benches/reports.rs` | Criterion benchmarks for the report queries |
| `crates/liwan/tests` | Axum integration tests; `common/mod.rs` builds an in-memory app and test client |
| `crates/api` | Rust client for ingesting events, with its own `CHANGELOG.md` |
| `packages/web` | Astro + React dashboard, served from the binary (pages are prerendered) |
| `packages/tracker` | The browser tracking script; `lib/script.js` is the built file the server embeds |
| `packages/api` | Server-side TypeScript client (fetch, Express and Hono helpers) |
| `packages/screenshots` | Puppeteer screenshots of the dashboard |
| `data/` | Example config, bot and referrer lists, update scripts, the Containerfile |
| `ROADMAP/TODO`, `ROADMAP/DONE` | Planned work and shipped work, one file per feature |

## Data and migrations

There are two databases, and they are not interchangeable:

- **DuckDB** (`events`) holds events and exits. Analytics queries are written
  against it in `crates/liwan/src/app/core/reports/`.
- **SQLite** (`app`) holds users, projects, entities, sessions, settings, teams,
  API keys, and onboarding state.

Migrations are embedded with `refinery` from
`crates/liwan/src/migrations/{app,events}/V<N>__name.sql`. **Never edit a
migration that has shipped.** Add the next number, and say in the pull request
what the migration costs on an existing database (millions of events, not
hundreds).

Config keys are user-visible API. Add them to `crates/liwan/src/config.rs`, to
`data/config.example.toml`, and to the `CHANGELOG.md` entry when they change what
an operator can do. Settings a user changes in the dashboard belong in the
`app` database, not the config file.

## Conventions

- **Rust**: edition 2024, `unsafe_code` forbidden, clippy warnings are errors in
  CI. Errors are `anyhow` in services, `thiserror` where a caller matches on
  them. Blocking database work does not run on the async runtime.
- **TypeScript**: Biome formats with tabs and a 120-column width; `bun run
  typecheck` must pass. Tests are `*.test.ts` next to the code and run with
  `bun test`. CSS is CSS Modules per component, tokens come from
  `packages/web/src/styles/theme.css`, and no new color or size is added per
  section.
- **API shape**: handlers use `aide` schemas; `liwan dev generate-openapi` writes
  the spec. A changed request or response is a breaking change for the clients
  in `packages/api` and `crates/api` — update both and the changelog.
- **Tests**: unit tests live in the module (`#[cfg(test)] mod tests`) when they
  touch one type; anything through HTTP or the database goes in
  `crates/liwan/tests`. A behavior change gets a test that fails before it.
- **Plain, contained changes**: no unrelated refactors in the same change, no
  speculative abstractions, no new dependency without a reason in the commit
  body.

## Commits and changelog

Conventional Commits, subject of at most 70 characters, imperative:
`feat: support multiple date ranges in dashboard reports`. A `feat`, `fix` or
`perf` commit says in its body what changed, why, and what it was measured
against. Do not push: pushes to `main` and tags start publishing workflows.

`CHANGELOG.md` is read by operators and site owners, not by contributors. Add a
line under the next release's `### Features` or `### Bug Fixes` when a person
using Liwan would notice; see the `changelog` skill for the voice and the
section rules. `crates/api` and `packages/api` keep their own changelogs.

## Invariants

- No tracking cookies, no cross-site identifiers, and raw IP addresses are never
  stored. Do not add a field or log line that breaks that.
- The dashboard and the database are built in: no new external service, no
  required network call at startup.
- Every page and asset is served from the binary, under `base_url` when one is
  configured. A new asset path must work under a sub-path.
- An agent that changes what a user sees updates `README.md` (the feature spec)
  in the same change.

## Skills

`.agents/skills/` holds the deeper playbooks: `add-todo`, `changelog`,
`db-performance`, `designing-websites`, `performance`, `state-machines`, `work`.
