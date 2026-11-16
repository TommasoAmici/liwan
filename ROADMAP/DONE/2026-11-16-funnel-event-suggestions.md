# Suggest existing custom events in the funnel editor

The funnel editor already suggests the project's top paths when an
administrator adds a page step, but a custom-event step is a plain text field.
The administrator has to remember the exact event name, and event matching is
case-sensitive, so `Signup` instead of `signup` produces a funnel whose step
silently counts zero visitor groups. The names also already exist in the
custom-events report for the selected range; the editor should offer them
instead of making the administrator retype them.

This changes only the editor input. The saved funnel, its API shape, and the
report query stay as they are.

## Checklist

- [x] Replace the event name text field with a suggestion combo box in
      `packages/web/src/components/dashboard/project/funnels/dialog.tsx`, using
      the same Base UI `Autocomplete` pattern as the page path field just above
      it:

  - Read the suggestions from the existing
    `useCustomEvents({ projectId, range, filters: [], enabled: !customEventsHidden })`
    hook in `packages/web/src/hooks/api.ts`. Pass an empty `filters` array on
    purpose, exactly as the path suggestions ignore the page's report filters;
    the dialog does not receive the filters and a filter must not hide the
    event an administrator is looking for. The hook already uses the selected
    range and refetches every 60 seconds when the range ends today.
  - Keep the report's order: it is already most completions first, then name.
    Map each row to `{ name, completions: row.completions ?? 0 }`. Do not slice
    the list in the dialog; the endpoint already applies
    `report_max_dimension_results`. `pageview` is absent because the
    custom-events report excludes it, which is correct: a pageview belongs in a
    page step.
  - Bind `Autocomplete.Root` with `value={step.name}`,
    `onValueChange={(name) => update(index, { ...step, name })}`,
    `itemToStringValue={(item) => item.name}`, and the same case-insensitive
    substring `filter` the path field uses. Include `openOnInputClick` and
    `autoHighlight="always"`, and keep the input `required`, `maxLength={255}`,
    `placeholder="signup"`, and `aria-label={`Step ${index + 1} event name`}`.
    A typed name that is not in the list is still valid and must be submitted
    unchanged; only `funnelStepFrom` in `step.ts` trims it, as it does today.
  - Show each suggestion's completions beside its name with
    `formatMetricVal(item.completions, "views")` followed by `events`, mirroring
    the path row's view count.
  - Popup states: while the hook is loading and the popup is open, show
    "Loading custom events…"; when there are no matching rows, show "No custom
    events in this range. Type a name to use it."; on a request error, show
    "Could not load custom events. Type a name to use it." Never block saving
    because suggestions failed to load. When `events.data?.truncated` is true,
    show "Showing the top events only." as a non-interactive note inside the
    popup, using the same words as the custom-events card.
  - When `customEventsHidden` is true, do not send the custom-events request at
    all (`enabled: false`), because the endpoint answers 400. The type selector
    already disables choosing Custom event, and the existing warning and blocked
    Save already cover an existing event step. Render the same combo for that
    step, with the current name intact, and use "Custom events are hidden for
    this project." as its empty text. Do not disable the input: the
    administrator still needs to see the name while converting or removing the
    step.
  - Reuse the existing suggestion popup classes in `funnels.module.css` (rename
    the `path*` classes to `suggestion*` only if the shared name reads better;
    either way there is one set of styles, not a second copy). Use theme tokens
    and the existing field sizing. The popup must sit above the dialog, as the
    path popup does, and the row layout must stay readable at 320 px. This plan
    owns the dialog and suggestion classes only;
    [2026-11-16-funnel-sidebar.md](2026-11-16-funnel-sidebar.md) owns the
    screen, rail, list, and picker classes in the same file. Do not rename or
    remove its classes, and keep both README and changelog entries.
  - Do not change the create/update request, the server validation, or any
    other screen. The event field remains the only place that suggests events.

- [x] Document the visible change. Add one sentence to the **See where visitors
      drop off** paragraph in `README.md` after the first sentence: "When you
      add a custom-event step, the editor suggests the events seen in the
      selected range, and you can still type a name that has not appeared
      yet." Append one `### Features` bullet under the existing
      `## [Unreleased]` heading in `CHANGELOG.md` per the `changelog` skill, for
      example: "The funnel editor now suggests the custom events seen in the
      selected range, and still accepts a name that has not appeared yet." Do
      not add a second `## [Unreleased]` or `### Features` heading, and do not
      mention hooks, endpoints, or component names.

## Implementation context

**Decisions (2026-11-16, user answers).**

- Suggestions are the custom events recorded in the selected dashboard range,
  not every event name ever stored. This reuses the existing custom-events
  report and matches how the page path field already works. An event with no
  traffic in the current range is not suggested, but the administrator can
  still type it, so a new event name is never blocked.
- Each suggestion shows its completions for the range, matching the path
  field's view count and the custom-events card's completion number. Do not use
  unique visitors here.
- The field stays free text on purpose: the user asked for the existing events
  plus the option to enter one that is not there yet. A combo box that only
  accepts listed values would make a brand-new event impossible to save before
  its first hit arrives.
- Suggestions ignore the report filters, like the path suggestions do. The
  dialog is opened from a page whose filters are not passed into it, and a
  filter would otherwise hide the event the administrator is looking for.
- No backend change, no new endpoint, no migration, and no new config key. The
  custom-events report already returns names, completions, and a `truncated`
  flag for one range.

**Landmarks.**

- `packages/web/src/components/dashboard/project/funnels/dialog.tsx` has the
  `PathSuggestion` type, the `useDimension` call for top paths, and the path
  `Autocomplete` block that this work mirrors. The event branch is the
  `Event name` input around line 212.
- `packages/web/src/hooks/api.ts` `useCustomEvents` takes `projectId`, `range`,
  `filters`, and `enabled`, and returns `{ data, isLoading, error }` where
  `data` is `{ rows, truncated }`. Each row is `{ name, completions, uniques }`
  and both counts are nullable. `isLoading` is the query's `isFetching`, so it
  stays true during a range or refetch refresh.
- `crates/liwan/src/app/core/reports/custom_events.rs` produces the rows, sorted
  by completions descending then event name, excludes `pageview`, and limits the
  event list to `report_max_dimension_results`.
- `crates/liwan/src/web/routes/dashboard.rs` rejects the custom-events request
  with 400 when `custom_events_display()` is `Hide`. That is why the dialog
  must not fetch when `customEventsHidden` is true.
- `packages/web/src/components/dashboard/project/funnels/funnels.module.css`
  has `.pathPositioner`, `.pathPopup`, `.pathList`, `.pathItem`, `.pathName`,
  `.pathCount`, and `.pathEmpty`; they already handle the portal z-index, the
  anchor width, the empty and highlighted states, and the muted count.
- `packages/web/src/components/dashboard/project/funnels/step.ts` owns
  `EditorStep`, `editorStepFrom`, and the trim in `funnelStepFrom`. Do not add a
  second step model.
- `packages/web/src/components/dashboard/project/custom-events.tsx` is where the
  "Showing the top events only." wording comes from.
- `README.md` line 30 is the **See where visitors drop off** paragraph;
  `CHANGELOG.md` line 19 is `## [Unreleased]` with the existing `### Features`
  list.

**Behavior the worker must not change.**

- Event steps still match the `event` column exactly and case-sensitively on
  the server. Selecting a suggestion fills the exact stored name; typing
  `Signup` still saves `Signup` and still counts nothing. This plan does not add
  case-insensitive matching or a warning for a name that looks like a
  suggestion in different case.
- The funnel must still save with an event name that has no recorded traffic.
  There is no "event must exist" validation on the client or the server.
- An existing event step in a project with custom events hidden must stay
  visible and block Save, as it does today. The suggestion fetch being disabled
  must not hide the step or clear its name.
- The page field, its path suggestions, the step reorder/remove controls, and
  the dialog's save/delete behavior are untouched.

**How to prove it.**

- `mise run web:test` typechecks the dialog against the generated API types and
  runs the web tests. `mise run web:build` proves the dashboard still builds.
- No Rust change, so no Rust test is required. If the worker extracts a pure
  suggestion-mapping helper, add a small `bun test` beside it for null
  completions and preserved order; do not extract one just to have a test.
- Browser check against a server on a seeded project, using the existing
  headless Chrome setup in `packages/screenshots` or a manual browser:
  - Open `/p/<project>/funnels`, open the editor, switch a step to Custom
    event, and confirm the list shows the same names and completion counts as
    the custom-events card for the same range, most used first.
  - Select a suggestion and confirm the field holds the exact name; type a name
    that is not in the list and confirm Save still sends it; use Arrow Down and
    Enter to pick a row, and Escape to close the popup without closing the
    dialog.
  - Set the project's custom events to `hide` and reopen an existing
    event-step funnel: the editor shows the warning, sends no custom-events
    request (no 400 in the network log), and Save stays blocked until the step
    is converted or removed.
  - Check the open popup at 320 px and 1440 px in light and dark, with no
    horizontal page overflow, and confirm the browser console is clean.

**Implementation (2026-11-16, this branch).**

- `EventSuggestion` and the event `Autocomplete` mirror the page path field in
  `dialog.tsx`. `useCustomEvents` is called with an empty `filters` array and
  `enabled: !customEventsHidden`, and rows map to
  `{ name, completions: row.completions ?? 0 }` without slicing.
- The dialog reuses the existing `.path*` classes, so there is no second set of
  styles. The `path*` names stay as they are to keep the merge with the funnel
  sidebar work small.
- Popup states: `Autocomplete.Status` shows "Loading custom events…" while the
  hook fetches, `Autocomplete.Empty` shows the hidden, error, or no-events text,
  and a truncated report adds "Showing the top events only.".
- The integration browser pass found that the pre-existing
  `.pathPositioner[data-empty] { display: none; }` rule hid the positioner in
  exactly the empty-list states Base UI marks, so none of the four popup texts
  were visible. The rule is removed, so the popup shows its status or empty
  content; the `.pathEmpty` styling already existed for it. As a consequence the
  path field's "No paths in this range." is now visible when a query matches
  nothing, which its JSX already intended. Filtering, Escape closing only the
  popup, and the full list on an empty query are unchanged.
- Re-checked after the fix on the combined branch with seeded events: a query
  with no match shows "No custom events in this range. Type a name to use it.",
  the hidden state shows "Custom events are hidden for this project." with zero
  custom-events requests and a disabled Save, and the list is most completions
  first (signup 35.3k through share 3.2k), matching the custom-events report.
  Typing an unlisted name saved it unchanged (`brand_new_event`), Arrow Down +
  Enter picked a row, and Escape closed only the popup. Not covered: the
  `truncated` note (the seeded data has 12 events, below the limit) and the
  loading text (no way to throttle the request in this setup).
- The input keeps `required`, `maxLength={255}`, `placeholder="signup"`, and
  `aria-label`. A `<label htmlFor>` and matching input `id` satisfy Biome's
  `noLabelWithoutControl`, which does not see Base UI's `Input` inside a
  wrapping label.
- Validation: `mise run web:test` (typecheck and 67 bun tests) and
  `mise run web:build` pass, and `biome check` is clean on `dialog.tsx`. Both
  pass again after the empty-popup fix.
- Limitation, unchanged on purpose: event matching stays case-sensitive on the
  server, and a typed name that is not suggested saves exactly as typed.
- The browser check runs on the integration branch, which found the empty-popup
  defect recorded above; this worktree must not start a server.

**Out of scope.**

- An all-time custom event list, a new endpoint, or a backend query that does
  not depend on the selected range.
- Matching suggestions against the report filters, fuzzy or case-insensitive
  event matching, auto-creating events, or a warning when a typed name is close
  to an existing one.
- Suggesting `pageview` as an event, or suggesting events in any screen other
  than the funnel editor.
- Skipping events already used by another step, and any change to funnel
  validation, storage, report semantics, API clients, migrations, or config.

**Ownership and shared work.**

This plan owns the event branch in
`packages/web/src/components/dashboard/project/funnels/dialog.tsx`, the shared
suggestion styles in `funnels.module.css`, one sentence in `README.md`, and one
`### Features` bullet in `CHANGELOG.md`. It reuses `useCustomEvents` and the
custom-events report without changing their contract.

There is no shared work with the open
[2026-11-15-duckdb-v2-followups.md](2026-11-15-duckdb-v2-followups.md) plan,
which changes the storage backend, or with
[2026-11-15-fork-container-image.md](2026-11-15-fork-container-image.md), which
changes the container image. The shipped
[2026-11-15-project-funnels.md](../DONE/2026-11-15-project-funnels.md) plan owns
the funnel store, query, routes, page, and the original editor; this plan only
changes how one step field is filled.

This plan also shares `funnels.module.css` with the open
[2026-11-16-funnel-sidebar.md](2026-11-16-funnel-sidebar.md) plan. This plan owns
the dialog and suggestion classes only; that plan owns the screen, rail, list,
and picker classes. Both plans add one sentence to `README.md` and one
`### Features` bullet to the same `[Unreleased]` list; keep both entries
whichever ships first, and neither plan may rename or remove the other's
classes.

The open [2026-11-16-utm-field-help.md](2026-11-16-utm-field-help.md) plan adds
one `### Features` bullet to the same `[Unreleased]` list for the UTM tag
editor, and one sentence to a different `README.md` paragraph. Its bullet is
additive too: keep all three, and do not replace the UTM entry with this plan's.
It touches no funnel code.
