# The Record (C) — 12-month public track record

> The 12-month clock starts **now**. The record is a core deliverable, not a nice-to-have: "12 months of logged decisions" cannot be AI-generated, and it's the essay material for the whole project.

## What goes here

- **Decision log** (`decision-log.md`) — every design choice, every paper trade, every session, every miss. One line per session, ~2 min.
- **Weekly log** (`weekly-log.md`) — one entry per week, no missing weeks.
- **Quarterly write-ups** (`quarterly/`) — Nov, Feb, May, Aug. What worked, what failed, what the benchmarks say. **Include the bad quarters** — that's what makes it unfakeable.

## Rules

1. **Zero missing weeks.** If a week is missed, backfill it honestly (say "missed, backfilled") — never fake an entry.
2. **Honesty over polish.** The record rewards honesty, not fake progress. A plateau published honestly is a win.
3. **No personal data.** This is a public repo — the record logs the *project*, not private life.
4. **Every benchmark number ships with its methodology** (see `docs/benchmarks.md`).

## File map

```
record/
├─ README.md          # You are here
├─ decision-log.md    # Design decisions table (append-only)
├─ weekly-log.md      # Weekly entries (append-only)
└─ quarterly/         # Quarterly write-ups
   └─ YYYY-QN-template.md
```

## Cadence reminders

- **Per session:** one line in the decision log (2 min).
- **Sunday afternoon:** weekly log entry + next-week plan.
- **Nov / Feb / May / Aug:** quarterly write-up.
