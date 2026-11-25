use anyhow::Result;
use schemars::JsonSchema;
use serde::Serialize;

use crate::app::DuckDBConn;
use crate::app::models::{FunnelMatch, FunnelStep};
use crate::utils::duckdb::{ParamVec, repeat_vars};

use super::shared::{build_filter_clause, range_events_sql, report_columns};
use super::{DateRange, DimensionFilter};

/// Cumulative visitor groups that reached each step of a funnel.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunnelReport {
    /// One cumulative count per step, in step order.
    pub counts: Vec<u64>,
}

/// Count the visitor groups that complete every step in order, once per range.
pub fn funnel_report(
    conn: &DuckDBConn,
    entities: &[String],
    steps: &[FunnelStep],
    ranges: &[DateRange],
    filters: &[DimensionFilter],
) -> Result<Vec<FunnelReport>> {
    let mut reports: Vec<_> = ranges.iter().map(|_| FunnelReport { counts: vec![0; steps.len()] }).collect();
    if entities.is_empty() || ranges.is_empty() || steps.is_empty() {
        return Ok(reports);
    }

    let entity_vars = repeat_vars(entities.len());
    let (filters_sql, filters_params) = build_filter_clause(filters)?;
    let mut params = ParamVec::new();
    let columns = report_columns(&["entity_id", "event", "created_at", "visitor_group_id", "path"], None, filters);
    let range_events = range_events_sql(ranges, &mut params, &columns);
    params.extend(entities);
    params.extend_from_params(filters_params);

    // One branch per step selects the events that satisfy it. The union reads the
    // materialized candidates, so the base table is scanned once per range.
    let mut matches = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        let step_idx = index + 1;
        let branch = match step {
            FunnelStep::Page { path, match_type } => {
                params.push(path.clone());
                let comparison = match match_type {
                    FunnelMatch::Equal => "path ilike ?",
                    FunnelMatch::Contains => "path ilike '%' || ? || '%'",
                    FunnelMatch::StartsWith => "path ilike ? || '%'",
                };
                format!(
                    "select range_idx, visitor_group_id, created_at, {step_idx} as step_idx
                     from candidates where event = 'pageview' and {comparison}"
                )
            }
            FunnelStep::Event { name } => {
                params.push(name.clone());
                format!(
                    "select range_idx, visitor_group_id, created_at, {step_idx} as step_idx
                     from candidates where event = ?"
                )
            }
        };
        matches.push(branch);
    }

    let mut step_ctes = Vec::with_capacity(steps.len());
    step_ctes.push(
        "step_1 as (
            select range_idx, visitor_group_id, min(created_at) as matched_at
            from step_matches where step_idx = 1
            group by range_idx, visitor_group_id
         )"
        .to_string(),
    );
    for step_idx in 2..=steps.len() {
        let previous = step_idx - 1;
        step_ctes.push(format!(
            "step_{step_idx} as (
                select s.range_idx, s.visitor_group_id, min(m.created_at) as matched_at
                from step_{previous} s
                join step_matches m
                  on m.range_idx = s.range_idx
                 and m.visitor_group_id = s.visitor_group_id
                 and m.step_idx = {step_idx}
                 and m.created_at > s.matched_at
                group by s.range_idx, s.visitor_group_id
             )"
        ));
    }

    let count_branches = (1..=steps.len())
        .map(|step_idx| {
            format!(
                "select {step_idx} as step_idx, range_idx, count(*) as matched from step_{step_idx} group by range_idx"
            )
        })
        .collect::<Vec<_>>()
        .join(" union all ");

    let sql = format!(
        "with candidates as materialized (
             select range_idx, event, created_at, visitor_group_id, path
             from ({range_events}) range_events
             where entity_id in ({entity_vars}) {filters_sql}
         ),
         step_matches as materialized (
             {}
         ),
         {},
         step_counts as ({count_branches})
         select range_idx, step_idx, matched from step_counts",
        matches.join(" union all "),
        step_ctes.join(", ")
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(duckdb::params_from_iter(params))?;
    while let Some(row) = rows.next()? {
        let range_idx = row.get::<_, usize>(0)?;
        let step_idx = row.get::<_, usize>(1)?;
        let matched = row.get::<_, u64>(2)?;
        if let Some(report) = reports.get_mut(range_idx)
            && let Some(count) = report.counts.get_mut(step_idx.saturating_sub(1))
        {
            *count = matched;
        }
    }

    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Liwan;
    use crate::app::models::FilterType;
    use crate::app::reports::Dimension;
    use crate::config::Config;
    use crate::utils::timestamp::SqlTimestamp;
    use duckdb::params;
    use jiff::{SignedDuration, Timestamp};

    fn step_page(path: &str, match_type: FunnelMatch) -> FunnelStep {
        FunnelStep::Page { path: path.into(), match_type }
    }

    fn step_event(name: &str) -> FunnelStep {
        FunnelStep::Event { name: name.into() }
    }

    fn base() -> Timestamp {
        Timestamp::from_second(1_700_000_000).unwrap()
    }

    fn range(base: Timestamp) -> DateRange {
        DateRange { start: base - SignedDuration::from_hours(1), end: base + SignedDuration::from_hours(24) }
    }

    fn filter(dimension: Dimension, value: &str) -> DimensionFilter {
        DimensionFilter {
            dimension,
            key: None,
            filter_type: FilterType::Equal,
            inversed: None,
            strict: None,
            value: Some(value.into()),
        }
    }

    fn insert(conn: &DuckDBConn, entity: &str, visitor: &str, event: &str, path: &str, time: Timestamp) {
        conn.execute(
            "insert into events (entity_id, visitor_group_id, event, created_at, path) values (?, ?, ?, ?, ?)",
            params![entity, visitor, event, SqlTimestamp(time), path],
        )
        .unwrap();
    }

    fn page(conn: &DuckDBConn, visitor: &str, path: &str, time: Timestamp) {
        insert(conn, "a", visitor, "pageview", path, time);
    }

    fn event(conn: &DuckDBConn, visitor: &str, name: &str, time: Timestamp) {
        insert(conn, "a", visitor, name, "/ignored", time);
    }

    fn counts(
        conn: &DuckDBConn,
        steps: &[FunnelStep],
        ranges: &[DateRange],
        filters: &[DimensionFilter],
    ) -> Vec<Vec<u64>> {
        funnel_report(conn, &["a".into()], steps, ranges, filters)
            .unwrap()
            .into_iter()
            .map(|report| report.counts)
            .collect()
    }

    #[test]
    fn three_step_funnel_counts_groups_stopping_at_each_step() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        // Completes every step.
        page(&conn, "one", "/pricing", base);
        event(&conn, "one", "signup", base + SignedDuration::from_mins(1));
        event(&conn, "one", "purchase", base + SignedDuration::from_mins(2));
        // Stops at step 2.
        page(&conn, "two", "/pricing", base + SignedDuration::from_hours(1));
        event(&conn, "two", "signup", base + SignedDuration::from_hours(1) + SignedDuration::from_mins(1));
        // Stops at step 1.
        page(&conn, "three", "/pricing", base + SignedDuration::from_hours(2));
        // Never matches step 1.
        event(&conn, "four", "signup", base + SignedDuration::from_hours(3));
        event(&conn, "four", "purchase", base + SignedDuration::from_hours(3) + SignedDuration::from_mins(1));

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup"), step_event("purchase")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![3, 2, 1]]);
    }

    #[test]
    fn out_of_order_events_do_not_count() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        event(&conn, "one", "purchase", base);
        event(&conn, "one", "signup", base + SignedDuration::from_mins(1));
        page(&conn, "one", "/pricing", base + SignedDuration::from_mins(2));
        event(&conn, "one", "signup", base + SignedDuration::from_mins(3));

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup"), step_event("purchase")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![1, 1, 0]]);

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("purchase")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![1, 0]]);
    }

    #[test]
    fn several_matching_events_count_a_group_once() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        page(&conn, "one", "/pricing", base);
        page(&conn, "one", "/pricing", base + SignedDuration::from_mins(1));
        page(&conn, "one", "/pricing", base + SignedDuration::from_mins(2));
        event(&conn, "one", "signup", base + SignedDuration::from_mins(3));
        event(&conn, "one", "signup", base + SignedDuration::from_mins(4));

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![1, 1]]);
    }

    #[test]
    fn same_timestamp_events_do_not_satisfy_consecutive_steps() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        page(&conn, "one", "/pricing", base);
        event(&conn, "one", "signup", base);

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![1, 0]]);
    }

    #[test]
    fn a_broad_step_still_requires_a_second_event() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        page(&conn, "one", "/pricing", base);

        let steps = [step_page("/", FunnelMatch::Contains), step_page("/pricing", FunnelMatch::Equal)];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![1, 0]]);

        page(&conn, "one", "/pricing", base + SignedDuration::from_mins(1));
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![1, 1]]);
    }

    #[test]
    fn every_range_gets_its_own_counts() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        page(&conn, "one", "/pricing", base);
        event(&conn, "one", "signup", base + SignedDuration::from_mins(1));
        page(&conn, "two", "/pricing", base + SignedDuration::from_hours(24));
        event(&conn, "two", "signup", base + SignedDuration::from_hours(24) + SignedDuration::from_mins(1));
        event(&conn, "two", "purchase", base + SignedDuration::from_hours(24) + SignedDuration::from_mins(2));

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup")];
        let ranges = [
            DateRange { start: base - SignedDuration::from_hours(1), end: base + SignedDuration::from_hours(1) },
            DateRange { start: base + SignedDuration::from_hours(23), end: base + SignedDuration::from_hours(48) },
        ];
        assert_eq!(counts(&conn, &steps, &ranges, &[]), vec![vec![1, 1], vec![1, 1]]);
        // The response follows the request order, not the range order.
        assert_eq!(counts(&conn, &steps, &[ranges[1].clone(), ranges[0].clone()], &[]), vec![vec![1, 1], vec![1, 1]]);
    }

    #[test]
    fn events_outside_the_range_are_ignored() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        page(&conn, "one", "/pricing", base - SignedDuration::from_hours(24));
        event(&conn, "one", "signup", base);
        page(&conn, "two", "/pricing", base);
        event(&conn, "two", "signup", base + SignedDuration::from_hours(24));

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![1, 0]]);
    }

    #[test]
    fn filters_apply_to_every_step() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        // The page is in the US but the later events are not.
        insert(&conn, "a", "one", "pageview", "/pricing", base);
        conn.execute("update events set country = 'US' where visitor_group_id = 'one' and event = 'pageview'", [])
            .unwrap();
        insert(&conn, "a", "one", "signup", "/ignored", base + SignedDuration::from_mins(1));
        conn.execute("update events set country = 'DE' where event = 'signup'", []).unwrap();
        // Every event of this group is in the US.
        insert(&conn, "a", "two", "pageview", "/pricing", base + SignedDuration::from_hours(1));
        insert(
            &conn,
            "a",
            "two",
            "signup",
            "/ignored",
            base + SignedDuration::from_hours(1) + SignedDuration::from_mins(1),
        );
        conn.execute("update events set country = 'US' where visitor_group_id = 'two'", []).unwrap();

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![2, 2]]);
        assert_eq!(counts(&conn, &steps, &[range(base)], &[filter(Dimension::Country, "US")]), vec![vec![2, 1]]);
        assert_eq!(counts(&conn, &steps, &[range(base)], &[filter(Dimension::Country, "DE")]), vec![vec![0, 0]]);
    }

    #[test]
    fn page_steps_support_every_match_type() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        page(&conn, "one", "/pricing", base);
        event(&conn, "one", "signup", base + SignedDuration::from_mins(1));
        page(&conn, "two", "/products/item", base);
        event(&conn, "two", "signup", base + SignedDuration::from_mins(1));
        page(&conn, "three", "/blog/post", base);
        event(&conn, "three", "signup", base + SignedDuration::from_mins(1));
        // Matching is case-insensitive, like a dashboard filter.
        page(&conn, "four", "/PRICING", base);
        event(&conn, "four", "signup", base + SignedDuration::from_mins(1));

        let after = step_event("signup");
        for (match_type, path, expected) in [
            (FunnelMatch::Equal, "/pricing", 2),
            (FunnelMatch::Contains, "products", 1),
            (FunnelMatch::StartsWith, "/blog", 1),
        ] {
            let steps = [step_page(path, match_type), after.clone()];
            assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![expected, expected]]);
        }
    }

    #[test]
    fn event_steps_match_the_exact_name() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        for (visitor, name) in [("one", "signup"), ("two", "Signup"), ("three", "signup2"), ("four", "sign up")] {
            page(&conn, visitor, "/pricing", base);
            event(&conn, visitor, name, base + SignedDuration::from_mins(1));
        }

        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup")];
        assert_eq!(counts(&conn, &steps, &[range(base)], &[]), vec![vec![4, 1]]);
    }

    #[test]
    fn empty_inputs_return_zeroed_counts() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        let base = base();
        page(&conn, "one", "/pricing", base);
        let steps = [step_page("/pricing", FunnelMatch::Equal), step_event("signup"), step_event("purchase")];

        assert_eq!(funnel_report(&conn, &[], &steps, &[range(base)], &[]).unwrap().remove(0).counts, vec![0, 0, 0]);
        assert!(funnel_report(&conn, &["a".into()], &steps, &[], &[]).unwrap().is_empty());
        let empty =
            DateRange { start: base - SignedDuration::from_hours(48), end: base - SignedDuration::from_hours(24) };
        assert_eq!(funnel_report(&conn, &["a".into()], &steps, &[empty], &[]).unwrap().remove(0).counts, vec![0, 0, 0]);
    }
}
