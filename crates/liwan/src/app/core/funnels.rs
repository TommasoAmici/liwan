use anyhow::{Result, bail};
use rand::distr::{Alphanumeric, SampleString};
use rusqlite::OptionalExtension;

#[cfg(test)]
use crate::app::models::FunnelMatch;
use crate::app::{
    SqlitePool,
    models::{Funnel, FunnelStep},
};

const MAX_NAME_CHARS: usize = 100;
const MIN_STEPS: usize = 2;
const MAX_STEPS: usize = 10;
const MAX_PATH_BYTES: usize = 2048;
const MAX_EVENT_BYTES: usize = 255;
const FUNNEL_COLUMNS: &str = "id, project_id, display_name, steps_json";

/// SQLite store for the funnels saved in each project.
#[derive(Clone)]
pub struct LiwanFunnels {
    pool: SqlitePool,
}

impl LiwanFunnels {
    /// Create a funnel store.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// List a project's funnels ordered by name, case-insensitively, then id.
    pub fn list(&self, project_id: &str) -> Result<Vec<Funnel>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached(&format!(
            "select {FUNNEL_COLUMNS} from funnels where project_id = ? order by display_name collate nocase, id"
        ))?;
        let funnels = stmt.query_map([project_id], funnel_from_row)?;
        Ok(funnels.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Get one funnel by project and funnel id.
    pub fn get(&self, project_id: &str, funnel_id: &str) -> Result<Option<Funnel>> {
        let conn = self.pool.get()?;
        let funnel = conn
            .prepare_cached(&format!("select {FUNNEL_COLUMNS} from funnels where project_id = ? and id = ?"))?
            .query_row(rusqlite::params![project_id, funnel_id], funnel_from_row)
            .optional()?;
        Ok(funnel)
    }

    /// Validate and store a new funnel.
    pub fn create(&self, project_id: &str, display_name: &str, steps: &[FunnelStep]) -> Result<Funnel> {
        let (display_name, steps) = validate_funnel(display_name, steps)?;
        let id = Alphanumeric.sample_string(&mut rand::rng(), 16);
        self.pool.get()?.execute(
            "insert into funnels (id, project_id, display_name, steps_json) values (?, ?, ?, ?)",
            rusqlite::params![id, project_id, display_name, serde_json::to_string(&steps)?],
        )?;
        Ok(Funnel { id, project_id: project_id.to_string(), display_name, steps })
    }

    /// Validate and replace a funnel's name and steps, returning whether it is in this project.
    pub fn update(&self, project_id: &str, funnel_id: &str, display_name: &str, steps: &[FunnelStep]) -> Result<bool> {
        let (display_name, steps) = validate_funnel(display_name, steps)?;
        let updated = self.pool.get()?.execute(
            "update funnels set display_name = ?, steps_json = ? where project_id = ? and id = ?",
            rusqlite::params![display_name, serde_json::to_string(&steps)?, project_id, funnel_id],
        )?;
        Ok(updated > 0)
    }

    /// Delete a funnel, returning whether it existed in this project.
    pub fn delete(&self, project_id: &str, funnel_id: &str) -> Result<bool> {
        let deleted = self
            .pool
            .get()?
            .execute("delete from funnels where project_id = ? and id = ?", rusqlite::params![project_id, funnel_id])?;
        Ok(deleted > 0)
    }
}

fn funnel_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Funnel> {
    let steps_json: String = row.get(3)?;
    let steps = serde_json::from_str(&steps_json)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error)))?;
    Ok(Funnel { id: row.get(0)?, project_id: row.get(1)?, display_name: row.get(2)?, steps })
}

/// Validate and normalize a funnel name and its steps. Create and update both use this.
pub fn validate_funnel(display_name: &str, steps: &[FunnelStep]) -> Result<(String, Vec<FunnelStep>)> {
    let display_name = display_name.trim().to_string();
    if display_name.is_empty() || display_name.chars().count() > MAX_NAME_CHARS {
        bail!("Name must be between 1 and {MAX_NAME_CHARS} characters");
    }
    if display_name.chars().any(char::is_control) {
        bail!("Name cannot contain control characters");
    }

    if !(MIN_STEPS..=MAX_STEPS).contains(&steps.len()) {
        bail!("A funnel must have between {MIN_STEPS} and {MAX_STEPS} steps");
    }

    let steps = steps
        .iter()
        .map(|step| match step {
            FunnelStep::Page { path, match_type } => {
                let path = path.trim();
                if path.is_empty() || path.len() > MAX_PATH_BYTES {
                    bail!("A page step path must be between 1 and {MAX_PATH_BYTES} bytes");
                }
                if path.chars().any(char::is_control) {
                    bail!("A page step path cannot contain control characters");
                }
                Ok(FunnelStep::Page { path: path.to_string(), match_type: *match_type })
            }
            FunnelStep::Event { name } => {
                let name = name.trim();
                if name.is_empty() || name.len() > MAX_EVENT_BYTES {
                    bail!("An event step name must be between 1 and {MAX_EVENT_BYTES} bytes");
                }
                if name == "pageview" {
                    bail!("A pageview step must use a page, not a custom event");
                }
                if name.chars().any(char::is_control) {
                    bail!("An event step name cannot contain control characters");
                }
                Ok(FunnelStep::Event { name: name.to_string() })
            }
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((display_name, steps))
}

/// Build the two-step page funnel used by the store tests.
#[cfg(test)]
fn page_step(path: &str) -> FunnelStep {
    FunnelStep::Page { path: path.to_string(), match_type: FunnelMatch::Equal }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        Liwan,
        models::{Entity, Project, ProjectVisibility},
    };
    use crate::config::Config;

    fn app_with_project() -> std::sync::Arc<Liwan> {
        let app = Liwan::new_memory(Config::default()).unwrap();
        for (id, name) in [("site", "Site"), ("other", "Other")] {
            app.projects
                .create(
                    &Project {
                        id: id.into(),
                        display_name: name.into(),
                        visibility: ProjectVisibility::Private,
                        secret: None,
                    },
                    &[],
                )
                .unwrap();
        }
        app
    }

    fn event_step(name: &str) -> FunnelStep {
        FunnelStep::Event { name: name.into() }
    }

    fn steps(count: usize) -> Vec<FunnelStep> {
        (0..count)
            .map(|index| if index == 0 { page_step("/pricing") } else { event_step(&format!("step{index}")) })
            .collect()
    }

    #[test]
    fn funnels_round_trip_through_the_store() {
        let app = app_with_project();
        let created = app.funnels.create("site", "  Signup funnel ", &steps(3)).unwrap();
        assert_eq!(created.id.len(), 16);
        assert!(created.id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(created.project_id, "site");
        assert_eq!(created.display_name, "Signup funnel");
        assert_eq!(created.steps, vec![page_step("/pricing"), event_step("step1"), event_step("step2"),]);

        assert_eq!(app.funnels.list("site").unwrap(), vec![created.clone()]);
        assert_eq!(app.funnels.get("site", &created.id).unwrap(), Some(created.clone()));

        let updated = app
            .funnels
            .update("site", &created.id, " checkout ", &[page_step("/cart"), event_step("purchase")])
            .unwrap();
        assert!(updated);
        let stored = app.funnels.get("site", &created.id).unwrap().unwrap();
        assert_eq!(stored.display_name, "checkout");
        assert_eq!(stored.steps, vec![page_step("/cart"), event_step("purchase")]);

        assert!(app.funnels.delete("site", &created.id).unwrap());
        assert!(!app.funnels.delete("site", &created.id).unwrap());
        assert!(app.funnels.list("site").unwrap().is_empty());
        assert_eq!(app.funnels.get("site", &created.id).unwrap(), None);
    }

    #[test]
    fn page_match_types_survive_the_store() {
        let app = app_with_project();
        let created = app
            .funnels
            .create(
                "site",
                "Browse",
                &[
                    FunnelStep::Page { path: "/blog".into(), match_type: FunnelMatch::Contains },
                    FunnelStep::Page { path: "/post".into(), match_type: FunnelMatch::StartsWith },
                ],
            )
            .unwrap();
        assert_eq!(app.funnels.get("site", &created.id).unwrap().unwrap().steps, created.steps);
    }

    #[test]
    fn funnels_are_ordered_by_name_case_insensitively() {
        let app = app_with_project();
        let beta = app.funnels.create("site", "beta", &steps(2)).unwrap();
        let alpha = app.funnels.create("site", "Alpha", &steps(2)).unwrap();
        let same = app.funnels.create("site", "alpha", &steps(2)).unwrap();
        let mut expected = vec![alpha, same, beta];
        expected.sort_by(|a, b| {
            a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()).then_with(|| a.id.cmp(&b.id))
        });
        assert_eq!(app.funnels.list("site").unwrap(), expected);
    }

    #[test]
    fn funnels_allow_two_to_ten_steps() {
        let app = app_with_project();
        assert!(app.funnels.create("site", "short", &steps(2)).is_ok());
        assert!(app.funnels.create("site", "long", &steps(10)).is_ok());
        assert!(app.funnels.create("site", "none", &steps(0)).is_err());
        assert!(app.funnels.create("site", "one", &steps(1)).is_err());
        assert!(app.funnels.create("site", "eleven", &steps(11)).is_err());
        assert_eq!(app.funnels.list("site").unwrap().len(), 2);
    }

    #[test]
    fn invalid_funnels_are_rejected_without_saving() {
        let app = app_with_project();
        let cases: Vec<(&str, Vec<FunnelStep>)> = vec![
            ("", steps(2)),
            ("   ", steps(2)),
            ("line\nbreak", steps(2)),
            ("Name", vec![page_step("/pricing"), page_step("   ")]),
            ("Name", vec![page_step("/pricing"), page_step("bad\npath")]),
            ("Name", vec![page_step("/pricing"), event_step("")]),
            ("Name", vec![page_step("/pricing"), event_step("  ")]),
            ("Name", vec![page_step("/pricing"), event_step("line\nbreak")]),
            ("Name", vec![page_step("/pricing"), event_step("pageview")]),
        ];
        for (name, steps) in cases {
            assert!(app.funnels.create("site", name, &steps).is_err(), "funnel {name:?} {steps:?} should be rejected");
        }
        assert!(app.funnels.list("site").unwrap().is_empty());

        let created = app.funnels.create("site", "Name", &steps(2)).unwrap();
        assert!(app.funnels.update("site", &created.id, "Name", &vec![event_step("pageview")]).is_err());
        assert_eq!(app.funnels.get("site", &created.id).unwrap().unwrap(), created);
    }

    #[test]
    fn funnel_values_are_trimmed_to_their_limits() {
        let app = app_with_project();
        let created = app
            .funnels
            .create(
                "site",
                "Name",
                &[
                    FunnelStep::Page { path: format!(" /{} ", "a".repeat(2047)), match_type: FunnelMatch::Equal },
                    FunnelStep::Event { name: format!(" {} ", "b".repeat(255)) },
                ],
            )
            .unwrap();
        assert_eq!(created.steps[0], page_step(&format!("/{}", "a".repeat(2047))));
        assert_eq!(created.steps[1], event_step(&"b".repeat(255)));

        assert!(
            app.funnels
                .create("site", "Too long", &[page_step(&format!("/{}", "a".repeat(2048))), event_step("signup")])
                .is_err()
        );
        assert!(
            app.funnels.create("site", "Too long", &[page_step("/pricing"), event_step(&"b".repeat(256))]).is_err()
        );
    }

    #[test]
    fn funnels_are_scoped_to_their_project() {
        let app = app_with_project();
        let funnel = app.funnels.create("site", "Mine", &steps(2)).unwrap();
        assert_eq!(app.funnels.get("other", &funnel.id).unwrap(), None);
        assert!(!app.funnels.update("other", &funnel.id, "Stolen", &steps(2)).unwrap());
        assert!(!app.funnels.delete("other", &funnel.id).unwrap());
        assert_eq!(app.funnels.get("site", &funnel.id).unwrap(), Some(funnel));
    }

    #[test]
    fn deleting_a_project_removes_its_funnels() {
        let app = app_with_project();
        app.entities.create(&Entity { id: "entity".into(), display_name: "Entity".into() }, &[]).unwrap();
        app.funnels.create("site", "Mine", &steps(2)).unwrap();
        app.projects.delete("site").unwrap();
        assert!(app.funnels.list("site").unwrap().is_empty());
    }
}
