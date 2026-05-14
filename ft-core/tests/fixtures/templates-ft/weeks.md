---
tags: [WeekJournal, {{ today | date(format="%Y") }}, {{ today | date(format="%Y-%m-%d") }}]
---
{%- set monday = title | parse_date(format="%G Week %V") %}
# {{ title }}

[[Task Overview]] [[{{ monday | weekday_of(1) | date(format="%Y-%m-%d") }}|Mon]] [[{{ monday | weekday_of(2) | date(format="%Y-%m-%d") }}|Tue]] [[{{ monday | weekday_of(3) | date(format="%Y-%m-%d") }}|Wed]] [[{{ monday | weekday_of(4) | date(format="%Y-%m-%d") }}|Thu]] [[{{ monday | weekday_of(5) | date(format="%Y-%m-%d") }}|Fri]] [[{{ monday | weekday_of(6) | date(format="%Y-%m-%d") }}|Sat]] [[{{ monday | weekday_of(7) | date(format="%Y-%m-%d") }}|Sun]]

![[Project Overview {{ monday | date(format="%Y") }}#Overview]]
![[{{ monday | date(format="%Y") }} Q{{ monday | quarter }}]]

---
## This Week
