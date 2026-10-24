"""Generate tests/fixtures/search-eval/{vault/,judgments.toml}.

Judgments are written as (path, snippet) and resolved to the 1-indexed
line where the snippet first occurs, so the TOML stays exact."""
import os, sys, shutil

root = sys.argv[1]
vault = os.path.join(root, "vault")
shutil.rmtree(vault, ignore_errors=True)

notes = {}

notes["know/Symmetric Multiprocessing.md"] = """# Symmetric Multiprocessing

Symmetric multiprocessing is a shared memory architecture where every processor reaches the same main memory with uniform cost.

## Scaling limits

Bus contention grows with the processor count, which is why large machines moved to NUMA designs.
"""

notes["know/NUMA.md"] = """# NUMA

Non-uniform memory access gives each socket its own local memory; remote memory is slower. See [[Symmetric Multiprocessing]].
"""

notes["know/Kernel Flags.md"] = """# Kernel Flags

Enable cgroups for memory limits on the Raspberry Pi with `cgroup_enable=memory cgroup_memory=1` in the boot command line.
"""

notes["know/Memoization.md"] = """---
aliases: [memo cache]
---
# Memoization

Memoization stores the result of a pure function call so a repeated call with identical arguments returns instantly.

Repeated eigen decompositions of one matrix are a classic place to apply this; key the cache by a hash of the matrix.

See also [[Caching]] and [[Dynamic Programming]].
"""

notes["know/Caching.md"] = """# Caching

A cache trades memory for latency: keep recently used results close to where they are needed.

Invalidation is the hard part; prefer time-based expiry when the source of truth is remote.
"""

notes["know/Dynamic Programming.md"] = """# Dynamic Programming

Dynamic programming solves overlapping subproblems once and reuses their answers, usually with a table. [[Memoization]] is the top-down variant.
"""

notes["know/Eigenvalues.md"] = """# Eigenvalues

An eigenvalue scales its eigenvector under a linear map.

Eigendecomposition factors a diagonalizable matrix into eigenvectors and eigenvalues.
"""

notes["know/Linear Algebra.md"] = """# Linear Algebra

Eigen methods show up everywhere in linear algebra: the spectral theorem, principal components, and the stationary state of Markov chains.

The determinant is the volume scaling factor of the map; it is zero exactly when the map collapses a dimension.
"""

notes["proj/busao/Route Planning.md"] = """---
tags: [proj/busao]
---
# Route Planning

- **Stop memory**: approved synthetic stops become known stops with usage counts
  and rank ahead of fresh candidates when snapping.
- **Corridor memory**: count how often each road segment appears in approved routes
  and apply a bounded discount to those edges in future solves.
- **Parameter memory**: per-plant defaults where the planners consistently override.
- Snap candidates to the road network before solving.
- Penalize left turns across the busy avenues.
- Respect the maximum ride time per passenger.
- Keep the vehicle capacity constraint hard.
- Re-run the solver nightly with the latest roster.
- Export the routes as GeoJSON for the planners.
- Log every manual override for later analysis.
- Cache the distance matrix between solves.
- Compare against the routes of last week before publishing.
"""

notes["proj/busao/Ônibus Fretado.md"] = """---
tags: [proj/busao]
---
# Ônibus fretado

O ônibus fretado sai da garagem às seis horas e passa por todas as estações da linha norte.

As casas perto da avenida principal ficam longe das paradas atuais.
"""

notes["proj/busao/Paradas.md"] = """# Paradas

A parada nova fica em frente à padaria, no caminho do [[Ônibus Fretado]].
"""

notes["proj/fluxo/Fluxo.md"] = """---
tags: [proj/fluxo]
---
# Fluxo

Punch-in and punch-out events land in one Postgres table; the release cadence is every second Friday.

Rip out the in-memory database and run every integration test against a real Postgres.
"""

notes["areas/work/Deep Work.md"] = """---
aliases: [focus routine]
tags: [habits]
---
# Deep Work

Block two hours every morning with notifications off and a single written goal.

Batch shallow tasks like email into one slot after lunch.
"""

notes["areas/work/Runbook.md"] = """# Runbook

## Rollback

Revert the deploy by promoting the previous release tag; never hotfix forward during an incident.

## Monitoring

Watch the error rate dashboard for fifteen minutes after every release.
"""

notes["areas/work/Retro 2026-09.md"] = """---
tags: [retrospective]
---
# Retro September

Action items: pair on the flaky integration tests and move the release to Thursdays.

Went well: the on-call rotation was quiet for the whole month.
"""

notes["areas/work/Release Checklist.md"] = """# Release Checklist

Tag the commit, publish the changelog, and announce the release in the team channel.
"""

notes["areas/work/Build System.md"] = """# Build System

The build cache keys every artifact by its inputs; a warm cache cuts the pipeline from twenty minutes to three, and clearing the cache fixes most flaky builds.
"""

notes["areas/work/Meeting Notes.md"] = """# Meeting Notes

The routine maintenance window needs more focus from the platform team before the audit.
"""

notes["areas/home/Café.md"] = """# Café

The café on the corner roasts its own beans; the naïve espresso blend is surprisingly good.
"""

notes["areas/home/Garden.md"] = """# Garden

Garden memories from last summer: tomatoes everywhere and far too much zucchini.
"""

notes["areas/home/Recipes.md"] = """# Recipes

The bread needs a long cold proof in the fridge; the crumb is open and the crust is dark.
"""

notes["archive/Old Cache Notes.md"] = """# Old Cache Notes

Memcached was the cache of choice before the move to the managed service.
"""

notes["archive/Old Deploy.md"] = """# Old Deploy

The old deploy script copied tarballs over ssh and restarted the service by hand.
"""

# Journal filler: common words, light distractors, one inline tag.
journal = [
    ("Talked through the release plan with the team; the memory leak in the importer is fixed.",
     "Read about the history of the printing press in the evening."),
    ("Cleaned up the backlog and closed the stale tickets from the summer.",
     "The weather turned cold, so the garden beds got their winter cover."),
    ("#idea Share the distance matrix cache across plants instead of rebuilding it per solve.",
     "Lunch with the old team; everyone is busy with the reorg."),
    ("Spent the morning on the onboarding docs for the new hires.",
     "The train was late again; worked on the slides during the delay."),
    ("Deploy went out late because the staging database was down.",
     "Watched a documentary about the deep ocean and its strange fish."),
    ("Paired on the parser; the tokenizer now handles the edge cases from the bug report.",
     "Bought new running shoes; the old ones were worn through."),
    ("The quarterly review is next week; drafted the summary of the wins and the misses.",
     "Long walk by the river in the afternoon."),
    ("Profiled the importer: most of the time goes to the JSON decoding, not the database.",
     "Called the family in the evening."),
    ("Wrote the proposal for the new search ranking and shared it with the team.",
     "The neighbours are renovating the kitchen; the drilling starts at eight."),
    ("Reviewed the security findings; two of them need a fix before the release.",
     "Finished the novel; the ending was rushed."),
    ("Moved the backups to the new bucket and tested a full restore.",
     "Made soup with the last of the garden vegetables."),
    ("The vendor call ran long; the contract renewal is still open.",
     "Practiced the guitar for half an hour."),
    ("Rewrote the onboarding checklist so the first week has a clear goal.",
     "The cat knocked the plant off the shelf again."),
    ("Investigated the flaky test; it depends on the order of the hash map iteration.",
     "Cooked dinner for the friends from the climbing gym."),
    ("Planned the next sprint around the reporting milestone.",
     "Fixed the bike brakes in the evening."),
    ("Interviewed two candidates for the platform role.",
     "The library had a sale on old atlases."),
    ("Migrated the cron jobs to the scheduler service.",
     "Slept badly; the street was loud all night."),
    ("Updated the dashboards so the error budget is visible to the whole team.",
     "Went to the market for apples and cheese."),
]
for i, (a, b) in enumerate(journal, start=1):
    notes[f"journal/2026-09-{i:02d}.md"] = f"# 2026-09-{i:02d}\n\n{a}\n\n{b}\n"

for rel, content in notes.items():
    p = os.path.join(vault, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w", encoding="utf-8") as f:
        f.write(content)

def line_of(rel, snippet):
    for i, l in enumerate(notes[rel].splitlines(), start=1):
        if snippet in l:
            return i
    raise SystemExit(f"snippet {snippet!r} not in {rel}")

SMP = "know/Symmetric Multiprocessing.md"
MEMO = "know/Memoization.md"
cases = [
    ("memory", False, "length-norm: focused paragraphs beat the long bullet list",
     [(SMP, "shared memory architecture", 3), ("know/NUMA.md", "Non-uniform", 2),
      ("know/Caching.md", "trades memory", 1)]),
    ("the eigendecomposition", True, "idf: the rare term outweighs the common one",
     [("know/Eigenvalues.md", "Eigendecomposition factors", 3)]),
    ("eigen memoization", False, "context-title: body has eigen, title has memoization",
     [(MEMO, "Repeated eigen", 3)]),
    ("focus routine", False, "context-alias: the alias names the note",
     [("areas/work/Deep Work.md", "Block two hours", 3), ("areas/work/Deep Work.md", "Batch shallow", 2)]),
    ("rollback release", False, "context-heading: the heading carries rollback",
     [("areas/work/Runbook.md", "Revert the deploy", 3)]),
    ("retrospective release", False, "context-tag: the frontmatter tag carries retrospective",
     [("areas/work/Retro 2026-09.md", "Action items", 3)]),
    ("=ônibus", False, "diacritics-pt: accented word clause",
     [("proj/busao/Ônibus Fretado.md", "O ônibus fretado", 3)]),
    ("onibus", False, "diacritics-pt: unaccented query finds accented text",
     [("proj/busao/Ônibus Fretado.md", "O ônibus fretado", 3), ("proj/busao/Paradas.md", "A parada nova", 1)]),
    ("cafe", False, "diacritics-en: unaccented query finds café",
     [("areas/home/Café.md", "roasts its own beans", 3)]),
    ("=naive", False, "diacritics-en: word clause over naïve",
     [("areas/home/Café.md", "roasts its own beans", 3)]),
    ("=memory garden", False, "stemming: memories satisfies =memory",
     [("areas/home/Garden.md", "Garden memories", 3)]),
    ("memory path:know/", False, "filter-path: only know/ paragraphs",
     [(SMP, "shared memory architecture", 3), ("know/NUMA.md", "Non-uniform", 2),
      ("know/Caching.md", "trades memory", 1)]),
    ("ônibus tag:proj", False, "filter-tag: hierarchical tag prefix",
     [("proj/busao/Ônibus Fretado.md", "O ônibus fretado", 3)]),
    ("cache tag:idea", False, "filter-tag: inline paragraph tag",
     [("journal/2026-09-03.md", "#idea", 3)]),
    ("cache scope:know-notes", False, "filter-scope: graph preset scoping",
     [("know/Caching.md", "trades memory", 3), (MEMO, "Repeated eigen", 2)]),
    ("cache near:[[Memoization]]", False, "near: graph proximity to the seed note",
     [("know/Caching.md", "trades memory", 3), (MEMO, "Repeated eigen", 3),
      (MEMO, "Memoization stores", 1)]),
    ('"shared memory"', False, "regression-phrase",
     [(SMP, "shared memory architecture", 3)]),
    ("[[Memoization]]", False, "regression-link",
     [("know/Dynamic Programming.md", "Dynamic programming solves", 3)]),
    ("~memoizaton", False, "regression-fuzzy",
     [(MEMO, "Memoization stores", 3), ("know/Dynamic Programming.md", "Dynamic programming solves", 2)]),
    ("memory -cgroup", False, "regression-exclude",
     [(SMP, "shared memory architecture", 3), ("know/NUMA.md", "Non-uniform", 2)]),
    ("eigen", False, "general",
     [("know/Eigenvalues.md", "An eigenvalue scales", 3), ("know/Linear Algebra.md", "Eigen methods", 2),
      ("know/Eigenvalues.md", "Eigendecomposition factors", 2), (MEMO, "Repeated eigen", 1)]),
    ("release", False, "general",
     [("areas/work/Release Checklist.md", "Tag the commit", 3), ("areas/work/Runbook.md", "Revert the deploy", 2),
      ("areas/work/Retro 2026-09.md", "Action items", 1), ("areas/work/Runbook.md", "Watch the error rate", 1)]),
    ("garagem", False, "general",
     [("proj/busao/Ônibus Fretado.md", "O ônibus fretado", 3)]),
    ("notifications morning", False, "general",
     [("areas/work/Deep Work.md", "Block two hours", 3)]),
    ("numa", False, "general",
     [("know/NUMA.md", "Non-uniform", 3), (SMP, "Bus contention", 2)]),
    ("invalidation", False, "general",
     [("know/Caching.md", "Invalidation is the hard part", 3)]),
    ("deploy", False, "general",
     [("areas/work/Runbook.md", "Revert the deploy", 3), ("archive/Old Deploy.md", "old deploy script", 2),
      ("journal/2026-09-05.md", "Deploy went out late", 1)]),
    ("tomatoes", False, "general",
     [("areas/home/Garden.md", "Garden memories", 3)]),
    ("cache -path:archive/", False, "filter-path: negated path filter",
     [("know/Caching.md", "trades memory", 3), ("areas/work/Build System.md", "The build cache", 3),
      (MEMO, "Repeated eigen", 2), ("journal/2026-09-03.md", "#idea", 2)]),
]

# Concept queries for the semantic eval: no query word appears in the
# judged paragraph, so lexical search alone cannot rank it.
semantic_cases = [
    ("concentrating without interruptions", False, "concept: Deep Work routine",
     [("areas/work/Deep Work.md", "Block two hours", 3)]),
    ("undoing a botched launch", False, "concept: rollback runbook",
     [("areas/work/Runbook.md", "Revert the deploy", 3), ("archive/Old Deploy.md", "old deploy script", 1)]),
    ("remembering outputs of slow computations", False, "concept: memoization",
     [(MEMO, "Memoization stores", 3), ("know/Dynamic Programming.md", "Dynamic programming solves", 2),
      ("know/Caching.md", "trades memory", 1)]),
    ("coffee shop", False, "concept: cafe",
     [("areas/home/Café.md", "roasts its own beans", 3)]),
    ("homegrown produce", False, "concept: garden",
     [("areas/home/Garden.md", "Garden memories", 3), ("journal/2026-09-11.md", "garden vegetables", 2)]),
    ("multiple CPUs accessing one RAM pool", False, "concept: shared memory",
     [(SMP, "shared memory architecture", 3), ("know/NUMA.md", "Non-uniform", 2)]),
    ("sourdough loaf proofing overnight", False, "concept: bread",
     [("areas/home/Recipes.md", "long cold proof", 3)]),
    ("lessons learned looking back", False, "concept: retrospective",
     [("areas/work/Retro 2026-09.md", "Action items", 3), ("areas/work/Retro 2026-09.md", "Went well", 2)]),
    ("speeding up continuous integration", False, "concept: build cache",
     [("areas/work/Build System.md", "The build cache", 3)]),
    ("evening stroll near water", False, "concept: walk by the river",
     [("journal/2026-09-07.md", "Long walk by the river", 3)]),
    ("recruiting engineers", False, "concept: interviews and onboarding",
     [("journal/2026-09-16.md", "Interviewed two candidates", 3), ("journal/2026-09-04.md", "onboarding docs", 1),
      ("journal/2026-09-13.md", "onboarding checklist", 1)]),
    ("eigen matrix", False, "concept-control: shares words, lexical must keep working",
     [("know/Eigenvalues.md", "Eigendecomposition factors", 3), ("know/Eigenvalues.md", "An eigenvalue scales", 2)]),
]

def q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'

out = ["# Relevance judgments for the paragraph search eval harness.",
       "# Generated alongside vault/; `line` is any line inside the judged paragraph.",
       "# grade: 1 = somewhat relevant, 2 = relevant, 3 = ideal.", ""]
for query, any_, note, rels in cases:
    out.append("[[case]]")
    out.append(f"query = {q(query)}")
    if any_:
        out.append("any = true")
    out.append(f"note = {q(note)}")
    for rel, snip, grade in rels:
        out.append("[[case.relevant]]")
        out.append(f"path = {q(rel)}")
        out.append(f"line = {line_of(rel, snip)}")
        out.append(f"grade = {grade}")
    out.append("")
with open(os.path.join(root, "judgments.toml"), "w", encoding="utf-8") as f:
    f.write("\n".join(out))

def render(cases, header):
    out = header[:]
    for query, any_, note, rels in cases:
        out.append("[[case]]")
        out.append(f"query = {q(query)}")
        if any_:
            out.append("any = true")
        out.append(f"note = {q(note)}")
        for rel, snip, grade in rels:
            out.append("[[case.relevant]]")
            out.append(f"path = {q(rel)}")
            out.append(f"line = {line_of(rel, snip)}")
            out.append(f"grade = {grade}")
        out.append("")
    return "\n".join(out)

with open(os.path.join(root, "semantic.toml"), "w", encoding="utf-8") as f:
    f.write(render(semantic_cases, [
        "# Concept-query judgments for the semantic (hybrid) eval: the judged",
        "# paragraphs share no words with the query (except the control case).",
        "# Generated by generate.py alongside vault/ and judgments.toml.", ""]))
print(len(notes), "notes,", len(cases), "cases")
