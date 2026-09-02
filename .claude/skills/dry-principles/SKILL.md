---
name: dry-principles
description: "DRY (Don't Repeat Yourself) — knowledge duplication vs coincidental similarity, when duplication is correct, Wrong Abstraction anti-pattern, DAMP testing"
---

# DRY Principles

Authoritative reference for the DRY principle as applied in this project. Defines knowledge duplication vs coincidental similarity, when duplication is correct, the Wrong Abstraction lifecycle, and DAMP testing.

**Precedent lives in this repo, not in example files** — every claim below points
at real code. See also `architecture` and its `decisions.md`.

**Related principles:** `solid-principles` (SRP-DRY tension when actors differ), `kiss-principles` (counterforce to premature extraction — see `kiss-principles` for the knowledge-based extraction guards and timing).

## The DRY Principle — Correctly Defined

> "Every piece of **knowledge** must have a single, unambiguous, authoritative representation within a system." — *The Pragmatic Programmer*, Hunt & Thomas

DRY is about **knowledge**, not **code**. Two identical-looking code blocks are not necessarily a DRY violation. Two different-looking code blocks can be a DRY violation if they encode the same business rule.

The key question: **"If this business rule changes, how many places do I need to update?"** If the answer is more than one, you have a DRY violation — regardless of whether the code looks similar.

## Three Types of Similarity

### 1. Knowledge Duplication (Semantic — Extract)

The same business rule or decision encoded in multiple places. When the rule changes, all copies must change together.

**Heuristic:** These change for the **same reason** at the **same time**.

**Example (real):** `finish_release_fix` and `finish_hotfix_fix` each spelled out "if the PR already merged, clean up; otherwise open a `fix: <name>` PR against the parent". One rule, two copies — extracted into `finish_fix`.

### 2. Coincidental Similarity (Syntactic — Keep Separate)

Code that looks similar today but represents different business concepts. They will diverge as requirements evolve.

**Heuristic:** These change for **different reasons** or at **different times**.

**Example (real):** `hosting/github.rs` and `hosting/devops.rs` have the same *shape* — check for a PR, create one, report a merge. They share almost no knowledge: `gh` takes `--body-file` while `az` needs the file read; ADO PR URLs must be synthesized. The one genuinely shared rule, `resolve_body_file`, IS extracted; the rest stays apart on purpose.

### 3. Structural Similarity (Pattern — Document)

Repeated code structure that follows a convention or pattern (e.g., all Application Services have similar constructor injection). This is intentional consistency, not duplication.

**Heuristic:** These change when the **pattern itself** changes, not when individual rules change.

**Example (real):** every idempotent finish step in `flows/mod.rs` reads "check a real-state predicate, print `↷ skipped:`, else act". That is a convention, documented in `decisions.md`; the steps are not folded into one generic step.

### The Decision Heuristic

> **"Do these change for the same reason?"**

| Answer | Action |
|--------|--------|
| Yes, same business rule | Extract — this is knowledge duplication |
| No, different business concepts | Keep separate — this is coincidental similarity |
| Same pattern/convention | Document the pattern, don't abstract the instances |

## When Duplication Is Correct

Duplication is not always wrong. In these cases, the coupling cost of sharing exceeds the duplication cost:

### Across the Port Boundary

A concept represented on both sides of a port is not duplication. `BranchType` (parsed knowledge), `Action` (what to do), and `FinishState` (what survives a crash) all describe "a release finish" and are correctly three types with three lifetimes.

### Between the Two Interfaces

`cli::resolve_action` and `menu::show_menu` both encode the branch-type gating table — the CLI's reject side, the menu's offer side. Deriving one from the other would need a mode parameter and would flatten the deliberately different wording (decisions.md: the menu adds "Switch to main or develop first", the CLI stays terse). This duplication is accepted and paid for with a **parity test** instead.

### When Coupling Cost Exceeds Duplication Cost

Small-scale duplication within a bounded context is sometimes preferable to a shared abstraction that creates coupling between unrelated features. See the `kiss-principles` skill for extraction timing (knowledge-based, with wrong-abstraction guards) and the KISS-DRY decision table.

### Naming the Concept (Before Extracting)

If you can't name the shared concept better than "SharedHelper", "CommonUtils", or "BaseProcessor", you haven't found the abstraction yet. A good extraction has a name that describes the **knowledge** being shared, not the **code structure**.

## The Wrong Abstraction

> "Duplication is far cheaper than the wrong abstraction." — Sandi Metz

### The Lifecycle of Abstraction Decay

1. **Two similar cases appear.** Developer extracts shared code. Feels good.
2. **Third case is slightly different.** Add a boolean parameter. Manageable.
3. **Fourth case needs another variation.** Add another parameter. Getting complex.
4. **Fifth case is an edge case.** Add a conditional branch. Now the shared code is harder to understand than the duplication was.
5. **Nobody dares touch it.** The abstraction has become load-bearing — everyone depends on it, nobody understands it.

### Red Flags of a Wrong Abstraction

| Signal | What It Means |
|--------|---------------|
| Boolean parameters controlling behavior | The abstraction serves multiple concepts |
| Growing conditional chains inside shared code | Cases are diverging, not converging |
| Callers passing `null` or empty values for unused parameters | Interface is too broad for some callers |
| "I need to understand all callers to change this" | Coupling exceeds the value of sharing |
| Shared base class where subclasses override most methods | Inheritance serving code reuse, not "is-a" |
| Comments like "// only used by X" inside shared code | The sharing is no longer symmetric |

### The Fix: Inline and Re-Extract

1. **Inline** the shared code back into each caller
2. **Accept the temporary duplication** — this is healthy intermediate state
3. **Let the natural groupings emerge** — with all code visible, the real abstractions become clear
4. **Re-extract** only the genuinely shared knowledge (if any exists)

Do NOT try to "fix" a wrong abstraction by adding more parameters or conditionals.

## DRY Violations to Watch For

### Shotgun Surgery
A single business rule change requires modifying multiple files. The knowledge is scattered.

### Business Rules in Multiple Places
The same validation, calculation, or business decision implemented in more than one location.

### Magic Strings
A branch name, config key, or remedy sentence written out in more than one place. `gflow.branch.main` is a `const`; the `gh auth login` remedy is a `const`; branch names come from `SemVer` methods, never `format!`.

### Copy-Paste with Minor Variations
Nearly identical blocks of code where the differences are incidental, not intentional.

### Parallel Hierarchies
Two class hierarchies that mirror each other and must be updated in lockstep.

## DRY vs SRP Tension

When DRY and SRP conflict, **SRP wins when actors differ**.

The same logic serving different stakeholders is coincidental similarity, not knowledge duplication. Even if the code is identical today, different stakeholders will drive divergence.

**Example (real):** `finish_release` and `finish_hotfix` share shape but answer to different rules — only the release path carries the RC gate, only the hotfix path propagates into open releases. Unifying them would need mode flags, which is the wrong-abstraction red flag.

Cross-reference: `solid-principles` skill, SRP section.

## DRY vs Loose Coupling

| Scope | Action | Rationale |
|-------|--------|-----------|
| Within a class | Extract method | Zero coupling cost |
| Within a module/feature | Extract to shared class in same module | Low coupling cost |
| Across flows in `flows/` | Shared helper in `flows/mod.rs` | Low cost — worth it for a real shared rule |
| Between the two interfaces | **Keep duplicated, add a parity test** | Unifying needs mode params; a test is cheaper |
| Between two hosting adapters | **Keep duplicated** | Provider knowledge diverges; only cross-provider rules move to `hosting/mod.rs` |
| Between production code and a mock | **Never** — fix the mock | Production code compensating for a mock is the relationship inverted |

## DAMP in Tests

> DAMP: Descriptive And Meaningful Phrases

In test code, **DRY the "how" (infrastructure), allow duplication in the "what" (scenarios)**.

### What to DRY in Tests
- Test infrastructure: builders, factories, fixtures, setup helpers
- Assertion helpers: custom matchers for domain-specific checks
- Mock configuration: shared mock setups for common dependencies

### What to Allow Duplication In
- Test scenarios: each test should tell a complete story
- Arrange sections: explicit setup makes preconditions visible
- Assert sections: explicit assertions make expected outcome visible

Test code optimizes for **readability at the individual test level**, not for minimizing total lines.


## DRY Code Review Checklist

| # | Check | Severity |
|---|-------|----------|
| 1 | Same business rule in multiple places | High |
| 2 | Magic numbers without named constants | Medium |
| 3 | Copy-paste with minor variations | Medium |
| 4 | Growing conditionals in shared code | High |
| 5 | Boolean parameters on shared methods | Medium |
| 6 | The same gating rule encoded in both `cli.rs` and `menu.rs` with no parity test | High |
| 7 | A branch or tag name built with `format!` instead of a `SemVer` method | High |

## Documentation Convention

**`// DRY:` — applying the principle** (explaining why duplication is kept):
```
// DRY: coincidental similarity — gh and az diverge on body-file handling
// DRY: the offer/reject sides differ in wording on purpose; parity is test-enforced
```

**`// DRY-DEVIATION:` — knowingly violating** (duplicating knowledge):
```
// DRY-DEVIATION: re-sorted here despite the trait contract — replay determinism
// is a crash-safety invariant the flow enforces rather than trusts
```
