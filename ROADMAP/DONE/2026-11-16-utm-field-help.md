# Explain each UTM field in the tag editor

The UTM tag editor shows five bare text fields — Source, Medium, Campaign,
Content, Term — and most people do not know what belongs in them. The values
come from campaign-tracking conventions rather than from Liwan, and a wrong
guess splits a campaign's traffic into a dimension row nobody meant to create.
The editor should say what each field means, with an example, without leaving
the dialog.

This changes only the editor's help. The saved tag, the tag screen, the API
shape, the report, and the URL builder stay as they are.

## Checklist

- [x] Add an accessible help tooltip to the shared UI. Create
      `packages/web/src/components/ui/help-tip.tsx` and
      `help-tip.module.css`, following the small shared-component pattern of
      `components/ui/snippet.tsx`:

  - Export `HelpTip({ label, description }: { label: string; description: string })`.
    It renders a Base UI tooltip from `@base-ui/react/tooltip` (already a
    dependency, v1.9): `Tooltip.Root`, a `Tooltip.Trigger` with
    `type="button"`, and `Tooltip.Portal` → `Tooltip.Positioner side="top"
    sideOffset={6}` → `Tooltip.Popup`.
  - The trigger is an icon-only button using `CircleHelpIcon` from
    `lucide-react` at 14 px, `aria-hidden` on the SVG. Its accessible name is
    `About <label>`, for example `About Source`. Give the button a hit area of
    at least 24 by 24 px even though the icon is small, so it can be tapped.
  - The popup shows the field name as a heading (the same `label`) and the
    `description` below it. The content is plain text: no links, buttons, or
    other interactive elements, so the tooltip never needs to be hovered.
  - Open on hover after a short delay (`delay={200}` on the trigger) and
    immediately on keyboard focus. Set `closeOnClick={false}` on the trigger so
    a tap on a touch screen opens it and keeps it open; Base UI opens a tooltip
    on focus, and a tap focuses the button. It closes on blur, on Escape, and
    when the dialog closes. Do not use the native `title` attribute: it is not
    keyboard accessible and cannot be styled.
  - Style with the existing tokens (`--text-muted`, `--text`, `--surface-raised`,
    `--border`, `--radius-control`, `--focus`, `--font-size-md`). The positioner
    needs a `z-index` above the dialog's `100` (`dialog.module.css`) because
    the tooltip portals outside the dialog; the popup must stay inside the
    viewport and above the dialog at narrow widths. Keep it readable in light
    and dark themes.
  - `type="button"` is required: the trigger sits inside the editor's `<form>`
    and a default `<button>` would submit the dialog when clicked.

- [x] Put the five descriptions next to the fields in
      `packages/web/src/components/dashboard/project/utm-tags/dialog.tsx`. The
      current `field()` helper at line 101 wraps the label text and input in one
      `<label>`; a help button inside a label would activate the input, so
      restructure each field as:

  ```tsx
  <div className={styles.field}>
    <div className={styles.fieldLabel}>
      <label htmlFor={id}>{label}</label>
      <HelpTip label={label} description={description} />
    </div>
    <input id={id} ... />
  </div>
  ```

  Use one `useId()` per dialog for the five input ids; keep `maxLength={200}`,
  the current value/change handling, and the trim on submit. Add `.field`,
  `.fieldLabel`, and a `.groupHint` to `utm-tags.module.css`; keep the
  two-column `.formFields` grid and its 32 rem one-column breakpoint, and make
  sure the input keeps the same 0.35 rem gap above it that `label > input`
  gave it.

  Above the grid, add one muted paragraph with the group rule:

  > **UTM parameters.** Fill in the ones your campaign uses; at least one is
  > required. Reuse the same values across links so reports add them up.

  The five descriptions, verbatim (the worker may fix a typo but may not change
  the meaning or drop the examples):

  - **Source** — "Where the visit came from: a website, a newsletter, or an ad
    platform. Example: google, newsletter, linkedin."
  - **Medium** — "How the link was shared: email, social media, paid search, or
    another channel. Example: email, social, cpc."
  - **Campaign** — "The campaign or promotion the link belongs to. Use the same
    value on every link in a campaign. Example: spring_sale."
  - **Content** — "Which link or creative in the campaign was used, to tell two
    links apart. Example: header_button, sidebar."
  - **Term** — "The paid keyword that brought the visit, mostly for search ads.
    Example: running_shoes."

  Do not add help to Name or Destination URL: their labels and the existing
  placeholder and preview already explain them. Do not change the preview, the
  create/edit mutation, or the delete dialog. The "at least one is required"
  line matches the server's existing validation; it does not add client-side
  validation or mark a field inline.

- [x] Document the visible change. Add one sentence to the **Campaign links**
      paragraph in `README.md` after the existing text: "The editor explains
      what each UTM field is for." Append one `### Features` bullet under the
      existing `## [Unreleased]` heading in `CHANGELOG.md` per the
      `changelog` skill, for example: "The UTM tag editor now explains what each
      field is for." Do not add a second `## [Unreleased]` or `### Features`
      heading, and do not mention Base UI, components, or tooltips in the
      changelog entry. There is no config key, no `data/config.example.toml`
      entry, and no `AGENTS.md` change.

## Implementation context

**Decisions (2026-11-16).**

- The user asked for help inside the editor because the five fields are
  unfamiliar. A tooltip next to each label keeps the two-column form compact;
  always-visible helper text under five fields would double the dialog's height
  and push the preview and actions down. The group line above the grid carries
  the one rule that applies to all five.
- The help opens on hover, on keyboard focus, and on a tap, because a tooltip
  that only opens on hover is unusable on a touch screen. `closeOnClick={false}`
  is what lets the first tap open it instead of toggling it shut; blur,
  Escape, and closing the dialog all dismiss it.
- The icon button lives outside the `<label>` element. A `<button>` inside a
  label is invalid HTML, and clicking it would also focus or activate the
  input.
- The trigger is an icon-only button, not a span. That gives it a focus ring, a
  keyboard stop, and a screen-reader name without extra code.
- `HelpTip` is a shared `components/ui` component because five copies of the
  same tooltip markup in the dialog would be worse, not because other screens
  are expected to adopt it. The funnel and settings dialogs keep their existing
  inline descriptions; adopting `HelpTip` there is a separate change.
- No new dependency. `@base-ui/react` already provides the tooltip and
  `lucide-react` already provides `CircleHelpIcon`; adding a tooltip library
  would be a second popup implementation next to Base UI.
- Copy is fixed here because it is what the user sees. "Example: google,
  newsletter, linkedin." teaches the value by example, and the Campaign line
  asks for one value across the campaign's links, which is what makes the
  report row add up. The group line states the server's at-least-one rule
  before the user hits it as a toast.
- This adds no API, storage, migration, config, or wire-format change.
  `packages/api` and `crates/api` cover event ingestion only and are untouched.
  There is no new test file: the five descriptions are static copy that
  TypeScript cannot test better than a manual read, and the tooltip is Base UI
  behavior that the existing web test setup (no DOM test environment) cannot
  exercise.
- The shipped UTM plan's note that the dialog has no inline validation still
  stands. The new group line tells the user the rule; an empty submit still
  fails server-side and shows a toast.

**Implementation (2026-11-16).**

- `components/ui/help-tip.tsx` exports `HelpTip({ label, description })` and
  owns `help-tip.module.css`. It is the plan's structure: `Tooltip.Root` →
  `Trigger` (`type="button"`, `delay={200}`, `closeOnClick={false}`,
  `aria-label="About <label>"`, a 24×24 icon button around a 14 px
  `CircleHelpIcon` with `aria-hidden`) → `Portal` → `Positioner`
  (`side="top"`, `sideOffset={6}`, `z-index: 110` so it sits above the dialog's
  `100`) → `Popup`. No `Tooltip.Provider`: in Base UI 1.9 it is optional and
  only groups tooltips for a shared delay.
- The popup is a flex column with the field label as a 600-weight heading and
  the description in `--text-muted`, capped at
  `min(18rem, calc(100vw - 1rem))` so it stays in a 360 px viewport. The
  trigger's `min-height` is reset because the global button rule gives every
  `<button>` `min-height: 2.15rem`.
- `utm-tags/dialog.tsx` keeps the `field()` helper but it now takes the input id
  instead of a max length. The five descriptions live in a typed
  `Record<UtmFieldKey, string>` at module scope, verbatim from the plan. One
  `useId()` per dialog builds `<id>-source`, `<id>-medium`, `<id>-campaign`,
  `<id>-content`, `<id>-term`; the label uses `htmlFor`, and the button sits in
  a sibling `.fieldLabel` row, never inside the `<label>`. `maxLength={200}`,
  value/change handling, and trim-on-submit are unchanged.
- The group line is one `<p className={styles.groupHint}>` directly above
  `.formFields`; `<strong>` carries "UTM parameters." `utm-tags.module.css` adds
  `.groupHint` (muted, `--font-size-sm`), `.field` (column flex, 0.35 rem gap —
  the same space `label > input` used to add), and `.fieldLabel` (row flex).
  The two-column grid and its 32 rem breakpoint are untouched.
- Base UI already scopes Escape to the innermost popup: a nested tooltip marks
  the keydown handled, so Escape closes the tooltip without closing the dialog.
  Blur and closing the dialog also dismiss it. Nothing needed for that beyond
  the default `Root` behavior.
- `README.md` gains the sentence "The editor explains what each UTM field is
  for." at the end of **Campaign links**, and `CHANGELOG.md` gains
  "- The UTM tag editor now explains what each field is for." at the end of the
  `[Unreleased]` `### Features` list. No other line was moved or reworded.
- No test file, per plan: the copy is static and the web test setup has no DOM
  environment. The browser pass (hover, keyboard, touch at 360 px, both themes,
  empty submit toast) is deferred to integration, where the coordinator runs
  the seeded server on the combined branch; this worker was told not to start
  a server or bind 9042/4321.
- Validation: `mise run web:test` (typecheck for all packages, 67 bun tests
  pass), `mise run web:build` (20 pages), and `bun x biome check` on the four
  touched files all pass. No Rust command was run, because the change has no
  Rust.

**Landmarks.**

- `packages/web/src/components/dashboard/project/utm-tags/dialog.tsx:101` the
  `field()` helper; `:113` the `Dialog`; `:139` where the five fields are
  rendered; `:146` the preview block.
- `packages/web/src/components/dashboard/project/utm-tags/utm-tags.module.css:171`
  `.form`, `:177` `.formFields`, `:187` `.preview`.
- `packages/web/src/components/ui/dialog.tsx` the dialog wrapper;
  `dialog.module.css:45` the viewport and `:100` the `z-index` the tooltip must
  sit above.
- `packages/web/src/components/ui/snippet.tsx` is the model for a small shared
  component with its own CSS module.
- `packages/web/src/styles/theme.css:21` text, `:29` borders, `:32` radii, `:43`
  focus, `:49` form tokens.
- Base UI tooltip parts live under
  `node_modules/@base-ui/react/tooltip` (`Root`, `Trigger`, `Portal`,
  `Positioner`, `Popup`); `Trigger` renders a `<button>` and accepts
  `delay` and `closeOnClick`.
- `packages/web/src/components/dashboard/project/funnels/dialog.tsx:208` is the
  existing always-visible `.hint` pattern; leave it alone.
- `README.md` has the **Campaign links** paragraph after **Collect only what you
  need**; `CHANGELOG.md` has `## [Unreleased]` with a `### Features` list that
  already contains the UTM screen bullet.

**How to prove it.**

- `mise run web:test` typechecks the dashboard and runs the existing bun tests;
  `mise run web:build` proves the page and its assets still build. No Rust
  change, so no Rust test is needed.
- With a seeded checkout (`mise exec -- cargo dev seed`), start the server and
  sign in as `admin`. Open `/p/public-project/utm-tags` and the **New UTM tag**
  dialog:
  - Hover each of the five icons and confirm the tooltip names the field and
    shows the exact description above.
  - Tab to an icon: the tooltip opens; press Escape: it closes; Tab away: it
    closes. Clicking an icon must not submit or close the dialog.
  - At 360 px, tap each icon in a touch-emulated browser: the tooltip opens on
    the first tap, is not clipped by the dialog or the viewport, and does not
    cover the field it describes. Check light and dark themes at 360 px and
    1440 px.
  - Save a tag with only a Source and confirm the preview, the saved row, and
    the `View traffic` link behave exactly as before; submit with all five
    empty and confirm the same "at least one UTM value" toast appears.
  - Close and reopen the dialog and confirm no tooltip stays open and the form
    resets as before.

**What the worker may decide.** The CSS values inside the token set, the exact
component props beyond what is described, class names, and whether the copy
lives in a small typed map or inline in `field()`. It may fix a typo in the
copy.

**What the worker may not change.** Which fields get help, the tooltip-versus-
inline choice, the meaning of the descriptions, the group line, the form's
labels, visible behavior on touch and keyboard, or the API, storage, config,
and report. It does not add help to other dialogs.

**Out of scope.** Inline validation or an error under each field, help for
Name and Destination URL, adopting `HelpTip` in the funnel or settings dialogs,
links to external UTM documentation, translations, and tracking whether anyone
opens the help.

**Ownership and shared work.** This plan owns
`packages/web/src/components/ui/help-tip.tsx`, `help-tip.module.css`,
`utm-tags/dialog.tsx`, `utm-tags.module.css`, one sentence in `README.md`, and
one `### Features` bullet in `CHANGELOG.md`.

The open [2026-11-16-funnel-event-suggestions.md](2026-11-16-funnel-event-suggestions.md)
and [2026-11-16-funnel-sidebar.md](2026-11-16-funnel-sidebar.md) plans also add
`### Features` bullets to the same `[Unreleased]` list in `CHANGELOG.md`, and
their own sentences to the **See where visitors drop off** paragraph in
`README.md`. Every line is additive and must sit side by side; neither funnel
plan replaces this bullet and this plan does not replace theirs. They do not use
`HelpTip` and this plan does not touch `funnels/dialog.tsx` or
`funnels/index.tsx`.

The shipped [2026-11-15-utm-tags.md](../DONE/2026-11-15-utm-tags.md) plan owns
the UTM store, routes, screen, and URL builder; this plan only adds help to its
editor. The shipped [2026-11-15-project-funnels.md](../DONE/2026-11-15-project-funnels.md)
plan owns the funnel editor and its inline hint.
