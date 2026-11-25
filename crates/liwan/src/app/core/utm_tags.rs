use anyhow::{Result, bail};
use jiff::Timestamp;
use rand::distr::{Alphanumeric, SampleString};
use rusqlite::OptionalExtension;

use crate::app::{SqlitePool, models::UtmTag};
use crate::utils::timestamp::SqlTimestamp;

const MAX_NAME_CHARS: usize = 100;
const MAX_URL_BYTES: usize = 2048;
const MAX_UTM_VALUE_BYTES: usize = 200;
const TAG_COLUMNS: &str =
    "id, project_id, display_name, url, utm_source, utm_medium, utm_campaign, utm_content, utm_term";

/// Validated and normalized fields for creating or updating a UTM tag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UtmTagInput {
    pub display_name: String,
    pub url: String,
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
    pub utm_content: Option<String>,
    pub utm_term: Option<String>,
}

#[derive(Clone)]
pub struct LiwanUtmTags {
    pool: SqlitePool,
}

impl LiwanUtmTags {
    /// Create a UTM tag store.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// List a project's tags ordered by name, case-insensitively, then id.
    pub fn list(&self, project_id: &str) -> Result<Vec<UtmTag>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare_cached(&format!(
            "select {TAG_COLUMNS} from project_utm_tags where project_id = ? order by display_name collate nocase, id"
        ))?;
        let tags = stmt.query_map([project_id], tag_from_row)?;
        Ok(tags.collect::<Result<Vec<_>, rusqlite::Error>>()?)
    }

    /// Get one tag by project and tag id.
    pub fn get(&self, project_id: &str, tag_id: &str) -> Result<Option<UtmTag>> {
        let conn = self.pool.get()?;
        let tag = conn
            .prepare_cached(&format!("select {TAG_COLUMNS} from project_utm_tags where project_id = ? and id = ?"))?
            .query_row(rusqlite::params![project_id, tag_id], tag_from_row)
            .optional()?;
        Ok(tag)
    }

    /// Validate and store a new tag.
    pub fn create(&self, project_id: &str, input: &UtmTagInput) -> Result<UtmTag> {
        let input = validate_utm_tag(input.clone())?;
        let id = Alphanumeric.sample_string(&mut rand::rng(), 16);
        let now = Timestamp::now();
        self.pool.get()?.execute(
            "insert into project_utm_tags
                (id, project_id, display_name, url, utm_source, utm_medium, utm_campaign, utm_content, utm_term, created_at, updated_at)
             values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                id,
                project_id,
                input.display_name,
                input.url,
                input.utm_source,
                input.utm_medium,
                input.utm_campaign,
                input.utm_content,
                input.utm_term,
                SqlTimestamp(now),
                SqlTimestamp(now),
            ],
        )?;
        Ok(input.into_tag(id, project_id.to_string()))
    }

    /// Validate and replace a tag's fields, returning `None` when it is not in this project.
    pub fn update(&self, project_id: &str, tag_id: &str, input: &UtmTagInput) -> Result<Option<UtmTag>> {
        let input = validate_utm_tag(input.clone())?;
        let updated = self.pool.get()?.execute(
            "update project_utm_tags set
                display_name = ?, url = ?, utm_source = ?, utm_medium = ?, utm_campaign = ?, utm_content = ?, utm_term = ?,
                updated_at = ?
             where project_id = ? and id = ?",
            rusqlite::params![
                input.display_name,
                input.url,
                input.utm_source,
                input.utm_medium,
                input.utm_campaign,
                input.utm_content,
                input.utm_term,
                SqlTimestamp(Timestamp::now()),
                project_id,
                tag_id,
            ],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        Ok(Some(input.into_tag(tag_id.to_string(), project_id.to_string())))
    }

    /// Delete a tag, returning whether it existed in this project.
    pub fn delete(&self, project_id: &str, tag_id: &str) -> Result<bool> {
        let deleted = self.pool.get()?.execute(
            "delete from project_utm_tags where project_id = ? and id = ?",
            rusqlite::params![project_id, tag_id],
        )?;
        Ok(deleted > 0)
    }
}

impl UtmTagInput {
    fn into_tag(self, id: String, project_id: String) -> UtmTag {
        UtmTag {
            id,
            project_id,
            display_name: self.display_name,
            url: self.url,
            utm_source: self.utm_source,
            utm_medium: self.utm_medium,
            utm_campaign: self.utm_campaign,
            utm_content: self.utm_content,
            utm_term: self.utm_term,
        }
    }
}

fn tag_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UtmTag> {
    Ok(UtmTag {
        id: row.get(0)?,
        project_id: row.get(1)?,
        display_name: row.get(2)?,
        url: row.get(3)?,
        utm_source: row.get(4)?,
        utm_medium: row.get(5)?,
        utm_campaign: row.get(6)?,
        utm_content: row.get(7)?,
        utm_term: row.get(8)?,
    })
}

/// Validate and normalize tag fields. Create and update both use this.
pub fn validate_utm_tag(input: UtmTagInput) -> Result<UtmTagInput> {
    let display_name = input.display_name.trim().to_string();
    if display_name.is_empty() || display_name.chars().count() > MAX_NAME_CHARS {
        bail!("Name must be between 1 and {MAX_NAME_CHARS} characters");
    }
    if display_name.chars().any(char::is_control) {
        bail!("Name cannot contain control characters");
    }

    let url = input.url.trim();
    if url.len() > MAX_URL_BYTES {
        bail!("Destination URL cannot exceed {MAX_URL_BYTES} bytes");
    }
    let parsed = url::Url::parse(url).map_err(|_| anyhow::anyhow!("Destination URL must be a valid URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none_or(str::is_empty) {
        bail!("Destination URL must be an absolute http or https URL");
    }
    if url.chars().any(char::is_control) {
        bail!("Destination URL cannot contain control characters");
    }

    let normalize = |field: &str, value: &Option<String>| -> Result<Option<String>> {
        let Some(value) = value.as_deref().map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        if value.len() > MAX_UTM_VALUE_BYTES {
            bail!("{field} cannot exceed {MAX_UTM_VALUE_BYTES} bytes");
        }
        if value.chars().any(char::is_control) {
            bail!("{field} cannot contain control characters");
        }
        Ok(Some(value.to_string()))
    };

    let utm_source = normalize("Source", &input.utm_source)?;
    let utm_medium = normalize("Medium", &input.utm_medium)?;
    let utm_campaign = normalize("Campaign", &input.utm_campaign)?;
    let utm_content = normalize("Content", &input.utm_content)?;
    let utm_term = normalize("Term", &input.utm_term)?;
    if [&utm_source, &utm_medium, &utm_campaign, &utm_content, &utm_term].iter().all(|value| value.is_none()) {
        bail!("A UTM tag needs at least one UTM value");
    }

    Ok(UtmTagInput { display_name, url: url.to_string(), utm_source, utm_medium, utm_campaign, utm_content, utm_term })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        Liwan,
        models::{Entity, Project, ProjectVisibility},
    };
    use crate::config::Config;

    fn input(source: Option<&str>) -> UtmTagInput {
        UtmTagInput {
            display_name: "Spring sale".into(),
            url: "https://example.com/landing?ref=newsletter".into(),
            utm_source: source.map(str::to_string),
            utm_medium: Some("cpc".into()),
            ..Default::default()
        }
    }

    fn app_with_project() -> std::sync::Arc<Liwan> {
        let app = Liwan::new_memory(Config::default()).unwrap();
        app.projects
            .create(
                &Project {
                    id: "site".into(),
                    display_name: "Site".into(),
                    visibility: ProjectVisibility::Private,
                    secret: None,
                },
                &[],
            )
            .unwrap();
        app
    }

    #[test]
    fn validation_trims_names_urls_and_values() {
        let validated = validate_utm_tag(UtmTagInput {
            display_name: "  Spring sale  ".into(),
            url: "  https://example.com/landing?ref=newsletter  ".into(),
            utm_source: Some("  newsletter ".into()),
            utm_medium: Some("   ".into()),
            utm_campaign: Some("".into()),
            utm_content: None,
            utm_term: Some(" spring ".into()),
        })
        .unwrap();
        assert_eq!(validated.display_name, "Spring sale");
        assert_eq!(validated.url, "https://example.com/landing?ref=newsletter");
        assert_eq!(validated.utm_source.as_deref(), Some("newsletter"));
        assert_eq!(validated.utm_medium, None);
        assert_eq!(validated.utm_campaign, None);
        assert_eq!(validated.utm_content, None);
        assert_eq!(validated.utm_term.as_deref(), Some("spring"));
    }

    #[test]
    fn validation_rejects_invalid_names() {
        let too_long = "a".repeat(101);
        for name in ["", "   ", "line\nbreak", "tab\there", "null\0byte", too_long.as_str()] {
            let candidate = UtmTagInput { display_name: name.to_string(), ..input(Some("newsletter")) };
            assert!(validate_utm_tag(candidate).is_err(), "name {name:?} should be rejected");
        }
        assert!(validate_utm_tag(UtmTagInput { display_name: "a".repeat(100), ..input(Some("newsletter")) }).is_ok());
    }

    #[test]
    fn validation_rejects_invalid_urls() {
        let too_long = format!("https://example.com/{}", "a".repeat(2048));
        for url in [
            "",
            "example.com/landing",
            "/landing",
            "ftp://example.com/landing",
            "javascript:alert(1)",
            "https://",
            "https:",
            too_long.as_str(),
        ] {
            let candidate = UtmTagInput { url: url.to_string(), ..input(Some("newsletter")) };
            assert!(validate_utm_tag(candidate).is_err(), "url {url:?} should be rejected");
        }
        let longest_allowed = format!("https://example.com/{}", "a".repeat(2048 - "https://example.com/".len()));
        assert!(validate_utm_tag(UtmTagInput { url: longest_allowed, ..input(Some("newsletter")) }).is_ok());
    }

    #[test]
    fn validation_rejects_invalid_utm_values() {
        let too_long = "a".repeat(201);
        for value in ["line\nbreak", "tab\there", too_long.as_str()] {
            let candidate = UtmTagInput { utm_source: Some(value.to_string()), ..input(None) };
            assert!(validate_utm_tag(candidate).is_err(), "value {value:?} should be rejected");
        }
        let longest_allowed = "a".repeat(200);
        assert!(validate_utm_tag(UtmTagInput { utm_source: Some(longest_allowed), ..input(None) }).is_ok());
    }

    #[test]
    fn validation_requires_at_least_one_utm_value() {
        let empty =
            UtmTagInput { display_name: "Bookmark".into(), url: "https://example.com".into(), ..Default::default() };
        let error = validate_utm_tag(empty).unwrap_err().to_string();
        assert!(error.contains("UTM"), "unexpected message: {error}");
    }

    #[test]
    fn tags_round_trip_through_the_store() {
        let app = app_with_project();
        let created = app.utm_tags.create("site", &input(Some("newsletter"))).unwrap();
        assert_eq!(created.id.len(), 16);
        assert!(created.id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(created.project_id, "site");
        assert_eq!(created.display_name, "Spring sale");

        assert_eq!(app.utm_tags.list("site").unwrap(), vec![created.clone()]);
        assert_eq!(app.utm_tags.get("site", &created.id).unwrap(), Some(created.clone()));

        let updated = app
            .utm_tags
            .update(
                "site",
                &created.id,
                &UtmTagInput {
                    display_name: "  Summer sale ".into(),
                    utm_campaign: Some("summer_sale".into()),
                    ..input(Some("google"))
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.display_name, "Summer sale");
        assert_eq!(updated.utm_source.as_deref(), Some("google"));
        assert_eq!(updated.utm_campaign.as_deref(), Some("summer_sale"));
        assert_eq!(app.utm_tags.get("site", &created.id).unwrap().unwrap(), updated);

        assert!(app.utm_tags.delete("site", &created.id).unwrap());
        assert!(!app.utm_tags.delete("site", &created.id).unwrap());
        assert!(app.utm_tags.list("site").unwrap().is_empty());
        assert_eq!(app.utm_tags.get("site", &created.id).unwrap(), None);
    }

    #[test]
    fn tags_are_scoped_to_their_project_and_ordered_by_name() {
        let app = app_with_project();
        app.projects
            .create(
                &Project {
                    id: "other".into(),
                    display_name: "Other".into(),
                    visibility: ProjectVisibility::Private,
                    secret: None,
                },
                &[],
            )
            .unwrap();
        let beta =
            app.utm_tags.create("site", &UtmTagInput { display_name: "beta".into(), ..input(Some("one")) }).unwrap();
        let alpha =
            app.utm_tags.create("site", &UtmTagInput { display_name: "Alpha".into(), ..input(Some("two")) }).unwrap();
        let same_name =
            app.utm_tags.create("site", &UtmTagInput { display_name: "alpha".into(), ..input(Some("three")) }).unwrap();

        let mut expected = vec![alpha.clone(), same_name.clone(), beta.clone()];
        expected.sort_by(|a, b| {
            a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()).then_with(|| a.id.cmp(&b.id))
        });
        assert_eq!(app.utm_tags.list("site").unwrap(), expected);

        assert_eq!(app.utm_tags.get("other", &beta.id).unwrap(), None);
        assert!(app.utm_tags.update("other", &beta.id, &input(Some("x"))).unwrap().is_none());
        assert!(!app.utm_tags.delete("other", &beta.id).unwrap());
        assert_eq!(app.utm_tags.get("site", &beta.id).unwrap(), Some(beta));
    }

    #[test]
    fn invalid_updates_leave_the_stored_tag_untouched() {
        let app = app_with_project();
        let created = app.utm_tags.create("site", &input(Some("newsletter"))).unwrap();
        let invalid = UtmTagInput { display_name: " ".into(), ..input(Some("newsletter")) };
        assert!(app.utm_tags.update("site", &created.id, &invalid).is_err());
        assert_eq!(app.utm_tags.get("site", &created.id).unwrap(), Some(created));
    }

    #[test]
    fn deleting_a_project_removes_its_tags() {
        let app = app_with_project();
        app.entities.create(&Entity { id: "entity".into(), display_name: "Entity".into() }, &[]).unwrap();
        app.utm_tags.create("site", &input(Some("newsletter"))).unwrap();
        app.projects.delete("site").unwrap();
        assert!(app.utm_tags.list("site").unwrap().is_empty());
    }
}
