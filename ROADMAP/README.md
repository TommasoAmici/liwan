# Roadmap

Planned work lives in `TODO/`, shipped work moves to `DONE/`. One file per
feature, named `YYYY-MM-DD-topic.md` after the date it was added, not a
schedule.

Each file is a stand-alone brief for the agent or person who picks it up:

```markdown
# Report filters on custom properties

Why this is worth doing, in a short paragraph: what is wrong today or what
cannot be done yet.

- [ ] One slice that can be built, checked and committed on its own.
- [ ] The next slice.

## Implementation context

Decisions with their reasons, the files and functions involved, how to prove
each slice, and what is deliberately out of scope.
```

Use the `add-todo` skill to write one and the `work` skill to build one. A plan
is committed on its own, before or beside the code, never mixed into it.
