alter table projects add column sort_order integer not null default 0;

update projects
set sort_order = (
    select count(*) from projects as earlier where earlier.rowid < projects.rowid
);
