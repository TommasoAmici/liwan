create table funnels (
    id text primary key not null,
    project_id text not null,
    display_name text not null,
    steps_json text not null,
    foreign key (project_id) references projects(id)
);

create index funnels_project_id on funnels (project_id);
