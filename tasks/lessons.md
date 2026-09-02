# Lessons

Patterns that caused rework, and the rule that prevents each. Keep entries
short and prescriptive — this file is read at session start.

Lifecycle: a lesson lives here until it is promoted to a permanent rule in
CLAUDE.md, then its entry is deleted. Promoted so far: skill loading
("Load these first"), plan intent-not-code (Plan Node Default),
consequence-not-shape and mutation verification (Verification Before Done).

---

## Re-load the principle skills between tasks, not once per session

**Correction (2026-08-05):** loading `kiss-principles` / `dry-principles` /
`solid-principles` / `architecture` + CLAUDE.md once at the start of a long
multi-task run is not enough — as context grows, the earliest material is the
first to stop shaping decisions.

**Rule:** on a run with more than a couple of chunks, re-invoke the four skills
and re-read CLAUDE.md between chunks, and always at a phase boundary. When a
skill is already in context the harness answers "instructions unchanged", so the
cost is near zero; when it is not, that is exactly when the reload was needed.
Do not skip it to save tokens — the user has explicitly traded tokens for
quality here.

## A mock is an implementation of the trait

A mock that ignores an argument or returns values the real impl could never
produce is an LSP violation, and it manufactures false confidence: tests pass on
input production cannot generate. Two bugs in this repo traced to exactly that.

**Rule:** whenever a trait method's doc states a guarantee, the mock must model
it or assert it. If production code exists to correct a mock's output, the
relationship is inverted — fix the mock and delete the production code.

## Vector tests are still red-first tests

**Correction (2026-08-05):** when a task's expected call vectors are fully
specified up front (an exact-strings catalog, a task brief's literal
sequence), it is tempting to write the test and the implementation together
"since the answer is already known" — co-designing tests alongside the
implementation happened once in this run. That forfeits the red signal: a
test that never failed never proved it can catch the bug it exists to catch.

**Rule:** even with the exact vector in hand, write the test first and run it
before touching production code. A having-the-answer test still has to fail
for the right reason once. Mutation checks (revert the fix, confirm the test
fails) compensate for a missed red step but don't replace it — they catch
a test that can't fail, not a test that was never proven to fail correctly.

## "Write a failing test" means stop at red

**Correction (2026-08-18):** asked for a failing test to prove an analysis, I
went on to implement the fix because the Stop hook refuses to end a turn with
red tests. The hook constrains *how the turn ends*, not the scope the user set.

**Rule:** when the user asks for the red step only, deliver the red step only.
Satisfy the gate by marking the proof test `#[ignore = "red proof …"]` and
showing it fail with `--ignored`; never widen scope silently to please a hook.
Say what the gate required and what you did about it.

## `.claude/skills/gflow/SKILL.md` is upper-case in git

**Correction (2026-08-18):** on this case-insensitive filesystem `git add
.claude/skills/gflow/skill.md` silently stages nothing — git tracks the file
as `SKILL.md`. Four docs commits in a row "included" skill edits that never
landed until `git status` showed the file still modified.

**Rule:** always `git add` the path exactly as `git ls-files` prints it, and
read `git status --short` before claiming a commit contains a file.

- 2026-09-01: Edited `.claude/skills/gflow/skill.md` twice before remembering
  the skill-sync-reminder skill must run whenever any skill file changes.
  Trigger it on EVERY skill-file edit, in the same turn as the edit.
