---
name: architecture-review
description: Analyze a software system's architecture — module structure, interfaces, coupling, and design quality — and turn findings into concrete, prioritized improvements. Use whenever the user asks to understand how a codebase is organized, critique module boundaries or interfaces, find design problems, improve separation of concerns, or prepare for a refactor — phrases like "how is this structured", "what's the architecture", "this module does too much", "let's dive into the core", or "what would you improve here?". Works for any system in any programming language; the method is tooling-agnostic (ripgrep, git, language doc tooling). Make sure to use this skill even when the request sounds vague — "get a feel for the codebase", "critique this", "understand the design" are all review requests.
---

# Architecture Review

A repeatable method for understanding a codebase, finding architectural problems, and turning them into improvements. Distilled from a session that produced two completed refactors (breaking a module cycle by extracting the shared pipeline; unifying scattered consumers onto one read pass) — the loop is what made those changes land safely.

The loop: **map → measure → critique → rank → confirm → propose → implement → verify → close**.

## Phase 0 — Scope

Decide what is in scope: the whole system, one package/crate, one module cluster? Note the build system (Cargo.toml, package.json, go.mod, pyproject.toml, …) and language(s). State the scope back to the user before diving in — a review that covers the wrong surface wastes the session.

## Phase 1 — Inventory

Build the module map before reading any logic. The map is the shared reference for every later phase; without it, findings float.

- Module declarations: the entry file (main/lib), module roots (`pub mod`/`module`/`go.mod`/`__init__.py`/import roots), directory tree.
- Size: LOC per file/module, sorted. `scripts/inventory.sh` gives a universal starting point; adapt to the language.
- Output: a table of modules with sizes. Note which modules are the largest — they are candidates for the "too much responsibility" critique later.

## Phase 2 — Interface extraction

For each module, extract the public surface: exported functions, types, traits/interfaces/classes. The surface is the module's *contract* — its size and shape are the primary quality signal. A deep module (big implementation, small surface) is the ideal; a flat grab-bag (many exports, no internal structure) is the smell. See `references/tooling.md` for language-agnostic extraction recipes.

## Phase 3 — Dependency mapping

Map module-level dependencies from imports. For each top-level module, list the modules it imports and which import it. Then classify:

- **Hubs** — imported by many; changes here ripple. Check whether the hub earns its centrality.
- **Leaves** — import nothing; foundation material.
- **Cycles** — A imports B and B imports A at the use level. This is the #1 smell to chase: find what *artifact placement* created the cycle (often one module holds the pipeline contract another module needs). Cycles that are acyclic at runtime (scan → build) are still worth breaking for clarity.

## Phase 4 — Core-model reading

Read the central data structures and the seams: traits/interfaces, config objects, environment overrides, format plug-in points. Most deep problems — god objects, duplicated models, hard-coded seams — live here. Also read the hot paths: the operations that run on every keystroke or every invocation.

## Phase 5 — Critique

Apply the analysis-dimensions catalog in `references/analysis-dimensions.md` (cohesion, coupling, cycles, surface discipline, deep-vs-shallow, god objects, error policy, seams, naming, dead capabilities, I/O hygiene, ripple cost). For every suspected problem, verify with evidence before listing it — a critique is only as good as its weakest claim.

## Phase 6 — Quantify

Findings without numbers are vibes. Measure what matters:

- Dead exports: public symbols referenced by nothing outside their module (write a quick script; the session that spawned this skill found 41 dead + 75 internal-only out of 380 — that number carried the finding).
- Call-site counts for any proposed signature change ("232 call sites — mechanical, codemoddable" vs "hand-edited everywhere" changes the plan).
- Duplicate implementations: e.g., three walkers with divergent semantics is a finding; a naming nit is not.
- Duplicated models: count the fields that must stay in sync across two representations.

## Phase 7 — Report

Present findings ranked by leverage, each with file:line evidence and the fix direction. Follow `references/reporting.md`. Include a "what's already good" section — a critique that only destroys isn't actionable, and the user needs to know what to preserve. End by asking which finding to pursue.

## Phase 8 — Confirm the design before writing

When the user picks a finding, restate the target design in one paragraph — "if I'm reading this right, the result is a module that does X, consumed read-only by Y and Z" — and let them correct it before any spec or code. A wrong direction written down costs far more than a conversation. As you discuss, record decisions *and rejected alternatives*; they become the spec's spine.

## Phase 9 — Propose

If the repo has an openspec/ADR workflow, use it — and commit the spec as its own commit before implementing. Otherwise write a change document with the same spine:

- **Why** — the problem in one or two sentences, tied to the review finding.
- **What changes** — bullets; mark breaking API changes explicitly; name the seams that already exist for the change.
- **Impact** — files touched, signature changes, test ripple, what is explicitly **out of scope** and how each deferred item serves the original motivation.
- **Design** — decisions with alternatives considered and rejected; risks with mitigations; migration order.
- **Tasks** — numbered sections, each leaving the repo's gates green (build, test, lint, format), with tests and expected results per task.

## Phase 10 — Implement in green sections

Order tasks so every numbered section compiles and passes the repo's invariants. Mechanical sweeps (call-site updates, renames, import moves) are codemodded, not hand-edited. Before a wide signature change, count call sites and prefer a compat helper or struct-params when the ripple is huge. Keep behavior identical where the change is structural — the tests are the proof.

## Phase 11 — Verify with audits

Beyond the gates, run audit greps that prove the *reason* for the change:

- The cycle is gone: no module A imports B at the use level.
- The coupling is gone: the low-level module no longer imports the feature module.
- The read path is single-pass: the full-read function is the only `read` of its kind.
- The dead surface is deleted: `rg` for the removed names returns nothing.

Also add tests that encode the *semantics* you changed — staleness windows, fallback paths, equivalence between old and new implementations. In the session this skill came from, a staleness test caught a real bug (an `Option<String>` conflating "no frontmatter" with "file absent") that the design had missed.

## Phase 12 — Close

Archive the change (move it out of active), sync any spec deltas into the capability specs, and record the commit shas (spec commit, implementation commit, archive commit) so the history reads as a chain.

## Parallel deep dives

When the system is large or a phase benefits from isolated context, delegate per-module-cluster analysis to subagents. The bundled instruction in `agents/module-reviewer.md` produces the structured per-module report (responsibilities with line ranges, surface, deps in/out, cohesion assessment, call sites) that the main session merges into the map. Register the agent per your harness's convention (pi: `~/.pi/agent/agents/` user scope or `.pi/agents/` project scope; other harnesses have their own agents directory) and invoke it by name. If no agents are registered, do the deep dives inline — the method does not depend on subagents.

## Guardrails

- **Code is ground truth.** Read code and measure; do not recite the project's own docs as analysis — docs describe intent, and the session that spawned this skill deliberately ignored docs to avoid reciting someone's self-image.
- **Evidence before claims.** Every finding needs file:line or a count. Unverifiable findings get dropped.
- **Name what to preserve.** Every critique ends with what's already good and must not be lost in the refactor.
- **Don't fix while analyzing.** The review produces findings, not patches. Implementation starts only after the user selects a finding and confirms the direction.
- **Rank by leverage, not annoyance.** Breaking a cycle or unifying a scattered pipeline outranks a naming nit — say so, and say why.
- **Respect the harness's change workflow.** If the repo has openspec (propose → apply → archive), use it; commit the spec separately from the implementation.

## References

- `references/analysis-dimensions.md` — the critique catalog: 12 dimensions with signals and example evidence.
- `references/reporting.md` — the findings-report format, evidence standards, and leverage ranking.
- `references/tooling.md` — language-agnostic data-gathering recipes: inventory, surface extraction, dependency mapping, dead-API measurement.
- `agents/module-reviewer.md` — subagent instruction for per-module deep dives.
- `scripts/inventory.sh` — universal module inventory (tree + LOC + symbol counts).
