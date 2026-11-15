---
name: changelog
description: Write, edit and review Liwan's CHANGELOG.md for the people who run and use it, saying what was added, changed and fixed in plain American English with no implementation details. Use when adding a line under the next release, preparing the notes for a release, turning commits or a finished roadmap item into changelog entries, or reviewing an entry.
---

# Changelog

`CHANGELOG.md` is read by two kinds of people: the operator who self-hosts Liwan
and has to decide whether an update will break their setup, and the site owner
who opens the dashboard and wants to know what changed. Neither has seen the
repository. They want to know three things: **what can I do now, what works
better, and what no longer goes wrong.**

Where it appears (so you know who is reading):

- the notes of each GitHub release: `.github/workflows/release.yaml` passes
  `CHANGELOG.md` to `create-gh-release-action` with `prefix: liwan`, so the
  `## [vX.Y.Z] - YYYY-MM-DD` heading is what the release picks up;
- what people read before deciding to update, and the link from the README;
- `crates/api/CHANGELOG.md` and `packages/api/CHANGELOG.md` are separate files
  for the client packages, written for developers integrating them. A change to
  the API belongs in all three when it reaches both.

It is also marketing in the plainest sense: it shows that the project is alive
and cared for. So the claims must be true.

## 1. Read first

1. **The facts of the change**, never your memory of it: the commits since the
   last release, the finished item in `ROADMAP/DONE/`, and the `README.md`
   section for the feature, which is the spec. For anything an operator touches,
   read `data/config.example.toml` and `crates/liwan/src/config.rs`; for
   anything a visitor sees, read the dashboard code in `packages/web`.
2. **The existing file**, top to bottom, for the voice and the section names.
3. **`AGENTS.md`** for what a change must not say (no tracking, no raw IPs) and
   for the commits that never get an entry.

Check the exact names: `Settings > Teams`, an environment variable,
`LIWAN_BASE_URL`, a config table `[limits]`. Do not guess them.

## 2. Does it belong?

Ask: **would someone running Liwan, or reading its dashboard, notice this?** If
yes, it gets an entry. If no, it does not.

**Include**

- A new capability, and where to find it or how to enable it.
- A change in behavior, and what stays the same for people who already run it.
- A config key that is new, renamed or removed, said as an instruction
  ("set `trusted_headers` to ..."), because the operator must act on it.
- A problem someone could see: a wrong number, a crash, a dashboard that would
  not load, a tracker that dropped an event, a confusing message.
- Speed they can feel, with the number when it was measured ("a year of reports
  now loads in under a second on ..."), never "faster" alone.
- A dependency or certificate update **only** when it has a security
  consequence, as v1.8.1 does.
- Something removed or no longer supported, said plainly, with what to do.

**Leave out**

- Refactors, renames, tests, CI, build and release scripts, formatting,
  documentation, dependency bumps with nothing user-visible.
- Internal file paths, table names, module names, SQL, migration numbers.
- A bug introduced and fixed within the same unreleased work.
- Several commits for one feature: they become **one** entry.

When in doubt, leave it out. A short list people read beats a complete list they
skip.

## 3. How to write one entry

One bullet, one idea, usually one to three sentences. The shape:

1. **Lead with what the person can now do, or what no longer goes wrong.**
2. **Then the one detail they need to use it:** the page or dialog
   (*Settings > Collection*), the config key, or the limit that would surprise
   them.
3. **Then reassurance when data is involved:** "Existing data is kept",
   "Sessions already recorded are not changed", "The migration runs once on
   startup." Say it when it applies.

For a fix, **say what the person saw, not what was wrong in the code.** "Fixed
the dashboard showing an empty graph for ranges that start today", not "fixed an
off-by-one in the bucket builder".

Rules for the words:

- American English. Second person ("you") or the thing itself; present tense
  for what Liwan does now. Start features with the verb or the thing ("Filter
  reports by ...", "Liwan now ..."), and fixes with "Fixed".
- Use the terms the product uses: project, entity, team, dashboard, report,
  event, pageview, session, bounce rate, API key, self-hosted.
- Name screens and buttons exactly as the dashboard shows them, in italics:
  *Settings > Projects*, *Add project*. Name config keys in backticks,
  `[limits]`.
- No code: no file paths, type names, variables, SQL, error codes, command
  lines, crate versions.
- Calm. No exclamation marks, no "we're excited", no "powerful", "seamless",
  "effortless", "unleash", "game-changing", "blazingly fast". A small change is a
  short line.
- Honest. Only what is true and was checked. State limits plainly ("needs a
  reverse proxy that sets the header"). Do not promise a speed-up without a
  number.
- Say what changed **for people who already run it**: if behavior changed, say
  what is the same and what is different.

## 4. The file

`CHANGELOG.md` has `## [vX.Y.Z] - YYYY-MM-DD` sections, newest first. New work
goes under the next version's heading; if no heading exists yet, add
`## [Unreleased]` at the top and let the release move it. Releases are cut by
pushing a `liwan-vX.Y.Z` tag; the version also lives in the workspace
`Cargo.toml`.

**Every release has the same shape.** Sections from the file's own legend, in
this order, and leave out a section that has nothing in it:

```
## [v1.9.0] - 2026-11-20

### ⚠️ Breaking Changes

- What stops working, and what to do instead.

### Features

- A change that stands alone.

### Bug Fixes

- Fixed something people could see.

### Security

- A vulnerability that is fixed, said without giving an exploit recipe.

### Other

- Dependency and maintenance notes that an operator still needs.
```

- **`⚠️ Breaking Changes`** is for anything an operator must act on after
  updating, or anything that changes recorded data. If a line here is missed,
  someone's instance breaks, so lead the release with it.
- **`Features`** holds everything new and every improvement to how something
  works: a capability, a changed behavior, something removed, a speed-up you can
  feel. **`Bug Fixes`** holds problems that are fixed. A change that is both
  becomes two bullets, one in each.
- **Subsections (`#### Title`)** group related changes when three or more are
  about the same area. A subsection needs at least two. Titles are short and
  name the area an operator knows: *Teams*, *Custom events*, *API keys*. No
  version numbers, no code, no verbs.
- Bullets are `- ` lines wrapped at about 80 columns with a two-space
  continuation indent. No dates, commit hashes, issue or pull request numbers,
  and no author names.
- **Order:** inside a section, standalone bullets first, then subsections, and
  within each the changes most people will notice first. Small tweaks go last.
- A small release is just `### Features` or `### Bug Fixes` with a few bullets
  and no subsections. Do not add structure to look organized.
- Do not rewrite a released section except to fix a mistake of fact or a typo.
  Rewrite `Unreleased` entries freely; if you touch one that breaks these rules
  (internal paths, jargon), fix it.
- Merge, do not repeat: before adding a line, read what is already under the
  next release and extend the existing bullet if it is the same feature.

There is no changelog test in this repository, and the release job does not
validate the structure: a malformed entry ships silently, and the release notes
show it. Read the rendered section once as the operator before you commit.

## 5. Examples

**A feature, from implementation to user**

> Not this: *Added `GET /api/dashboard/reports/custom-events` returning
> aggregated custom event counts keyed by name, built on DuckDB window
> functions.*

> This: *See how often each custom event happens. The project dashboard lists
> every event you send — sign-up, purchase, download — with its count and trend
> for the selected range. Events without a name are ignored.*

**A breaking change**

> Not this: *Client IP detection now trusts only loopback proxies for
> X-Forwarded-For.*

> This: *The client IP is now read only from proxies on the local machine. If
> Liwan runs behind a proxy on another host, set `trusted_proxies` to that
> address or every visitor appears to come from the proxy.*

**A fix**

> Not this: *Fixed a DuckDB interval cast that returned the wrong bucket for
> DST transitions.*

> This: *Fixed report graphs shifting an hour at daylight saving time.*

**No entry at all:** `refactor: split reports module`, `ci: pin rust 1.99`,
`test: dashboard ranges`, `chore: update deps`. An operator sees nothing.

## 6. Steps

1. Gather the facts (§1) and list the changes in one line each.
2. Cross out everything that fails §2.
3. Group what is left by feature, so one feature is one bullet.
4. Write each bullet with §3, checking names against the dashboard and the
   example config.
5. Sort each into a section, then group three or more related ones under a
   subsection (§4), merging with what is already there.
6. Read the new section from the top as someone who has not read the code or the
   diff.
7. Report which commits you left out and why, in one line each, so the human can
   overrule you.

## 7. Audit before you finish

- [ ] Every bullet answers *what can I do now / what works better / what no
      longer goes wrong*.
- [ ] Nothing an operator must act on (a config key, a migration, a removed
      feature) is missing or buried below Features.
- [ ] No file paths, type names, SQL, error codes, or internal jargon.
- [ ] Screen and button names match the dashboard exactly and are in italics;
      config keys are in backticks and match `data/config.example.toml`.
- [ ] American English, calm, no hype words, no exclamation marks.
- [ ] Nothing is claimed that was not checked; limits and "what stays the same"
      are stated.
- [ ] One feature, one bullet; no repeats of what is already listed.
- [ ] Nothing internal slipped in (refactors, CI, docs, tests, routine
      dependency bumps).
- [ ] Every release is ordered Breaking Changes, Features, Bug Fixes, Security,
      Other; improvements are Features, fixed problems are Bug Fixes.
- [ ] Subsections hold three or more related changes (never fewer than two) and
      sit after the section's standalone bullets.
- [ ] Most noticeable first; the heading is `## [vX.Y.Z] - YYYY-MM-DD` so the
      release job finds it.
