use anyhow::{Result, bail};

use crate::app::{SqlitePool, models};
use crate::utils::validate;

#[derive(Clone)]
pub struct LiwanProjects {
    pool: SqlitePool,
}

impl LiwanProjects {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Replace the entities associated with a project
    pub fn update_entities(&self, project_id: &str, entity_ids: &[String]) -> Result<()> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        tx.execute("delete from project_entities where project_id = ?", rusqlite::params![project_id])?;
        {
            let mut exists = tx.prepare_cached("select 1 from entities where id = ? limit 1")?;
            for entity_id in entity_ids {
                if !exists.exists([entity_id])? {
                    bail!("entity not found: {entity_id}");
                }
                tx.execute(
                    "insert into project_entities (project_id, entity_id) values (:project_id, :entity_id)",
                    rusqlite::named_params! { ":project_id": project_id, ":entity_id": entity_id },
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Get all entities associated with a project
    pub fn entities(&self, project_id: &str) -> Result<Vec<models::Entity>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached(
            "select e.id, e.display_name from entities e join project_entities pe on e.id = pe.entity_id where pe.project_id = ?",
        )?;
        let entities = stmt.query_map(rusqlite::params![project_id], |row| {
            Ok(models::Entity { id: row.get("id")?, display_name: row.get("display_name")? })
        })?;
        Ok(entities.collect::<Result<Vec<models::Entity>, rusqlite::Error>>()?)
    }

    /// Get all entity IDs associated with a project
    pub fn entity_ids(&self, project_id: &str) -> Result<Vec<String>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached("select entity_id from project_entities where project_id = ?")?;
        let entities = stmt.query_map(rusqlite::params![project_id], |row| row.get("entity_id"))?;
        Ok(entities.collect::<Result<Vec<String>, rusqlite::Error>>()?)
    }

    /// Get a project by ID
    pub fn get(&self, id: &str) -> Result<models::Project> {
        let conn = self.pool.get()?;
        let project = conn
            .prepare("select id, display_name, visibility, secret from projects where id = ?")?
            .query_row(rusqlite::params![id], |row| {
                Ok(models::Project {
                    id: row.get("id")?,
                    display_name: row.get("display_name")?,
                    visibility: row.get("visibility")?,
                    secret: row.get("secret")?,
                })
            })?;
        Ok(project)
    }

    /// Get all projects, in the order administrators chose
    pub fn all(&self) -> Result<Vec<models::Project>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "select id, display_name, visibility, secret from projects
             order by sort_order, display_name collate nocase, id",
        )?;
        let projects = stmt.query_map([], |row| {
            Ok(models::Project {
                id: row.get("id")?,
                display_name: row.get("display_name")?,
                visibility: row.get("visibility")?,
                secret: row.get("secret")?,
            })
        })?;

        Ok(projects.collect::<Result<Vec<models::Project>, rusqlite::Error>>()?)
    }

    /// Create a new project
    pub fn create(&self, project: &models::Project, initial_entities: &[String]) -> Result<models::Project> {
        if !validate::is_valid_id(&project.id) {
            bail!("invalid project ID");
        }
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        tx.execute(
            "insert into projects (id, display_name, visibility, secret, sort_order)
             values (:id, :display_name, :visibility, :secret, (select coalesce(max(sort_order) + 1, 0) from projects))",
            rusqlite::named_params! {
                ":id": project.id,
                ":display_name": project.display_name,
                ":visibility": project.visibility,
                ":secret": project.secret,
            },
        )?;
        {
            let mut exists = tx.prepare_cached("select 1 from entities where id = ? limit 1")?;
            for entity_id in initial_entities {
                if !exists.exists([entity_id])? {
                    bail!("entity not found: {entity_id}");
                }
                tx.execute(
                    "insert into project_entities (project_id, entity_id) values (:project_id, :entity_id)",
                    rusqlite::named_params! { ":project_id": project.id, ":entity_id": entity_id },
                )?;
            }
        }
        tx.commit()?;
        Ok(project.clone())
    }

    /// Update a project
    pub fn update(&self, project: &models::Project) -> Result<models::Project> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached(
            "update projects set display_name = :display_name, visibility = :visibility, secret = :secret where id = :id",
        )?;
        stmt.execute(rusqlite::named_params! {
            ":display_name": project.display_name,
            ":visibility": project.visibility,
            ":secret": project.secret,
            ":id": project.id,
        })?;
        Ok(project.clone())
    }

    /// Delete a project and its entity memberships
    pub fn delete(&self, id: &str) -> Result<()> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        tx.execute("delete from project_settings where project_id = ?", rusqlite::params![id])?;
        tx.execute("delete from project_utm_tags where project_id = ?", rusqlite::params![id])?;
        tx.execute("delete from funnels where project_id = ?", rusqlite::params![id])?;
        tx.execute("delete from api_key_projects where project_id = ?", rusqlite::params![id])?;
        tx.execute("delete from team_projects where project_id = ?", [id])?;
        tx.execute("delete from projects where id = ?", rusqlite::params![id])?;
        tx.execute("delete from project_entities where project_id = ?", rusqlite::params![id])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Liwan, models::ProjectVisibility};
    use crate::config::Config;

    fn app_with_projects(ids: &[&str]) -> std::sync::Arc<Liwan> {
        let app = Liwan::new_memory(Config::default()).unwrap();
        for id in ids {
            app.projects
                .create(
                    &models::Project {
                        id: (*id).into(),
                        display_name: (*id).into(),
                        visibility: ProjectVisibility::Private,
                        secret: None,
                    },
                    &[],
                )
                .unwrap();
        }
        app
    }

    fn listed(app: &Liwan) -> Vec<String> {
        app.projects.all().unwrap().into_iter().map(|project| project.id).collect()
    }

    fn set_positions(app: &Liwan, ids: &[&str]) {
        let conn = app.projects.pool.get().unwrap();
        for (position, id) in ids.iter().enumerate() {
            conn.execute("update projects set sort_order = ? where id = ?", rusqlite::params![position as i64, id])
                .unwrap();
        }
    }

    #[test]
    fn projects_are_listed_in_creation_order() {
        let app = app_with_projects(&["zeta", "alpha", "mu"]);
        assert_eq!(listed(&app), ["zeta", "alpha", "mu"]);
    }

    #[test]
    fn projects_are_listed_in_their_configured_order() {
        let app = app_with_projects(&["a", "b", "c"]);
        set_positions(&app, &["c", "a", "b"]);
        assert_eq!(listed(&app), ["c", "a", "b"]);
    }

    #[test]
    fn projects_sharing_a_position_are_listed_by_name() {
        let app = app_with_projects(&["zeta", "Alpha"]);
        let conn = app.projects.pool.get().unwrap();
        conn.execute("update projects set sort_order = 0", []).unwrap();
        assert_eq!(listed(&app), ["Alpha", "zeta"]);
    }

    #[test]
    fn a_new_project_is_appended_at_the_end() {
        let app = app_with_projects(&["a", "b", "c"]);
        set_positions(&app, &["c", "a", "b"]);
        app.projects
            .create(
                &models::Project {
                    id: "d".into(),
                    display_name: "d".into(),
                    visibility: ProjectVisibility::Private,
                    secret: None,
                },
                &[],
            )
            .unwrap();
        assert_eq!(listed(&app), ["c", "a", "b", "d"]);
    }
}
