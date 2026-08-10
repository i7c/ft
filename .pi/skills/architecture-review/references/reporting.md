# Reporting Findings

How to present the review so it lands: ranked, evidenced, and actionable.

## Structure

```
## <Module/System> architecture review

### The map (1 table)
module | LOC | public items | imports | imported-by
(one row per top-level module; the table everyone refers back to)

### Findings (ranked by leverage)
1. **<One-line claim>** — evidence + why it matters + fix direction + cost
2. ...

### What's already good
The conventions and shapes to preserve: consistent plan/apply discipline,
genuinely deep modules, well-drawn seams. A review that only destroys
isn't actionable.

### Recommendations
Which finding to pursue first and why; what it unblocks.
```

## Evidence standards

Every finding carries one of:

- **file:line** — `vault.rs:356` is where the parse pipeline lives.
- **A count** — "232 call sites; mechanical sweep", "41 dead public items out of 380".
- **An exact use-level edge** — `vault → crate::graph::parser` and `graph → crate::vault::Vault`.

Drop any finding you can't back with one of these. A critique is only as good as its weakest claim — and the user will (rightly) probe the weakest one first.

## Quantification patterns that carry findings

- **Surface audit**: total public items, dead (no refs anywhere), internal-only (never imported by the external consumer), external. "~31% of the declared surface isn't really public" is a headline; "some dead code" is not.
- **Ripple counts**: `rg -c` call sites before proposing any signature change. 235 untouched (delegator kept) vs 232 mechanical (codemod) vs 40 hand-edited changes the plan.
- **Duplication counts**: three walkers, two task models, two mtime sources. Name each instance.
- **Ratio**: LOC per public item per module — the deep-vs-shallow table.

## Leverage ranking

Order findings by what unlocks the most, and say why:

1. **Structural fixes first** — breaking a cycle, extracting the pipeline contract, unifying scattered implementations. These are prerequisites for everything else: each later finding's fix gets cheaper and the dependency graph gets simpler.
2. **Mechanical clarity wins second** — surface discipline (pub → crate-private), dead-code deletion. Cheap, immediate, and they make the codebase honest about its contract.
3. **Perf/layering wins third** — killing redundant read passes, moving a shared predicate to the foundation layer.
4. **Model-level refactors last** — deduplicating a mirrored model, splitting a god object. Highest ripple; design forward rather than force them now (new fields should not require touching three representations).

Explicitly state the trade: "this is the costliest refactor on the list — flag it as such."

## Out-of-scope tails

For the change the user picks, list what is **deliberately out of scope** and — crucially — how each deferred item serves the original motivation:

> "The unifying thread: each out-of-scope item is the unfinished tail of one strand of the motivation. Cycle/linear-pipeline → drift promotion. One-read-pass → search + incremental refresh. Clean surface → model dedup. The sequencing logic: land the infrastructure first, then each follow-up becomes a bounded, mechanical change on top of it."

This turns the review into a roadmap instead of a wishlist: the user can see the whole arc and pick the next tail.

## "What's already good" — keep it specific

Name the actual shapes to preserve:

- The plan/apply mutation discipline that is consistent across every module.
- The genuinely deep modules (small surface, big implementation) that set the standard the rest should meet.
- The seams that are already well-drawn (clock injection, format interface, layered config).
- The single-read pipeline that just lives in the wrong file.

This is also the safety net: when the refactor lands, the user checks these still hold.
