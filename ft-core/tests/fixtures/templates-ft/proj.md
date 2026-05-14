---
tags: [Created-{{ today | date(format="%Y-%m-%d") }}, PROJ, Project]
---
# {{ title }}
-short description-
## Status
Proposed <progress max=100 value=0 />
## Scope and Success
-what is the definition of done? how does success look like?-
## Execution
%% Begin Waypoint %%

%% End Waypoint %%

```tasks
(not done) AND ((path includes {{ title }}) OR (description includes [[{title}]]))
show tree
group by filename
sort by due
sort by priority
sort by created
```

