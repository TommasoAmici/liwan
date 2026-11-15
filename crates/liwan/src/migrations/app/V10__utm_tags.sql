create table project_utm_tags (
    id text primary key not null,
    project_id text not null,
    display_name text not null,
    url text not null,
    utm_source text,
    utm_medium text,
    utm_campaign text,
    utm_content text,
    utm_term text,
    created_at timestamp not null,
    updated_at timestamp not null,
    foreign key (project_id) references projects(id)
);

create index project_utm_tags_project_id on project_utm_tags (project_id);
