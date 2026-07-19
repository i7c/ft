# ft — Premise Review

*2026-07-19*

> **Scope.** Unlike the prior reviews
> ([2026-06-02](2026-06-02-architecture-review.md),
> [2026-06-12](2026-06-12-refactor-review.md),
> [2026-07-02](architecture-review-2026-07-02.md)), this one does not
> assume the tool's purpose is fixed and ask "is it built well?" It
> questions the *what* and the *why*: which premises are load-bearing,
> which are quietly load-bearing for the *story* but not for the *use*,
> which features don't earn their place, and where a different bet would
> reach the same goal with less machinery.
>
> Grounded in: the README, `docs/guide/philosophy.md`,
> `docs/guide/synthesis.md`, the three prior reviews, the openspec
> change archive, and a read of the actual graph/task/timeblock
> integration in `ft-core`. Conclusions the user confirmed before
> writing: single power-user author; git-as-memory is settled and
> correct; ghosts are load-bearing in practice; synth is genuinely
> used; tasks belong because they appear *during* note-taking;
> timeblocks may be the odd one out; drift's *idea* is right, its
> *implementation* is noisy.

## 0. The thesis, as I now understand it

Strip everything else and `ft`'s claim is:

> **A note is valuable to the degree you can retrieve it later, and
> retrieval should not depend on decisions made at capture. So at
> capture, record only the one cheap thing you always know — *what is
> this about* — as a `[[wikilink]]`. Defer everything else. The
> wikilink, plus git history, is enough to make the pile queryable,
> rankable, and re-gatherable on demand, forever.**

That thesis is coherent, novel, and — once you accept the git
dependency — cheaply true. The architecture reviews already established
that the implementation honours it well. This review accepts the thesis
as given and asks: does the *whole tool* serve it, or has the tool
acquired parts that the thesis does not justify?

My one-sentence verdict up front: **the note-flow core is a genuinely
original bet, well-executed; the tasks subsystem has a real thesis but
is wired in shallowly enough that its thesis is not actually reachable
from the query surface; the timeblocks subsystem has no thesis at all
beyond "I am in the terminal anyway"; and the TUI has grown large
enough relative to the core that it now shapes feature direction more
than the core does.** The specifics follow.

---

## 1. The thesis is real and the core honours it

Before the critiques, the load-bearing parts are worth naming plainly,
because a premise review that only complains has failed to locate the
premise.

- **Ghosts as first-class citizens.** `NodeKind::Ghost`, a shared
  `ghost_index`, `incoming(ghost)` working like `incoming(note)`,
  pulse/gather counting ghosts on equal footing with notes — this is
  the single most original idea in the tool, and the user confirms it
  is load-bearing in practice (many concepts accumulate weight in the
  vault without ever getting a note). Obsidian does not do this.
  This alone justifies the tool's existence for its one user.
- **Paragraph as the retrieval unit.** `extract_paragraphs` +
  `ParagraphLink` edges mean a daily note of five unrelated thoughts
  is five independently retrievable units, each carrying its own
  concepts. This is the right granularity for the thesis, and it is
  what makes gather/pulse/synth actually useful rather than
  note-level backlinks in a new dress.
- **Git as the temporal memory.** Confirmed as settled. Reusing git
  for "when was this paragraph authored" is the right call: it is
  free, it is already there for the power user, and re-implementing a
  temporal index would be a tax with no payoff for the target
  audience. The synth-verify/repair machinery is the honest price of
  pinning excerpts to SHAs, and it is paid in the right place
  (behind a command the user runs deliberately, not on every call).
- **Synthesis as compression, not maintenance.** Pulse → gather →
  scaffold, with the citation-state badges (`cited` / `cited*` /
  uncited-only) so the session is incremental — this is the part the
  user says is most useful, and it is the part that turns "deferred
  filing" from a promise into a concrete workflow. The
  `ft.synth.targets` frontmatter + context-note mode in the Gather
  tab is exactly the right shape: "here's what this note already
  holds, here's what's still missing." That is a feature that could
  not exist without the paragraph graph, which is the right kind of
  feature to build on a thesis.

None of this is in question. The rest of the review is about the
parts that are *not* the thesis.

---

## 2. Tasks have a thesis — but it isn't reachable from the query surface

This is my sharpest finding, and it survived verification, so I'll
state it precisely.

The user's argument for tasks-in-`ft` (and it's a good one) is:

> Tasks appear *during* note-taking. A conversation generates notes,
> thoughts, *and* TODOs. So the task manager belongs in the same vault
> and the same surface, because the act that produces a task is the
> same act that produces a note.

That is a real thesis, and it implies a concrete, testable
consequence: **a task should be queryable by the concept it arose
from.** "Show me everything about `[[onboarding]]`" should include the
task `- [ ] chase Priya re: onboarding metrics 📅 2026-06-15` that was
written in the same paragraph, because that task *is* part of what
that conversation produced.

Here is what the code actually does:

- A task lives in a note. `extract_paragraphs` does **not** skip task
  lines, so a `- [ ] task [[concept]]` line is absorbed into the
  surrounding `Paragraph` node, and that paragraph node *does* get
  `ParagraphLink` edges to `[[concept]]`. So at the paragraph level,
  the thesis holds: the note-flow graph sees the task's concept
  context.
- But `ft tasks list` runs the unified DSL under `Profile::Tasks`,
  which synthesizes `node where kind = Task and …` and walks
  **`NodeKind::Task` nodes directly**. The `Task` node has only
  `HasTask` (up to its note) and `Subtask` edges; it has **no
  outgoing edge to the concepts mentioned in its paragraph**. The
  query `Attr` set covers `Status`, `Priority`, `Due`, `Scheduled`,
  `Tags`, `Description`, `Kind`, `Path`, `Indegree`, `Outdegree` —
  there is no `mentions` / `co-occurs-with` / `about` predicate.

The consequence: **the one query the task thesis predicts is
impossible to write.** You cannot ask `ft tasks list` for "tasks
about `[[onboarding]]`," nor "tasks that came up in the same
paragraph as `[[analytics migration]]`," nor "overdue tasks whose
concept has a ghost." The integration that would make tasks belong to
the note-flow thesis — task-to-concept edges, or a task profile that
falls back to paragraph membership — is exactly the integration that
is missing. What's there instead is a perfectly competent Tasks-plugin
clone that happens to live in the same binary as a note tool.

So the current state is:

- *Storage* integration: real (same vault, same files, same scan pass).
- *Query DSL* integration: superficial (the parser is shared, but the
  task profile can only see task fields, never the graph it lives in).
- *Thesis* integration: absent. The argument "tasks belong because
  they arise during note-taking" is true at the *file* level and
  false at the *query* level.

This is the gap I'd put first on the list, because it is the gap
between the *argument for the feature* and the *feature as built*.
Two ways to close it, in increasing cost:

1. **Cheap, partial:** add an `Attr::Mentions` (or `about`) to the
   task profile that resolves a task's concept set by looking up its
   owning paragraph and walking `ParagraphLink` edges. This makes
   `ft tasks list --query 'about = [[onboarding]] and due < today'`
   work. It costs one eval arm + a paragraph lookup; it does not
   change the graph.
2. **Right, structural:** give `Task` nodes an outgoing concept edge
   at graph-build time (a `TaskMentions` edge, or reuse
   `ParagraphLink` from the task to its concepts). Then "tasks about
   X" and "notes about X" are the same graph query with a `kind`
   filter, and the task manager stops being a parallel system that
   happens to share a parser.

Either way, the point stands: **today, the task subsystem's reason
for existing is not wired into the task subsystem's query surface.**
If you accept the user's argument (and I do), that's the gap worth
closing before more task features land — because every new task
feature inherits the same shallowness until the concept edge exists.

## 3. Timeblocks have no thesis — and the user half-agrees

The user said: *"time blocks might be the odd one out … There is no
connection between the notes flow and timeblocks. Neither do tasks
have anything to do with it. One could consider splitting that part
out and giving blockary a proper TUI."*

I agree, and I'd push it further: timeblocks don't just lack a
connection to the note-flow thesis — they lack a *thesis of their
own* that justifies living in `ft` rather than in a standalone tool.
The note-flow thesis is "capture can't wait, filing can't be
predicted, the link makes it retrievable." What is the timeblock
thesis? "Day-Planner blocks, in the terminal." That's a feature
description, not a premise. There is no bet about *why* timeblocks
behave the way they do, no claim about what they get right that the
Day Planner plugin or `blockary` gets wrong, no principle that
constrains what a future timeblock feature should or shouldn't do.

The cost is concrete:

- ~2k LOC in `ft-core/src/timeblock/` (`doc.rs`, `ops.rs`, `mod.rs`,
  `report.rs`) + ~1.6k LOC in the TUI tab + ~1k LOC in the CLI +
  ~840 LOC of tests. Roughly **5–6k LOC** — about 6% of the whole
  codebase — sits on a subsystem with no thesis connecting it to the
  tool's reason for existing.
- The TUI tab is opt-in and off by default, which is the honest
  signal: the tool itself knows timeblocks aren't part of the core.
  But opt-in tabs still pay maintenance, still claim a keymap region,
  still appear in `ft commands list`, still need their snapshots
  refreshed on every visual pass (the active `refresh-tui-visuals`
  openspec change will touch them).
- Every openspec change that touches "all tabs" or "all output
  formats" carries timeblocks along for free in the diff and pays
  for them in review.

The user's own framing — "give `blockary` a proper TUI" — is, I
think, the right resolution, and I'd raise it from "one could
consider" to "this is the cleanest cut available." Two honest options:

- **Split it out.** `blockary` already exists as the format's
  reference implementation; a standalone TUI over it would be a
  sibling tool, not a sub-feature. `ft`'s vault-format compatibility
  means the two tools share files just as `ft` and Obsidian do. The
  note-flow thesis stops being diluted, and timeblocks get a tool
  whose entire surface is about them.
- **If it stays, give it a thesis.** Right now "timeblocks in `ft`"
  has no answer to "why not the Day Planner plugin?" The answer
  could be something real — e.g. "timeblocks should be derivable
  from the day's tasks and concepts, so a block is a *projection* of
  the note-flow graph, not a separate authored list" — but that
  would be a feature to build, not a premise that's already there. If
  nobody's going to build it, the honest move is the split.

Either is defensible; "status quo, off by default, ~6% of the
codebase, no thesis" is the one option I'd argue against.

## 4. The TUI now shapes the tool more than the thesis does

This is a premise-level observation, not an architecture one (the
architecture reviews already covered the TUI's internal quality). The
point here is about *direction of influence*.

Look at the active openspec changes at the time of writing:

```
color-graph-nodes          improve-graph-node-display
compact-task-rows         journal-sources-manager
configurable-keymaps      navigate-periodic-notes-in-graph
feed-split-layout         refresh-tui-visuals
graph-task-edit-modal     shared-graph-snapshot
graph-task-interaction    support-ft-vault-marker
hierarchical-ft-frontmatter  tasks-preset-pick
improve-graph-node-display  text-input-ux
                           tui-graph-crud-operations
                           tui-help-overlay-scrolling
                           unify-query-dsls
```

Of 20 active changes, **13 are TUI/visual or TUI-interaction work.**
The archived set tells the same story over a longer arc: the
majority of recent capabilities are TUI tabs and graph-visualization
features. The thesis — "capture can't wait, filing can't be
predicted, the link makes it retrievable" — is a claim about
*retrieval and consolidation*, which is largely a CLI-shaped promise
(gather, pulse, synth, rename are all one-shot operations over the
graph). The TUI is a *live surface* over that promise, and it has
grown to ~55k of the ~90k LOC.

The premise-level risk: **when the TUI is the largest single
subsystem, feature direction starts being driven by "what's missing
in the TUI" rather than "what's missing in the thesis."** That is
how a tool whose stated differentiator is "runs where a GUI can't,
in cron, in pipelines" ends up spending most of its active
development on GUI concerns: node coloring, row compaction, help
overlay scrolling, feed split layout. None of those are *wrong*;
each is reasonable in isolation. But in aggregate they are a signal
that the TUI's gravity is now stronger than the thesis's.

Two things would make this observable and manageable rather than just
a vibe:

- **State the TUI's scope explicitly, the way the thesis states the
  note-flow's.** Today the docs say the TUI is "the live surface"
  and "the right tool for 'I want to see what's happening, then
  decide.'" That's a description, not a constraint. A scope
  statement like "the TUI exists to make the *interactive* parts of
  the thesis — gather/synth sessions, ghost promotion, drift triage
  — low-friction; it does not acquire features the CLI cannot
  express" would give future changes a yes/no test. Right now there
  is no such test, so the TUI accretes.
- **Watch the CLI/TUI feature parity.** A useful leading indicator:
  is there a thing the TUI can do that the CLI cannot, and is that
  thing part of the thesis? Today most flows exist on both surfaces
  (good). The drift is mostly in *visualization* — and that's
  arguably fine, since visualization is inherently a TUI concern.
  But if a future change adds, say, a TUI-only synth editing flow
  with no CLI equivalent, that's the thesis drifting.

The one concrete recommendation here: **the `refresh-tui-visuals`
openspec change is going to touch a large fraction of the TUI
snapshots.** Use that as the moment to ask, per tab, "does this tab
serve the thesis, or does it serve the TUI's own gravity?" If the
answer is the latter for any tab, that's the candidate for the
timeblock treatment — split it out, or give it a thesis.

## 5. Drift: the right idea, defeated by a default that excludes nothing

The user said: *"drift detection has not been as useful as I hoped
mostly because there is a lot of noise (image links, numbered files
etc). But that doesn't invalidate the idea, it invalidates the
implementation."* I agree on both counts, and I can point at the
specific implementation fault.

`drift.exclude` is a `Vec<String>` of glob patterns, and its default
is **`Vec::new()`** — empty. Every ghost in the vault is a drift
candidate unless the user has manually written `[drift] exclude =
[…]` into their config. So on a fresh vault, `ft notes drift` fires
on:

- `[[diagram-v1.png]]` vs `[[diagram-v2.png]]` (image attachments)
- `[[2026-06-12]]` vs `[[2026-06-13]]` (daily-note numbered files)
- `[[fig-1]]` vs `[[fig-2]]` (sequenced figures)
- every other systematically-similar-name pair that isn't a concept

The implementation *has* a glob exclude mechanism and tests for the
`*.png`/`*.pdf` case — so the feature's authors knew this category
of noise existed. What it doesn't have is a **default exclude set**.
The result is exactly what the user reports: the signal is drowned
by noise that the tool already knows how to classify.

This is the kind of fault that's invisible to the thesis (the thesis
is sound) and invisible to the architecture (the architecture is
fine) but *fatal to the feature in practice* — because the first
time a user runs `ft notes drift` and gets thirty image-pair
suggestions, they conclude "drift doesn't work" and stop running it.
The user did exactly this.

The fix is small and thesis-aligned:

- **Ship a sensible default exclude set.** Patterns like `*.png`,
  `*.jpg`, `*.jpeg`, `*.gif`, `*.webp`, `*.svg`, `*.pdf`, and a
  rule for "purely numeric / date-shaped targets" (daily notes)
  would remove the bulk of the noise without the user having to
  discover the config key by being annoyed first.
- **Consider whether non-concept targets should be drift candidates
  at all.** A ghost whose target string parses as a date, or ends in
  a known attachment extension, is almost never a *concept* that
  drifted — it's a different *kind* of link. The graph already
  distinguishes `![[embeds]]` from `[[links]]`; the distinction
  "is this target plausibly a concept name" is one regex away. If
  drift only ever ran against concept-shaped targets, the noise
  category the user hit would not exist as a category.
- **Per-pair signal, not just per-pattern.** A `[[onboarding]]` /
  `[[onboarding-flow]]` pair that *also* shares co-occurrence
  neighbors is the real signal; a `[[fig-1]]` / `[[fig-2]]` pair
  that never co-occurs with anything is noise even if it passes the
  name gate. The neighborhood-overlap signal already exists in the
  scorer — raising its weight (or making it a hard gate) would let
  the feature ship with a tighter default.

The point for this review: **the feature's failure mode is not
premise-level, it's defaults-level**, and a premise-level review
would be wrong to call drift a "clever feature nobody asked for."
The user asked for it, the idea is right, and the fix is in the
defaults, not the design.

## 6. The "exactly two things" framing has already quietly broken

The README says: *"On top of the note-flow core, `ft` adds exactly two
things — task management and time management — and nothing else."*

This is a load-bearing sentence in the docs. It is the promise that
the tool's scope is bounded. And it has already quietly broken in
two ways:

1. **Synthesis is a fourth thing, and it's the biggest one.** Pulse,
   gather, synth scaffold/verify/repair/reslice/accrete, the
   `[!ft-source]` callout grammar, citation-state badges, the
   context-note mode, the Synthesis tab — this is not "task
   management" or "time management," and it's not strictly
   "note-flow core" either; it's a *consolidation layer* that sits
   on top of the core. The docs acknowledge this by giving it its
   own chapter and its own config table, but the "exactly two
   things" sentence in the README still reads as if synthesis is a
   minor feature. It is not — it is the thesis's payoff, it is the
   user's most-used flow, and it is where the git dependency
   becomes load-bearing. The framing undersells it.
2. **The TUI is a fifth thing, and it's the largest by LOC.** See
   §4. The "exactly two things" sentence scopes the *capabilities*
   but ignores the *surface*, which is where most of the code and
   most of the active development lives.

This matters at the premise level because **scope promises that the
code has already outgrown become anti-signals**: a contributor (or
the solo author, in six months) reading "exactly two things" will
treat a third or fourth capability as a violation of the design,
when the design has in fact already accepted four. The honest rewrite
is something like:

> `ft`'s core is the note-flow: capture with `[[wikilinks]]`,
> retrieve via the paragraph graph, consolidate via synthesis. On
> top of that it carries task management (because tasks arise during
> note-taking) and an opt-in timeblock layer. The CLI is the
> headless surface; the TUI is the live surface over the same core.

That sentence admits four things and names the relationship between
them, which is what the code already reflects. The current "exactly
two things" sentence is a museum label on a room that's been
remodelled.

## 7. Onboarding is a real gap — for the stated audience

The user said: *"I am the sole user but I publish my work in case
any other power user wants to use this. We could question how
polished the tool is at this point (setting it up and learning how
to use it is probably not straight forward)."*

Confirmed by the docs. The setup path is:

1. `cargo install --path ft` (build from source — no releases, no
   binaries, no `cargo install ft` from crates.io).
2. Generate shell completions and man pages *manually* (separate
   commands, documented but not run for you).
3. Discover or configure a vault (`.obsidian/` or `.ft/` walk-up,
   `--vault`, `FT_VAULT`, or `default_vault` — four mechanisms, all
   documented, none surfaced on first run if none match).
4. Learn the command surface: 10 top-level commands, `ft notes`
   alone has 13 subcommands, plus a unified query DSL with two
   profiles and a preset system.
5. Learn the TUI: 5 default tabs + 2 opt-in, a command/keymap
   registry, modal flows, chord leaders.
6. Learn synthesis: pulse → gather → scaffold → (write prose) →
   verify, plus repair/reslice for when history moves.

Every one of those steps is *individually* well-documented. The
problem is that there is no **single guided path through them**, and
the conceptual model the tool asks you to adopt (write anywhere,
file never, link everything, git is memory, ghosts are real,
synthesis is compression) is not the model any other note tool asks
for. A power user who arrives from Obsidian will try to use `ft`
like Obsidian-in-the-terminal and bounce off, because the thesis is
genuinely different: in Obsidian the graph is a visualization; in
`ft` the graph is the substrate the operations run over, and the
*point* is that you stop filing.

For the sole-user case this is irrelevant — the author already has
the mental model. For the "any other power user" case it is the
binding constraint, and it is a *premise* problem, not a docs
problem: **the tool's value proposition is non-obvious from its
command surface, and no amount of `--help` polish will fix that.**
Two things that would, and neither is "write more docs":

- **A `ft intro` or `ft onboarding` command** that walks a fresh
  vault through the thesis in five commands: create a daily note,
  write a paragraph with a `[[concept]]`, run `ft notes pulse`,
  gather on that concept, scaffold a synth note. The thesis is
  *demonstrable in five commands*; today those five commands are
  scattered across the guide and the user has to assemble the
  sequence themselves. A single command that runs them in order,
  printing what it's doing and why, would convey the premise in
  ninety seconds — which is exactly the README's "ninety seconds of
  it" pitch, except real.
- **A `ft doctor` / `ft vault --check`** that surfaces, in one
  place: is this a git repo, is there a remote, is there a daily
  note configured, are the opt-in tabs on, what's the commit cadence
  (from `git log` timestamps) — i.e. the things that determine
  whether the tool's bets will pay off for *this* vault. Today each
  of those is a separate command or a config-file edit; for a new
  user there's no way to ask "is my vault set up for `ft` to work?"

These are small, thesis-aligned additions. The point is that the
onboarding gap is not "the docs are bad" (they aren't) — it's that
the tool asks for a mental model shift and provides no single
command that *performs* the shift.

## 8. Smaller premise-level observations

- **`ft find` vs `ft notes open` is still a real split.** The
  2026-06-02 review flagged this (Critique 3) and it's still true:
  `ft find` prints, `ft notes open` opens, same fuzzy syntax. A
  premise-level read: the tool's thesis is that *the link* is the
  unit of retrieval, not the filename — so a fuzzy-finder over
  filenames is arguably the *wrong* primary retrieval primitive,
  and `ft notes gather --link` is the right one. The fact that
  `ft find` exists and is prominent suggests the filename-finder
  reflex is still being catered to alongside the thesis. Minor, but
  worth a thought: is `ft find` serving the thesis or serving the
  Obsidian-reflex?
- **The unified DSL is a real win, but the two-profile design
  papers over the tasks gap from §2.** `Profile::Tasks` prepends
  `node where kind = Task and …` so users can type
  `priority = High`. This is convenient, but it also means the task
  query surface *looks* unified with the graph query surface while
  being unable to express graph-shaped questions (about/concept).
  The unification is syntactic, not semantic. If the §2 fix lands,
  the profiles converge for real; until then, the "unified" framing
  slightly oversells what task queries can do.
- **The `hierarchical-ft-frontmatter` active change is a good
  premise-level signal.** The flat `ft-synth:` / `ft-tasks-section:`
  keys were a format wart; consolidating under one `ft:` map is the
  right direction and shows the tool is willing to break
  compatibility to fix a format-level mistake. Worth noting as a
  positive: the "no backwards-compat shims" principle (philosophy
  doc) is being honoured, and it's the right call for a tool with
  one user and a small public surface.
- **No LLM/embedding story, and that's a defensible omission — for
  now.** The philosophy doc explicitly argues against embeddings:
  "aboutness beats string matching," "the operations need identity
  not similarity." That argument is correct *for the operations
  that exist today* (count, rank, rewrite all need exact identity).
  But the synthesis step — "turn gathered excerpts into a focused
  note" — is the one place an LLM would materially help, and the
  tool currently punts it to `$EDITOR` + the user writing prose.
  This is not a fault; it's a deferral. The premise-level question
  is whether synthesis *without* an LLM is thesis-complete, or
  whether "compression" inherently wants a summarizer. I lean
  toward "thesis-complete without LLM, but the seam should be
  named": a future `ft notes synth draft` that fills prose between
  callouts from an LLM is a natural extension, and the callout
  grammar is already the right boundary for it (provenance stays
  plain text and verifiable; only the connective prose is
  generated). Worth not foreclosing.

---

## 9. Priorities, premise-level

Ranked by how much each would sharpen the tool's reason for
existing, not by effort:

1. **Close the task-to-concept query gap (§2).** This is the
   single highest-leverage premise fix: it makes the task
   subsystem's stated reason for existing actually reachable from
   its query surface. Without it, "tasks belong because they arise
   during note-taking" is an argument the code doesn't honour.
2. **Decide the timeblock question (§3).** Either split it to
   `blockary`-with-a-TUI, or give it a real thesis that connects
   it to the note-flow. The status quo — off by default, ~6% of
   the codebase, no thesis — is the option that should not
   survive.
3. **Ship default drift excludes (§5).** Smallest change on this
   list, biggest "feature that currently doesn't work in practice
   but should" payoff. The idea is right; the defaults are wrong.
4. **Rewrite the "exactly two things" framing (§6).** A doc change,
   but a premise one: the docs should admit four things and name
   their relationships, because the code already does.
5. **Add `ft intro` / `ft doctor` (§7).** The onboarding gap is a
   premise gap for the "any other power user" audience, and it's
   closable with two small, thesis-aligned commands.
6. **Name the TUI's scope (§4).** Not a code change; a scope
   statement that gives future TUI changes a yes/no test against
   the thesis, before the TUI's gravity pulls in another tab.

The note-flow core — ghosts, paragraph graph, git memory, gather,
synth — needs no premise work. It is the part the rest of the tool
should be measured against, and mostly is.
