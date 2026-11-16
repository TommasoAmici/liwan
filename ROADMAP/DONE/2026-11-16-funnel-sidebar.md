# Move saved funnels into a sidebar

On the Funnels screen, the saved funnels are a wrapping row of buttons above the
report. A project can have many funnels with long names, and the row pushes the
report down, wraps unpredictably, and reads like a set of tabs even though the
buttons select one report rather than switch between independent pages.
A vertical rail beside the report is a better structure: the funnel names are a
navigation list, the report is the content, and the list can grow without moving
the report.

This is a layout change on the Funnels screen only. The shared project
navigation row (Overview, Funnels, UTM tags) stays where it is.

## Checklist

- [x] Restructure the screen in
      `packages/web/src/components/dashboard/project/funnels/index.tsx` and
      `funnels.module.css`:

  - Replace the wrapping `.funnelRow` with a two-zone workspace: a sticky
    `<nav className={styles.funnelRail} aria-label="Saved funnels">` on the left
    and a `min-width: 0` report pane on the right. Use a grid with
    `grid-template-columns: minmax(9rem, 12rem) minmax(0, 1fr)` and a `1rem`
    gap, inside the existing `.screen` column. The page shell stays 64 rem; do
    not widen it.
  - The rail holds one full-width primary `New funnel` button for an
    administrator, then a `<ul>` of funnel buttons, one per saved funnel, full
    width and left-aligned. Keep `aria-pressed={funnel.id === selected?.id}` and
    the existing click that sets `selectedId`. Long names use
    `overflow-wrap: anywhere` and wrap inside the rail. A viewer (non-admin)
    sees the same list without the button.
  - Make the rail `position: sticky; top: 1rem; align-self: start;` with
    `max-height: calc(100vh - 2rem)`. Put the button above a `.funnelList` that
    owns `overflow-y: auto` and `min-height: 0`, so the action stays put, the
    list can shrink inside the rail, and a project with many funnels scrolls the
    list, not the page.
  - Restyle the selected funnel so it no longer looks like a tab: a
    `--surface-hover` background, `--text-title` text, a 600 weight, and a 2px
    `--accent` left border on `aria-pressed="true"`, with the usual hover on
    other rows. Use existing tokens only; do not add a color or size token. The
    selected state must stay visible without relying on color alone, so keep the
    weight change and the left bar together.
  - Move `SelectFilters` into the report pane, above the report card, so the
    filters align with the report they affect. The range control stays in the
    page header next to the project name. The report card's contents, the
    empty/failed/loading states, the editor, and the delete dialog are
    unchanged.
  - Below `52rem`, hide the rail and show one labeled `Funnel` select above the
    filters in the report pane, with the same `New funnel` button beside it for
    an administrator. Let the row wrap so the button drops below the select at
    320 px. The select lists every saved funnel, uses
    `value={selected?.id ?? ""}`, and sets `selectedId` on change. Keep the
    label visible above the control; do not use a placeholder as the label.
    At this width the rail, including its button, is `display: none`, so exactly
    one `New funnel` control and one funnel selector are exposed at a time. If
    both are in the DOM, hide each with the same breakpoint as its container;
    do not use `visibility` or `opacity`.
  - The select row and the rail are not rendered when the project has no
    funnels: the existing full-width empty card with its `New funnel` button
    stays as it is. The loading and list-error states also stay full width.
  - The report pane must survive the narrower column: `.tableScroll` already
    scrolls the report table, and `.reportHeader` already stacks at `40rem`.
    Do not add a second mobile control, a drawer, or a JS breakpoint hook; the
    native select is the narrow layout.

- [x] Document the visible change. Add one sentence at the end of the
      **See where visitors drop off** paragraph in `README.md`: "Saved funnels
      sit in a sidebar beside the report, and on narrow screens you choose one
      from the Funnel menu." Append one `### Features` bullet under the existing
      `## [Unreleased]` heading in `CHANGELOG.md` per the `changelog` skill, for
      example: "Saved funnels now sit in a sidebar beside the report on the
      *Funnels* screen. On narrow screens, choose the funnel from the *Funnel*
      menu." Do not add a second `## [Unreleased]` or `### Features` heading,
      and do not mention CSS grid, classes, or breakpoints.

## Implementation context

**Decisions (2026-11-16, user answers).**

- Only the saved funnel list moves. The shared project navigation row
  (Overview, Funnels, UTM tags) stays at the top of every project screen; this
  plan does not restructure the app shell or the other project pages.
- On screens narrower than `52rem`, the saved funnel list becomes one labeled
  `Funnel` select, not a horizontal tab row and not a drawer. This keeps the
  control compact with any number of funnels, works by keyboard and on touch,
  and avoids open/close state and focus trapping. The selected report still
  shows its name in the report heading.
- A wide sidebar uses a quiet selected state (surface, weight, left accent bar)
  rather than the filled accent pill the old row used. The filled pill is what
  made the list read as tabs; the rail should read as navigation.
- The rail is sticky and scrolls its own list when it is taller than the
  viewport. Liwan does not limit how many funnels a project can save, so the list
  must not push the report off-screen.
- The layout uses the existing 64 rem shell. The report table keeps its internal
  horizontal scroll; the page itself must never scroll sideways.

**Landmarks.**

- `packages/web/src/components/dashboard/project/funnels/index.tsx` renders the
  header, `ProjectNav`, the current `.toolbar` and `.funnelRow`, `SelectFilters`,
  and the report card. `selected` is `funnels.find(...) ?? funnels[0]`; keep that
  fallback so deleting the selected funnel selects the first remaining one.
- `packages/web/src/components/dashboard/project/funnels/funnels.module.css`
  has `.screen`, `.header`, `.card`, `.toolbar`, `.empty`, `.funnelRow`,
  `.funnelButton`, `.reportHeader`, and `.tableScroll`. Replace the row rules;
  keep the report and dialog rules. The event-suggestions plan
  ([2026-11-16-funnel-event-suggestions.md](2026-11-16-funnel-event-suggestions.md))
  owns the dialog and suggestion popup classes in this file; do not rename or
  remove them.
- `packages/web/src/components/dashboard/project/filter.tsx` is the
  `SelectFilters` control. It already wraps its filters, so it can live in the
  narrower report pane without a second control.
- `packages/web/src/components/dashboard/project/project-nav.tsx` is the shared
  Overview/Funnels/UTM tags row. Touch it only if the page structure needs it,
  and do not change its links, order, or `aria-current`.
- `README.md` line 30 is the **See where visitors drop off** paragraph.
  `CHANGELOG.md` line 19 is `## [Unreleased]` with the existing `### Features`
  list.

**What the sidebar must not change.**

- Selecting a funnel still changes only `selectedId`; it does not navigate,
  rewrite the URL, or refetch the list. The report hook keeps its current
  placeholder behavior and selection fallback.
- `New funnel`, `Edit`, `Delete`, the empty states, the hidden-events message,
  and the report table keep their current wording, permissions, and behavior.
  The create button remains administrator-only.
- The page still works for a signed-out visitor of a public project: no New
  funnel, no Edit/Delete, and the rail or select still lists the saved funnels.
- No new state, route, API, migration, config key, or dependency.

**How to prove it.**

- `mise run web:test` typechecks and runs the web tests; `mise run web:build`
  builds the embedded dashboard.
- Browser check against a server on a seeded project:
  - With several funnels including a 100-character name, the rail stays beside
    the report, long names wrap inside it, the selected row is obvious, and the
    report does not move down. The report table scrolls inside its own box and
    the page does not scroll sideways.
  - Add enough funnels to overflow the viewport and confirm the rail list
    scrolls internally while the `New funnel` button stays visible.
  - At 52rem and below, the rail disappears, one labeled `Funnel` select
    appears, and changing it selects the same report. Above 52rem, the sidebar
    is back and the select is gone. Exactly one `New funnel` control is visible
    at every width.
  - Keyboard: Tab reaches every funnel row, Enter selects one, the selected
    state is visible, the native select works without a mouse, and the dialog
    still opens and closes. Check focus rings in both themes.
  - Delete the selected funnel and confirm the first remaining funnel is
    selected in both layouts. Check the no-funnels empty state and the list
    failure state.
  - Check 320 px, 768 px, 1024 px, 1440 px, and 200% zoom in light and dark,
    with the browser console and network log clean.
- This is presentational, so no Rust test is required. Add a web unit test only
  if a pure layout helper is extracted; do not extract one just to have a test.

**Out of scope.**

- Turning the shared Overview/Funnels/UTM tags navigation into a sidebar.
- A collapsible drawer, a filter/search box inside the rail, funnel counts or
  sparklines in the rail, drag reordering, pinning, or URL state for the
  selected funnel.
- Moving the range control into the sidebar, changing the report card, or
  changing funnel permissions, validation, storage, or the API.
- Any change to the editor dialog, including the custom-event suggestions
  planned separately.

**Ownership and shared work.**

This plan owns the screen layout in
`packages/web/src/components/dashboard/project/funnels/index.tsx` and the
`.screen`, `.toolbar`, rail, list, picker, and report-pane classes in
`funnels.module.css`, plus one sentence in `README.md` and one `### Features`
bullet in `CHANGELOG.md`.

[2026-11-16-funnel-event-suggestions.md](2026-11-16-funnel-event-suggestions.md)
changes the editor dialog and the suggestion popup classes in the same CSS file,
and adds its own README sentence and `### Features` bullet. The two plans are
additive: keep both entries whichever ships first, and neither plan may rename
or remove the other's classes. Do not have two workers rewrite the same block of
`funnels.module.css`; this plan owns the screen and report classes, that plan
owns the dialog and suggestion classes.

The open [2026-11-16-utm-field-help.md](2026-11-16-utm-field-help.md) plan adds
one `### Features` bullet to the same `[Unreleased]` list for the UTM tag
editor, and one sentence to a different `README.md` paragraph. Its bullet is
additive too: keep all three, and do not replace the UTM entry with this plan's.
It touches no funnel code.

The shipped [2026-11-15-project-funnels.md](../DONE/2026-11-15-project-funnels.md)
plan owns the funnel store, query, routes, page, and original editor. Its
checklist's "wrapping row of saved funnel buttons", "button row wraps", and "Do
not invent a separate mobile control" lines describe the layout this plan
replaces. The 2026-11-16 answer above supersedes that narrow-screen decision: a
sidebar cannot fit at 320 px, so one labeled select is the reflow, not a second
way to browse funnels. The rest of that plan, including the selection fallback
and the editor, still applies. The UTM tags and project order plans do not touch
this screen.

**Implementation notes (2026-11-16).**

- The workspace is `.workspace` (grid), `.funnelRail` (sticky column with the
  admin `New funnel` button and `.funnelList`), and `.reportPane` (filters plus
  the unchanged report card). The mobile picker is `.picker` / `.pickerRow`;
  both it and the rail switch on `max-width: 52rem`, so only one `New funnel`
  control is ever displayed.
- The selected row uses `--surface-hover`, `--text-title`, weight 600, and a
  2 px `--accent` left border, with the left edge squared so the bar reads as a
  navigation marker rather than a pill. No token was added.
- Landmark for the sibling plan: the dialog and `.path*` / suggestion rules
  below the report styles were not touched.
- Validation: `mise run web:test` (typecheck plus 67 bun tests) and
  `mise run web:build` both pass. The browser pass (long names, rail overflow,
  52 rem switch, 320/768/1024/1440 px, 200 % zoom, light and dark, focus rings,
  delete fallback) is deferred to integration on the combined branch because
  this worktree must not start a server.
- Integration browser pass (2026-11-16, seeded events on a separate port): at
  1440 px the rail sits beside the report, a long name wraps inside it, and the
  selected row keeps both the accent bar and the weight. The 52 rem switch is
  exact: 831 px hides the rail and shows the labeled select, 833 px does the
  reverse, and exactly one `New funnel` control is visible at every width.
  320 px and dark theme show no horizontal page scroll, and changing the narrow
  select re-renders the report without changing the URL. Not covered: enough
  funnels to overflow the rail (only four were created), 200 % zoom, and the
  viewer and list-error states.
