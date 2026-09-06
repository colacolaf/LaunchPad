# Implementation Plan: Phase 0 Learning Curriculum (§2 Rust fluency + §3 microstructure)

## Overview

The deliverable is the user's fluency, not code. Phase 0's exit gate requires
explaining price-time priority "in 3 sentences, cold, unscripted", and TODO §2
defines done as "you can do it unaided". We ship three documents built entirely
from the real codebase (`domain.rs`, `book.rs`): a guided walkthrough
(worked examples — right for a novice), a closed-book self-test (retrieval
practice — for retention), and a separate answer key (feedback after retrieval,
and no accidental peeking).

## Research basis (what the method is and why)

| Finding | Source | How it shapes the design |
|---|---|---|
| Retrieval beats re-reading (61% vs 40% recall at 1 week) | Roediger & Karpicke 2006 | A closed-book `self-test.md` is the core artifact, not a nice-to-have |
| Worked examples for novices; problem-solving practice flips too early = expertise-reversal effect | Sweller; Renkl | Lessons show fully-worked reasoning first; the quiz comes *after*, never before teaching |
| Generation effect / "explain in your own words" | Multiple (structural-learning, Karpicke 2025 review) | Quiz is short-answer "why" questions, not multiple choice |
| Feedback must follow retrieval | Testing-effect literature | `answers.md` explains, cites file:line, and points to what to re-read on a miss |
| Spacing | Ebbinghaus onward | A built-in day-2 / day-7 re-quiz schedule |
| Desirable difficulty | Bjork | Answer key in a separate file; quiz is closed-book by rule |

## Architecture decisions

- **Docs only; no code changes.** Exercises are *prediction* exercises (predict
  what a snippet does, verify by running), so nothing in `core/` is touched.
- **Every lesson anchors to real code** (file:line) the user already owns.
  Nothing is taught on toy examples they can't map back.
- **100% checklist coverage is a test:** every unchecked §2 item and every §3
  item maps to ≥1 lesson; the mapping table ships at the end of the walkthrough.
- **Quiz integrity:** every question traces to a lesson; `self-test.md`
  contains zero answers.

## Task list

### Phase 1: Implement
- [ ] Task 1: `docs/learning/walkthrough.md` — 8 lessons, each: Read (anchors) →
  Guided why (worked reasoning) → Check yourself (retrieval Qs) → Do (prediction
  exercise). Coverage table for §2/§3 at the end.
- [ ] Task 2: `docs/learning/self-test.md` — ~30 interleaved questions across all
  lessons, the §9 exit exercise (3-sentence price-time priority) with rubric,
  day-2/day-7 spacing schedule, scoring guide.
- [ ] Task 3: `docs/learning/answers.md` — keyed answers with code citations and
  re-read pointers.

### Phase 2: Verify (the double-test)
- [ ] Task 4: Coverage audit — walk TODO §2 (all unchecked) + §3 (all items)
  against the mapping table; fix gaps.
- [ ] Task 5: Answer audit — re-verify every answer against the actual code
  (line citations must be true; run the prediction exercises to confirm outputs).
- [ ] Task 6: Integrity check — grep self-test.md for answer leakage; confirm
  every quiz question maps to a lesson.

### Phase 3: Close out
- [ ] Task 7: TODO §2/§3 pointer note, decision-log row, weekly-log entry, commit.

## Acceptance criteria

- [ ] All 8 lessons cite real file:line anchors that exist
- [ ] 100% of unchecked §2 items and 100% of §3 items covered
- [ ] Every answer in answers.md verified against code
- [ ] Zero answers visible in self-test.md
- [ ] The §9 exit exercise + rubric present in self-test.md
- [ ] All gates still green (docs commit can't break them — verified anyway)

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Docs teach "AI's reasoning", not the user's | High | Lessons are Socratic questions + "you must be able to say this back" checks; quiz is the real gate |
| Line numbers drift after future edits | Low | Cite function names + approximate lines; drift is cosmetic, answers cite behavior not just lines |
| Quiz too easy / undesirable difficulty missing | Medium | Mix recall + transfer questions ("what happens if…"), not just lookup |

## Open questions

None — the §9 exit gate defines the bar.
