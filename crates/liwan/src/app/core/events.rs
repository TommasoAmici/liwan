use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use chrono::{DateTime, Local, NaiveTime, TimeZone, Utc};
use duckdb::{Connection, Result as DuckResult, params};
use futures_util::{FutureExt, StreamExt};
use rand::distr::{SampleString, StandardUniform};
use tokio::sync::mpsc::Receiver;
use tokio_util::time::DelayQueue;

use crate::app::models::{CustomData, Event, EventExit, GeoDetail, ResolvedCollectionSettings, event_params};
use crate::app::{DuckDBPool, SqlitePool};
use crate::utils::duckdb::{ParamVec, repeat_vars};

const EVENT_EXIT_DELAY: std::time::Duration = std::time::Duration::from_secs(30);

/// Exits are updated through a values list; chunk it so one flush cannot build
/// an unbounded parameter list.
const EXIT_CHUNK: usize = 1000;

#[derive(Clone)]
pub struct LiwanEvents {
    duckdb: DuckDBPool,
    sqlite: SqlitePool,
    daily_salt: Arc<ArcSwap<(String, DateTime<Utc>)>>,
    visitor_group_rotation_hour: u8,
    /// Per-entity cache for [`LiwanEvents::custom_data`], updated as events are appended
    custom_data: Arc<Mutex<HashMap<String, CustomData>>>,
}

#[derive(Debug, Clone, Default)]
pub struct PruneStats {
    pub total_events: u64,
    pub deleted_events: u64,
    pub cleared_utm_events: u64,
    pub cleared_geo_events: u64,
    pub cleared_session_events: u64,
}

impl LiwanEvents {
    pub fn try_new(duckdb: DuckDBPool, sqlite: SqlitePool, visitor_group_rotation_hour: u8) -> Result<Self> {
        let daily_salt: (String, DateTime<Utc>) = {
            tracing::debug!("Loading visitor group salt");
            sqlite.get()?.query_row("select salt, updated_at from salts where id = 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
        };
        Ok(Self {
            duckdb,
            sqlite,
            daily_salt: ArcSwap::new(daily_salt.into()).into(),
            visitor_group_rotation_hour,
            custom_data: Default::default(),
        })
    }

    /// Get the visitor group salt, generating a new one after the daily local rotation time
    pub fn get_salt(&self) -> Result<String> {
        let (salt, updated_at) = &**self.daily_salt.load();

        if should_rotate_salt(*updated_at, self.visitor_group_rotation_hour) {
            tracing::debug!("Visitor group salt expired, generating a new one");
            let new_salt = StandardUniform.sample_string(&mut rand::rng(), 16);
            let now = Utc::now();
            let conn = self.sqlite.get()?;
            conn.execute(
                "update salts set salt = :salt, updated_at = :updated_at where id = 1",
                rusqlite::named_params! { ":salt": &new_salt, ":updated_at": now },
            )?;
            self.daily_salt.store((new_salt.clone(), now).into());
            Ok(new_salt)
        } else {
            Ok(salt.clone())
        }
    }

    /// Return whether any of the entities has recorded custom events or custom properties.
    ///
    /// Each entity is queried once and then kept up to date by [`LiwanEvents::append`].
    pub fn custom_data(&self, entities: &[String]) -> Result<CustomData> {
        let mut result = CustomData::default();
        for entity in entities {
            let cached = self.custom_data.lock().expect("custom data lock").get(entity).copied();
            let data = match cached {
                Some(data) => data,
                None => {
                    let data = self.duckdb.get()?.query_row(
                        "select
                            exists(select 1 from events where entity_id = $1 and event <> 'pageview'),
                            exists(select 1 from events where entity_id = $1 and properties is not null)",
                        params![entity],
                        |row| Ok(CustomData { events: row.get(0)?, properties: row.get(1)? }),
                    )?;
                    self.custom_data.lock().expect("custom data lock").insert(entity.clone(), data);
                    data
                }
            };
            result.events |= data.events;
            result.properties |= data.properties;
        }
        Ok(result)
    }

    /// Append events in a batch and update session timing fields when needed
    pub fn append(&self, events: impl Iterator<Item = Event>) -> Result<()> {
        let conn = self.duckdb.get()?;
        let mut first_event_time = None;
        let mut session_visitors: ahash::HashSet<(String, String)> = ahash::HashSet::default();
        let mut custom_data = Vec::new();
        let mut appender = conn.appender("events").context("Failed to get DuckDB appender")?;
        for event in events {
            if event.event != "pageview" || !event.properties.is_empty() {
                custom_data.push((event.entity_id.clone(), event.event != "pageview", !event.properties.is_empty()));
            }
            if event.track_sessions && event.event == "pageview" {
                first_event_time =
                    Some(first_event_time.map_or(event.created_at, |time: DateTime<Utc>| time.min(event.created_at)));
                session_visitors.insert((event.entity_id.clone(), event.visitor_group_id.clone()));
            }
            appender.append_row(event_params![event]).context("Failed to append event to DuckDB")?;
        }

        appender.flush().context("Failed to flush events to DuckDB")?;
        // only update entities that are already cached; the others are queried on first use
        let mut cache = self.custom_data.lock().expect("custom data lock");
        for (entity, events, properties) in custom_data {
            if let Some(data) = cache.get_mut(&entity) {
                data.events |= events;
                data.properties |= properties;
            }
        }
        drop(cache);
        if let Some(first_event_time) = first_event_time {
            update_event_times(&conn, first_event_time, &session_visitors)
                .context("Failed to update event times in DuckDB")?;
        }
        Ok(())
    }

    /// Start processing events from the given channel. Blocks until the channel is closed
    pub async fn process_events(&self, events_rx: Receiver<Event>) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let events = self.clone();
        std::thread::spawn(move || {
            let res = events.process_events_sync(events_rx).context("Event processing task failed");
            let _ = tx.send(res);
        });
        rx.await??;
        Ok(())
    }

    /// Process exit updates after a short delay so queued events can reach shared storage first.
    pub async fn process_exits(&self, mut exits_rx: Receiver<EventExit>) -> Result<()> {
        let mut pending = DelayQueue::new();
        let mut channel_closed = false;

        loop {
            if channel_closed && pending.is_empty() {
                tracing::info!("Event exit channel closed, stopping event exit processing");
                return Ok(());
            }

            tokio::select! {
                exit = exits_rx.recv(), if !channel_closed => match exit {
                    Some(exit) => {
                        pending.insert(exit, EVENT_EXIT_DELAY);
                    }
                    None => channel_closed = true,
                },
                Some(expired) = pending.next(), if !pending.is_empty() => {
                    let mut exits = vec![expired.into_inner()];
                    while let Some(Some(expired)) = pending.next().now_or_never() {
                        exits.push(expired.into_inner());
                    }

                    let count = exits.len();
                    let events = self.clone();
                    match tokio::task::spawn_blocking(move || events.update_exits(exits)).await? {
                        Ok(matched) => tracing::debug!(count, matched, "Processed event exits"),
                        Err(err) => tracing::error!(?err, "Failed to process event exits"),
                    }
                }
            }
        }
    }

    fn process_events_sync(&self, mut events: Receiver<Event>) -> Result<()> {
        let mut buffer = Vec::with_capacity(512);

        loop {
            let event_count = events.blocking_recv_many(&mut buffer, 512);
            if event_count == 0 {
                tracing::info!("Event channel closed, stopping event processing");
                break Ok(());
            }

            match self.append(buffer.drain(..)) {
                Err(err) => tracing::error!(?err, "Failed to process events"),
                Ok(()) => tracing::debug!(event_count, "Processed events"),
            }
        }
    }

    /// Apply delayed exit timestamps in one transaction and return how many
    /// exits updated a row.
    /// Apply delayed exit timestamps in one transaction and return how many
    /// exits updated a row.
    pub fn update_exits(&self, exits: Vec<EventExit>) -> Result<usize> {
        let conn = self.duckdb.get().context("Failed to get DuckDB connection")?;
        let tx = conn.unchecked_transaction().context("Failed to start the exit transaction")?;
        let mut matched = 0;
        for chunk in exits.chunks(EXIT_CHUNK) {
            let mut params = ParamVec::new();
            let values = chunk
                .iter()
                .enumerate()
                .map(|(index, exit)| {
                    params.push(index as i64);
                    params.push(exit.entity_id.as_str());
                    params.push(exit.visitor_group_id.as_str());
                    params.push(exit.event.as_str());
                    params.push(exit.fqdn.as_deref());
                    params.push(exit.path.as_deref());
                    params.push(exit.created_at);
                    "(?::bigint, ?::text, ?::text, ?::text, ?::text, ?::text, ?::timestamp)"
                })
                .collect::<Vec<_>>()
                .join(", ");

            let sql = format!(
                "--sql
                with
                    exits(exit_idx, entity_id, visitor_group_id, event, fqdn, path, exited_at) as (values {values}),
                    latest as (
                        select e.rowid, x.exit_idx, x.exited_at
                        from exits x
                        join events e on
                            e.entity_id = x.entity_id and
                            e.visitor_group_id = x.visitor_group_id and
                            e.event = x.event and
                            e.fqdn is not distinct from x.fqdn and
                            e.path is not distinct from x.path and
                            e.time_to_next_event is null and
                            e.created_at <= x.exited_at and
                            e.created_at >= x.exited_at - interval '30 minutes'
                        qualify row_number() over (partition by x.exit_idx order by e.created_at desc) = 1
                    )
                update events
                    set exited_at = greatest(
                        coalesce(events.exited_at, events.created_at),
                        least(latest.exited_at, events.created_at + interval '30 minutes')
                    )
                    from latest
                    where events.rowid = latest.rowid;
            "
            );
            matched += usize::try_from(tx.execute(&sql, duckdb::params_from_iter(params))?)
                .context("exit update count is too large")?;
        }
        tx.commit().context("Failed to commit event exits")?;
        Ok(matched)
    }

    /// Preview or apply collection-setting pruning for a single entity
    pub fn prune_entity(
        &self,
        entity_id: &str,
        settings: &ResolvedCollectionSettings,
        dry_run: bool,
    ) -> Result<PruneStats> {
        let conn = self.duckdb.get()?;
        let mut stats = PruneStats {
            total_events: count_rows(&conn, "select count(*) from events where entity_id = ?", params![entity_id])?,
            ..Default::default()
        };

        if let crate::app::models::DataRetention::Days(data_retention_days) = settings.data_retention {
            let cutoff = Utc::now() - chrono::Duration::days(i64::from(data_retention_days.get()));
            stats.deleted_events = count_rows(
                &conn,
                "select count(*) from events where entity_id = $entity_id and created_at < $cutoff::timestamp",
                duckdb::named_params! { "entity_id": entity_id, "cutoff": cutoff },
            )?;
            if !dry_run {
                conn.execute(
                    "delete from events where entity_id = $entity_id and created_at < $cutoff::timestamp",
                    duckdb::named_params! { "entity_id": entity_id, "cutoff": cutoff },
                )?;
            }
        }

        if !settings.track_utm_params {
            let sql = "entity_id = ? and (utm_source is not null or utm_medium is not null or utm_campaign is not null or utm_content is not null or utm_term is not null)";
            stats.cleared_utm_events =
                count_rows(&conn, &format!("select count(*) from events where {sql}"), params![entity_id])?;
            if !dry_run {
                conn.execute(
                    &format!(
                        "update events set utm_source = null, utm_medium = null, utm_campaign = null, utm_content = null, utm_term = null where {sql}"
                    ),
                    params![entity_id],
                )?;
            }
        }

        match settings.track_geo {
            GeoDetail::None => {
                let sql = "entity_id = ? and (country is not null or city is not null)";
                stats.cleared_geo_events =
                    count_rows(&conn, &format!("select count(*) from events where {sql}"), params![entity_id])?;
                if !dry_run {
                    conn.execute(
                        &format!("update events set country = null, city = null where {sql}"),
                        params![entity_id],
                    )?;
                }
            }
            GeoDetail::Country => {
                let sql = "entity_id = ? and city is not null";
                stats.cleared_geo_events =
                    count_rows(&conn, &format!("select count(*) from events where {sql}"), params![entity_id])?;
                if !dry_run {
                    conn.execute(&format!("update events set city = null where {sql}"), params![entity_id])?;
                }
            }
            GeoDetail::City => {}
        }

        if !settings.track_sessions {
            let sql = "entity_id = ? and (time_from_last_event is not null or time_to_next_event is not null or exited_at is not null)";
            stats.cleared_session_events =
                count_rows(&conn, &format!("select count(*) from events where {sql}"), params![entity_id])?;
            if !dry_run {
                conn.execute(
                    &format!(
                        "update events set time_from_last_event = null, time_to_next_event = null, exited_at = null where {sql}"
                    ),
                    params![entity_id],
                )?;
            }
        }

        Ok(stats)
    }
}

fn should_rotate_salt(updated_at: DateTime<Utc>, rotation_hour: u8) -> bool {
    let now = Local::now();
    let rotation_time = NaiveTime::from_hms_opt(u32::from(rotation_hour.min(23)), 0, 0).expect("valid rotation hour");
    let local_rotation = now.date_naive().and_time(rotation_time);
    let latest_rotation = match Local.from_local_datetime(&local_rotation) {
        chrono::LocalResult::Single(rotation) => rotation,
        chrono::LocalResult::Ambiguous(earlier, later) => earlier.min(later),
        chrono::LocalResult::None => now,
    };
    let latest_rotation =
        if now < latest_rotation { latest_rotation - chrono::Duration::days(1) } else { latest_rotation };

    updated_at < latest_rotation.with_timezone(&Utc)
}

fn count_rows(conn: &Connection, sql: &str, params: impl duckdb::Params) -> DuckResult<u64> {
    conn.query_row(sql, params, |row| row.get(0))
}

/// At or below this many changed visitors, the interval update touches only
/// their rows. A larger append (a bulk import, or the development seed) uses
/// one scan of the batch's entities and time span instead: a values list of
/// millions of visitors would be larger and slower than the scan it replaces.
const MAX_VISITOR_SCOPE: usize = 4096;

/// Recompute the intervals around the pageviews a batch just appended.
///
/// Only a visitor's previous pageview and the new pageview can change, so the
/// common path looks back a session's length from each changed visitor. A gap
/// longer than that stays null, which every report already treats like a gap
/// longer than 30 minutes (`SESSION_DURATION_SQL`).
fn update_event_times(
    conn: &Connection,
    from_time: DateTime<Utc>,
    changed: &ahash::HashSet<(String, String)>,
) -> DuckResult<()> {
    if changed.is_empty() {
        return Ok(());
    }

    if changed.len() <= MAX_VISITOR_SCOPE {
        update_changed_visitors(conn, from_time, changed)
    } else {
        update_entity_events(conn, from_time, changed)
    }
}

fn update_changed_visitors(
    conn: &Connection,
    from_time: DateTime<Utc>,
    changed: &ahash::HashSet<(String, String)>,
) -> DuckResult<()> {
    let mut entities: Vec<&String> = Vec::new();
    let mut visitors: Vec<&String> = Vec::new();
    for (entity, visitor) in changed {
        if !entities.contains(&entity) {
            entities.push(entity);
        }
        visitors.push(visitor);
    }

    // `in (...)` lists rather than a join against a `values` relation: DuckDB
    // planned the join as a nested loop over every row in the window, which
    // cost more than the scan it was meant to narrow. Visitor ids already
    // include the entity in their hash input, and both lists are filtered, so
    // the result is the same.
    let entity_vars = repeat_vars(entities.len());
    let visitor_vars = repeat_vars(visitors.len());
    // The time bound is a constant so DuckDB can prune by `created_at` before
    // matching the visitors.
    let sql = format!("--sql
        with
            filtered_events as (
                select rowid, entity_id, visitor_group_id, created_at
                from events
                where event = 'pageview'
                    and entity_id in ({entity_vars})
                    and visitor_group_id in ({visitor_vars})
                    and created_at >= ?::timestamp - interval '30 minutes'
            ),
            cte as (
                select
                    rowid,
                    created_at - lag(created_at) over (partition by entity_id, visitor_group_id order by created_at, rowid) as time_from_last_event,
                    lead(created_at) over (partition by entity_id, visitor_group_id order by created_at, rowid) - created_at as time_to_next_event
                from filtered_events
            )
        update events
            set
                time_from_last_event = cte.time_from_last_event,
                time_to_next_event = cte.time_to_next_event
            from cte
            where events.rowid = cte.rowid;
    ");
    let mut params = ParamVec::new();
    params.extend(entities);
    params.extend(visitors);
    params.push(from_time);
    conn.execute(&sql, duckdb::params_from_iter(params))?;
    Ok(())
}

fn update_entity_events(
    conn: &Connection,
    from_time: DateTime<Utc>,
    changed: &ahash::HashSet<(String, String)>,
) -> DuckResult<()> {
    let mut entities: Vec<&String> = Vec::new();
    for (entity, _) in changed {
        if !entities.contains(&entity) {
            entities.push(entity);
        }
    }

    let entity_vars = repeat_vars(entities.len());
    let sql = format!("--sql
        with
            filtered_events as (
                select rowid, entity_id, visitor_group_id, created_at
                from events
                where event = 'pageview' and entity_id in ({entity_vars}) and created_at >= ?::timestamp - interval '30 minutes'
            ),
            cte as (
                select
                    rowid,
                    created_at - lag(created_at) over (partition by entity_id, visitor_group_id order by created_at, rowid) as time_from_last_event,
                    lead(created_at) over (partition by entity_id, visitor_group_id order by created_at, rowid) - created_at as time_to_next_event
                from filtered_events
            )
        update events
            set
                time_from_last_event = cte.time_from_last_event,
                time_to_next_event = cte.time_to_next_event
            from cte
            where events.rowid = cte.rowid;
    ");

    let mut params = ParamVec::new();
    params.extend(entities);
    params.push(from_time);
    conn.execute(&sql, duckdb::params_from_iter(params))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Liwan;
    use crate::config::Config;

    #[test]
    fn failed_append_does_not_update_cache() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        conn.execute_batch("create unique index unique_visitor on events(visitor_group_id)").unwrap();
        let entities = ["entity-1".into()];
        assert!(!app.events.custom_data(&entities).unwrap().events);
        let mut first = event(Utc::now());
        first.event = "signup".into();
        let second = first.clone();

        assert!(app.events.append([first, second].into_iter()).is_err());

        assert_eq!(conn.query_row("select count(*) from events", [], |row| row.get::<_, u64>(0)).unwrap(), 0);
        assert!(!app.events.custom_data(&entities).unwrap().events);
    }

    #[test]
    fn session_updates_identify_individual_rows() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let conn = app.events_conn().unwrap();
        conn.execute_batch(
            "insert into events (entity_id, visitor_group_id, event, created_at)
             values ('a', 'visitor', 'pageview', '2026-01-01 00:00:00'),
                    ('a', 'visitor', 'pageview', '2026-01-01 00:00:00'),
                    ('a', 'visitor', 'pageview', '2026-01-01 00:01:00'),
                    ('b', 'visitor', 'pageview', '2026-01-01 00:00:00'),
                    ('b', 'visitor', 'pageview', '2026-01-01 00:02:00'),
                    ('a', 'visitor', 'signup', '2026-01-01 00:00:30'),
                    ('c', 'visitor', 'pageview', '2026-01-01 00:00:00')",
        )
        .unwrap();
        let from: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
        let changed = [("a", "visitor"), ("b", "visitor")]
            .into_iter()
            .map(|(entity, visitor)| (entity.to_string(), visitor.to_string()))
            .collect();
        update_event_times(&conn, from, &changed).unwrap();
        let rows = conn
            .prepare("select epoch(time_from_last_event)::bigint, epoch(time_to_next_event)::bigint from events order by rowid")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)))
            .unwrap()
            .collect::<DuckResult<Vec<_>>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (None, Some(0)),
                (Some(0), Some(60)),
                (Some(60), None),
                (None, Some(120)),
                (Some(120), None),
                (None, None),
                (None, None)
            ]
        );
    }

    fn event(created_at: DateTime<Utc>) -> Event {
        Event {
            entity_id: "entity-1".to_string(),
            visitor_group_id: "visitor-1".to_string(),
            event: "pageview".to_string(),
            created_at,
            fqdn: Some("example.com".to_string()),
            path: Some("/docs".to_string()),
            referrer: None,
            platform: None,
            browser: None,
            mobile: None,
            country: None,
            city: None,
            utm_source: None,
            utm_medium: None,
            utm_campaign: None,
            utm_content: None,
            utm_term: None,
            screen_width: None,
            orientation: None,
            properties: Default::default(),
            track_sessions: true,
        }
    }

    #[test]
    fn exit_updates_only_the_latest_event_without_a_next_event() {
        let app = Liwan::new_memory(Config::default()).expect("failed to create app");
        let first = Utc::now() - chrono::Duration::minutes(2);
        let second = first + chrono::Duration::minutes(1);
        let mut custom = event(second + chrono::Duration::seconds(5));
        custom.event = "signup".into();
        app.events.append(vec![event(first), event(second), custom].into_iter()).expect("failed to append events");

        let conn = app.events_conn().expect("failed to get event connection");
        let matched = app
            .events
            .update_exits(vec![EventExit {
                entity_id: "entity-1".to_string(),
                visitor_group_id: "visitor-1".to_string(),
                event: "pageview".to_string(),
                created_at: second + chrono::Duration::seconds(15),
                fqdn: Some("example.com".to_string()),
                path: Some("/docs".to_string()),
            }])
            .expect("failed to update exit");

        assert_eq!(matched, 1);
        let rows = conn
            .prepare("select exited_at from events order by created_at")
            .expect("failed to prepare query")
            .query_map([], |row| row.get::<_, Option<DateTime<Utc>>>(0))
            .expect("failed to query exits")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("failed to collect exits");
        assert_eq!(
            rows[1].map(|value| value.timestamp_millis()),
            Some((second + chrono::Duration::seconds(15)).timestamp_millis())
        );
        assert_eq!(rows[0], None);
        assert_eq!(rows[2], None);
    }

    #[test]
    fn custom_events_do_not_change_pageview_intervals() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let first = Utc::now() - chrono::Duration::minutes(3);
        let middle = first + chrono::Duration::minutes(1);
        let last = middle + chrono::Duration::minutes(1);
        let mut custom = event(middle);
        custom.event = "signup".into();
        app.events.append(vec![event(first), custom, event(last)].into_iter()).unwrap();

        let conn = app.events_conn().unwrap();
        let rows = conn
            .prepare("select event, time_from_last_event, time_to_next_event from events order by created_at")
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<chrono::Duration>>(1)?,
                    row.get::<_, Option<chrono::Duration>>(2)?,
                ))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows[0].2, Some(chrono::Duration::seconds(120)));
        assert_eq!(rows[1], ("signup".into(), None, None));
        assert_eq!(rows[2].1, Some(chrono::Duration::seconds(120)));
    }

    #[test]
    fn a_gap_longer_than_a_session_leaves_intervals_null() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let first = Utc::now() - chrono::Duration::minutes(50);
        let second = first + chrono::Duration::minutes(45);
        app.events.append(vec![event(first)].into_iter()).unwrap();
        app.events.append(vec![event(second)].into_iter()).unwrap();

        let conn = app.events_conn().unwrap();
        let rows = conn
            .prepare("select epoch(time_from_last_event)::bigint, epoch(time_to_next_event)::bigint from events order by created_at")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)))
            .unwrap()
            .collect::<DuckResult<Vec<_>>>()
            .unwrap();
        // A gap wider than the 30-minute session window is left null, which the
        // reports treat exactly like a gap longer than 30 minutes.
        assert_eq!(rows, vec![(None, None), (None, None)]);
    }

    #[test]
    fn large_batches_fall_back_to_the_entity_scan() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let start = Utc::now() - chrono::Duration::minutes(5);
        let mut events = vec![event(start), event(start + chrono::Duration::seconds(60))];
        for visitor in 0..MAX_VISITOR_SCOPE {
            events.push(Event {
                visitor_group_id: format!("bulk-{visitor}"),
                created_at: start + chrono::Duration::seconds(1),
                ..event(start)
            });
        }
        assert!(events.len() > MAX_VISITOR_SCOPE);

        app.events.append(events.into_iter()).unwrap();

        let conn = app.events_conn().unwrap();
        let rows = conn
            .prepare(
                "select epoch(time_from_last_event)::bigint, epoch(time_to_next_event)::bigint
                 from events where visitor_group_id = 'visitor-1' order by created_at",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)))
            .unwrap()
            .collect::<DuckResult<Vec<_>>>()
            .unwrap();
        assert_eq!(rows, vec![(None, Some(60)), (Some(60), None)]);
    }

    #[test]
    fn several_exits_are_committed_together() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let created_at = Utc::now() - chrono::Duration::minutes(2);
        let events = (0..3)
            .map(|visitor| Event { visitor_group_id: format!("exit-{visitor}"), ..event(created_at) })
            .collect::<Vec<_>>();
        app.events.append(events.into_iter()).unwrap();

        let exits = (0..3)
            .map(|visitor| EventExit {
                entity_id: "entity-1".to_string(),
                visitor_group_id: format!("exit-{visitor}"),
                event: "pageview".to_string(),
                created_at: created_at + chrono::Duration::seconds(30),
                fqdn: Some("example.com".to_string()),
                path: Some("/docs".to_string()),
            })
            .collect::<Vec<_>>();
        assert_eq!(app.events.update_exits(exits).unwrap(), 3);

        let conn = app.events_conn().unwrap();
        let exited: u64 =
            conn.query_row("select count(*) from events where exited_at is not null", [], |row| row.get(0)).unwrap();
        assert_eq!(exited, 3);
    }

    #[test]
    fn properties_round_trip_through_appender() {
        let app = Liwan::new_memory(Config::default()).unwrap();
        let properties: crate::app::models::EventProperties = [
            ("plan", "pro"),
            ("delimiters", "a=b, {c}: [d]"),
            ("quotes", "it's \"quoted\""),
            ("backslash", "C:\\path\\"),
            ("  spaced  ", "  padded  "),
            ("unicode", "grüße 🦆"),
            ("empty", ""),
            ("null", "NULL"),
            ("k'ey=", "v\\'"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
        let mut with_properties = event(Utc::now());
        with_properties.properties = properties.clone();
        app.events.append(vec![with_properties, event(Utc::now())].into_iter()).unwrap();

        let conn = app.events_conn().unwrap();
        let stored = conn
            .prepare("select unnest(map_keys(properties)), unnest(map_values(properties)) from events")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<crate::app::models::EventProperties, _>>()
            .unwrap();
        assert_eq!(stored, properties);
        let without: u64 =
            conn.query_row("select count(*) from events where properties is null", [], |row| row.get(0)).unwrap();
        assert_eq!(without, 1);
    }
}
