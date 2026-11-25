use anyhow::Result;
use schemars::JsonSchema;
use serde::Serialize;

use crate::app::DuckDBConn;
use crate::app::models::UtmTag;
use crate::utils::duckdb::{ParamVec, repeat_vars};
use crate::utils::timestamp::SqlTimestamp;

use super::DateRange;

/// Pageviews and unique visitor groups for one saved tag in the selected range.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UtmTagSummary {
    pub id: String,
    /// Pageviews matching the tag's current UTM values.
    pub total_views: u64,
    /// Distinct visitor groups, not people. Groups rotate daily.
    pub unique_visitors: u64,
}

/// Summarize every tag in one grouped query, returning one row per input tag.
///
/// Each non-empty tag value is matched with `ilike`, the same rule the
/// dashboard's `equal` filter uses, so a row and the report it opens agree. An
/// unset value is ignored. Tags with no traffic keep their row with zeros.
pub fn utm_tag_summary(
    conn: &DuckDBConn,
    entities: &[String],
    tags: &[UtmTag],
    range: &DateRange,
) -> Result<Vec<UtmTagSummary>> {
    if tags.is_empty() {
        return Ok(Vec::new());
    }
    if entities.is_empty() {
        return Ok(tags.iter().map(|tag| UtmTagSummary { id: tag.id.clone(), ..Default::default() }).collect());
    }

    let mut params = ParamVec::new();
    // Text order: the two range bounds, then five values per tag, then the event
    // and the project's entity ids.
    params.push(SqlTimestamp(range.start));
    params.push(SqlTimestamp(range.end));
    let value_rows = (0..tags.len())
        .map(|index| format!("({index}, ?::varchar, ?::varchar, ?::varchar, ?::varchar, ?::varchar)"))
        .collect::<Vec<_>>()
        .join(", ");
    for tag in tags {
        params.push(tag.utm_source.clone());
        params.push(tag.utm_medium.clone());
        params.push(tag.utm_campaign.clone());
        params.push(tag.utm_content.clone());
        params.push(tag.utm_term.clone());
    }
    params.push("pageview");
    params.extend(entities.iter().cloned());
    let entity_vars = repeat_vars(entities.len());

    let query = format!(
        "--sql
        with
            range_events as (
                select entity_id, event, visitor_group_id, created_at,
                    utm_source, utm_medium, utm_campaign, utm_content, utm_term
                from events
                where created_at >= ?::timestamp and created_at < ?::timestamp
            ),
            tag_filters(tag_idx, source, medium, campaign, content, term) as (
                values {value_rows}
            ),
            matched as (
                select f.tag_idx, e.created_at, e.visitor_group_id
                from tag_filters f
                join range_events e on
                    e.event = ?::text
                    and e.entity_id in ({entity_vars})
                    and (f.source is null or e.utm_source ilike f.source)
                    and (f.medium is null or e.utm_medium ilike f.medium)
                    and (f.campaign is null or e.utm_campaign ilike f.campaign)
                    and (f.content is null or e.utm_content ilike f.content)
                    and (f.term is null or e.utm_term ilike f.term)
            )
        select f.tag_idx, count(m.created_at), count(distinct m.visitor_group_id)
        from tag_filters f
        left join matched m on m.tag_idx = f.tag_idx
        group by f.tag_idx
        order by f.tag_idx
    "
    );

    let mut summaries: Vec<UtmTagSummary> =
        tags.iter().map(|tag| UtmTagSummary { id: tag.id.clone(), ..Default::default() }).collect();
    let mut stmt = conn.prepare_cached(&query)?;
    let mut rows = stmt.query(duckdb::params_from_iter(params))?;
    while let Some(row) = rows.next()? {
        let index = row.get::<_, usize>(0)?;
        if let Some(summary) = summaries.get_mut(index) {
            summary.total_views = row.get(1)?;
            summary.unique_visitors = row.get(2)?;
        }
    }
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Liwan;
    use crate::app::models::{FilterType, UtmTag};
    use crate::app::reports::{Dimension, DimensionFilter, overall_stats};
    use crate::config::{Config, LimitsConfig};
    use jiff::{SignedDuration, Timestamp};

    fn tag(id: &str, source: Option<&str>, medium: Option<&str>, campaign: Option<&str>) -> UtmTag {
        UtmTag {
            id: id.into(),
            project_id: "project".into(),
            display_name: id.into(),
            url: "https://example.com".into(),
            utm_source: source.map(str::to_string),
            utm_medium: medium.map(str::to_string),
            utm_campaign: campaign.map(str::to_string),
            utm_content: None,
            utm_term: None,
        }
    }

    type Event<'a> = (&'a str, &'a str, Option<&'a str>, Option<&'a str>, Option<&'a str>);

    fn insert(conn: &DuckDBConn, time: Timestamp, (visitor, event, source, medium, campaign): Event<'_>) {
        conn.execute(
            "insert into events (entity_id, visitor_group_id, event, created_at, utm_source, utm_medium, utm_campaign)
             values ('entity-1', ?, ?, ?, ?, ?, ?)",
            duckdb::params![visitor, event, SqlTimestamp(time), source, medium, campaign],
        )
        .unwrap();
    }

    fn range(time: Timestamp) -> DateRange {
        DateRange { start: time - SignedDuration::from_hours(1), end: time + SignedDuration::from_hours(1) }
    }

    #[test]
    fn matches_all_five_values_case_insensitively() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let time = Timestamp::now();
        insert(&conn, time, ("v1", "pageview", Some("google"), Some("cpc"), Some("summer_sale")));
        insert(&conn, time, ("v2", "pageview", Some("Google"), Some("CPC"), Some("SUMMER_SALE")));
        insert(&conn, time, ("v1", "pageview", Some("google"), Some("cpc"), Some("summer_sale")));
        insert(&conn, time, ("v3", "pageview", Some("google"), Some("cpc"), Some("other")));

        let tags = [tag("all", Some("google"), Some("cpc"), Some("summer_sale"))];
        let summaries = utm_tag_summary(&conn, &["entity-1".into()], &tags, &range(time)).unwrap();
        assert_eq!(summaries, vec![UtmTagSummary { id: "all".into(), total_views: 3, unique_visitors: 2 }]);
    }

    #[test]
    fn a_partial_tag_matches_every_other_value() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let time = Timestamp::now();
        insert(&conn, time, ("v1", "pageview", Some("google"), Some("cpc"), Some("summer_sale")));
        insert(&conn, time, ("v2", "pageview", Some("google"), Some("organic"), None));
        insert(&conn, time, ("v3", "pageview", Some("newsletter"), None, None));

        let tags = [tag("source", Some("google"), None, None)];
        let summaries = utm_tag_summary(&conn, &["entity-1".into()], &tags, &range(time)).unwrap();
        assert_eq!(summaries, vec![UtmTagSummary { id: "source".into(), total_views: 2, unique_visitors: 2 }]);
    }

    #[test]
    fn tags_without_traffic_keep_a_zeroed_row() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let time = Timestamp::now();
        insert(&conn, time, ("v1", "pageview", Some("google"), None, None));

        let tags = [tag("none", Some("missing"), None, None), tag("some", Some("google"), None, None)];
        let summaries = utm_tag_summary(&conn, &["entity-1".into()], &tags, &range(time)).unwrap();
        assert_eq!(
            summaries,
            vec![
                UtmTagSummary { id: "none".into(), total_views: 0, unique_visitors: 0 },
                UtmTagSummary { id: "some".into(), total_views: 1, unique_visitors: 1 },
            ]
        );
    }

    #[test]
    fn overlapping_tags_each_count_the_traffic_they_match() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let time = Timestamp::now();
        insert(&conn, time, ("v1", "pageview", Some("google"), Some("cpc"), Some("summer_sale")));
        insert(&conn, time, ("v2", "pageview", Some("google"), Some("cpc"), Some("winter_sale")));
        insert(&conn, time, ("v3", "pageview", Some("newsletter"), Some("email"), Some("summer_sale")));

        let tags = [tag("source", Some("google"), None, None), tag("campaign", None, None, Some("summer_sale"))];
        let summaries = utm_tag_summary(&conn, &["entity-1".into()], &tags, &range(time)).unwrap();
        assert_eq!(
            summaries,
            vec![
                UtmTagSummary { id: "source".into(), total_views: 2, unique_visitors: 2 },
                UtmTagSummary { id: "campaign".into(), total_views: 2, unique_visitors: 2 },
            ]
        );
    }

    #[test]
    fn custom_events_and_other_entities_are_excluded() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let time = Timestamp::now();
        insert(&conn, time, ("v1", "pageview", Some("google"), None, None));
        insert(&conn, time, ("v1", "signup", Some("google"), None, None));
        conn.execute(
            "insert into events (entity_id, visitor_group_id, event, created_at, utm_source)
             values ('entity-2', 'v9', 'pageview', ?, 'google')",
            duckdb::params![SqlTimestamp(time)],
        )
        .unwrap();

        let tags = [tag("source", Some("google"), None, None)];
        let summaries = utm_tag_summary(&conn, &["entity-1".into()], &tags, &range(time)).unwrap();
        assert_eq!(summaries, vec![UtmTagSummary { id: "source".into(), total_views: 1, unique_visitors: 1 }]);
    }

    #[test]
    fn summary_agrees_with_the_equivalent_report_filters() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let time = Timestamp::now();
        insert(&conn, time, ("v1", "pageview", Some("google"), Some("cpc"), Some("summer_sale")));
        insert(&conn, time, ("v1", "pageview", Some("google"), Some("cpc"), Some("summer_sale")));
        insert(&conn, time, ("v2", "pageview", Some("google"), Some("cpc"), Some("summer_sale")));
        insert(&conn, time, ("v3", "pageview", Some("google"), Some("cpc"), Some("other")));

        let mut long = tag("all", Some("google"), Some("cpc"), Some("summer_sale"));
        long.utm_content = Some("banner".into());
        long.utm_term = Some("shoes".into());
        // Content and term are unset on the events, so the tag has no matches.
        let summaries = utm_tag_summary(&conn, &["entity-1".into()], &[long.clone()], &range(time)).unwrap();
        assert_eq!(summaries[0].total_views, 0);
        assert_eq!(summaries[0].unique_visitors, 0);

        let filters = [("utm_source", "google"), ("utm_medium", "cpc"), ("utm_campaign", "summer_sale")].map(
            |(dimension, value)| DimensionFilter {
                dimension: match dimension {
                    "utm_source" => Dimension::UtmSource,
                    "utm_medium" => Dimension::UtmMedium,
                    _ => Dimension::UtmCampaign,
                },
                key: None,
                filter_type: FilterType::Equal,
                inversed: None,
                strict: None,
                value: Some(value.into()),
            },
        );
        let summary = utm_tag_summary(
            &conn,
            &["entity-1".into()],
            &[tag("three", Some("google"), Some("cpc"), Some("summer_sale"))],
            &range(time),
        )
        .unwrap()
        .remove(0);
        let stats =
            overall_stats(&conn, &["entity-1".into()], "pageview", &[range(time)], &filters, &LimitsConfig::default())
                .unwrap()
                .remove(0);
        assert_eq!(summary.total_views, stats.total_views);
        assert_eq!(summary.unique_visitors, stats.unique_visitors);
        assert_eq!(summary.total_views, 3);
        assert_eq!(summary.unique_visitors, 2);
    }

    #[test]
    fn empty_inputs_return_one_zeroed_row_per_tag() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let time = Timestamp::now();
        let tags = [tag("one", Some("google"), None, None), tag("two", Some("newsletter"), None, None)];
        assert_eq!(utm_tag_summary(&conn, &[], &tags, &range(time)).unwrap().len(), 2);
        assert!(utm_tag_summary(&conn, &["entity-1".into()], &[], &range(time)).unwrap().is_empty());
    }
}
