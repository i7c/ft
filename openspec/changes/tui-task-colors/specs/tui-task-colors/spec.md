## ADDED Requirements

### Requirement: Color tags

A task SHALL be colored by a `#color/<name>` hashtag in its description, where `<name>` is one of `red`, `yellow`, `green`, `blue`, `magenta`, `cyan`. When a line carries several such tags, the first SHALL count. Tags with other names under `color/` SHALL be ordinary tags. No task-format, ft-core or CLI change SHALL be made.

#### Scenario: Own color
- **WHEN** a task line is `- [ ] Ship Q4 integrations #color/blue`
- **THEN** its Tasks-tab row renders its description in blue

### Requirement: Inheritance along the displayed path

A row's color SHALL be its task's own color, else the color of its nearest colored ancestor row on the path it is displayed under (physical or `⤴`-linked). A root row (depth 0) without its own color SHALL inherit from its effective ancestors outside the view: effective parents are searched breadth-first in `parent_keys` order (physical parent first), and the first own color found applies.

#### Scenario: Subtasks inherit
- **WHEN** `Ship Q4 #color/blue` is expanded and has an indented child and a `⤴`-linked child, neither colored
- **THEN** both child rows render blue

#### Scenario: Own color wins
- **WHEN** a child of a blue task carries `#color/red`
- **THEN** it and its own subtree render red

#### Scenario: Two parents, two colors
- **WHEN** a task with `⤴ a1,b1` is shown under a blue parent `a1` and under a green parent `b1`
- **THEN** it renders blue under `a1` and green under `b1`

#### Scenario: Parent filtered out
- **WHEN** the query matches a subtask but not its blue parent
- **THEN** the subtask's root row renders blue

### Requirement: Row rendering

Only the description and its tree glyphs SHALL take the row color. Status, priority, due, scheduled and age columns SHALL keep their own colors; done and cancelled rows SHALL stay dimmed; the selected row SHALL keep its highlight background. Known `#color/<name>` words SHALL be hidden from the displayed description (the file is unchanged).

#### Scenario: Tag hidden
- **WHEN** a row's task is `Ship Q4 #color/blue #work`
- **THEN** the row text reads `Ship Q4 #work`

### Requirement: Assign colors with `m`

The Tasks tab SHALL bind `m` to `tasks.color`, which opens a picker listing `red`, `yellow`, `green`, `blue`, `magenta`, `cyan` and `none`. Choosing a color SHALL replace any `#color/<known name>` tag on each target task's line with the chosen one; `none` SHALL remove them. Targets SHALL be the cut set when it is non-empty (which is then cleared), otherwise the cursor task. Writes SHALL go through the guarded task update; a stale line SHALL toast the error and request a refresh. Without a selected task and an empty cut set, `m` SHALL toast `select a task first`.

#### Scenario: Color the cursor task
- **WHEN** the cursor is on `Ship Q4` and the user presses `m` and picks `green`
- **THEN** the line gains `#color/green` (replacing any other color tag) and the row and its subtree render green after the refresh

#### Scenario: Clear a color
- **WHEN** the user picks `none` on a task tagged `#color/red`
- **THEN** the tag is removed from the line

#### Scenario: Color the cut set
- **WHEN** two tasks are marked with `d` and the user picks `magenta`
- **THEN** both lines gain `#color/magenta` and the cut set is empty
