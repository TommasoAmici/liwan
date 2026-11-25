use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main};
use jiff::Timestamp;
use liwan::app::Liwan;
use liwan::app::models::{Event, EventExit};
use liwan::config::Config;
use liwan::utils::useragent;
use liwan::utils::visitor_group_id;

const ENTITY: &str = "bench-entity";
const BATCH_SIZE: usize = 512;
const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 13_2_3 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/13.0.3 Mobile/15E148 Safari/604.1";

fn configure_group(group: &mut BenchmarkGroup<'_, criterion::measurement::WallTime>) {
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);
}

fn live_app() -> Arc<Liwan> {
    Liwan::new_memory(Config::default()).expect("failed to create in-memory app")
}

/// One million pageviews for a single entity over the last 24 hours, grouped in
/// ten-event sessions. The last event of each session has no next event, which
/// stands for a browser session that has not ended yet.
///
/// The 512 visitors the writer benchmark appends for get three pageviews a few
/// minutes before the batch, the last one without a next event, so every
/// iteration has a previous event to fill.
fn seed_day(app: &Liwan) {
    let conn = app.events_conn().expect("failed to get events connection");
    conn.execute(
        "insert into events (entity_id, visitor_group_id, event, created_at, fqdn, path, time_from_last_event, time_to_next_event)
         select ?1, 'visitor-' || (i / 10), 'pageview', now()::timestamp - (i % 86400) * interval '1 second',
             'example.com', '/page/' || (i % 20),
             case when i % 10 = 0 then null else interval '10 seconds' end,
             case when i % 10 = 9 then null else interval '10 seconds' end
         from range(1000000) t(i)",
        duckdb::params![ENTITY],
    )
    .expect("failed to seed the benchmark day");

    conn.execute(
        "insert into events (entity_id, visitor_group_id, event, created_at, path, time_from_last_event, time_to_next_event)
         select ?1, 'bench-visitor-' || j, 'pageview', now()::timestamp - (3 - p) * interval '1 minute', '/before-batch',
             case when p > 1 then interval '1 minute' else null end,
             case when p < 3 then interval '1 minute' else null end
         from range(512) t(j), range(1, 4) u(p)",
        duckdb::params![ENTITY],
    )
    .expect("failed to insert the previous events");
}

fn batch_events(track_sessions: bool) -> Vec<Event> {
    let created_at = Timestamp::now();
    (0..BATCH_SIZE)
        .map(|visitor| Event {
            entity_id: ENTITY.to_string(),
            visitor_group_id: format!("bench-visitor-{visitor}"),
            event: "pageview".to_string(),
            created_at,
            fqdn: Some("example.com".to_string()),
            path: Some("/batch".to_string()),
            track_sessions,
            ..Default::default()
        })
        .collect()
}

fn bench_writer(c: &mut Criterion) {
    let app = live_app();
    seed_day(&app);

    let mut group = c.benchmark_group("ingest");
    configure_group(&mut group);
    group.throughput(Throughput::Elements(BATCH_SIZE as u64));
    group.bench_function("writer", |b| {
        b.iter_batched(
            || {
                // Drop the previous iteration's batch so the last 30 minutes of
                // events stays the same size every iteration; otherwise the
                // window grows with the number of samples and the benchmark
                // measures its own history.
                app.events_conn()
                    .expect("failed to get events connection")
                    .execute("delete from events where path = '/batch'", [])
                    .expect("failed to reset the previous batch");
                batch_events(true)
            },
            |events| app.events.append(events.into_iter()).expect("failed to append the benchmark batch"),
            BatchSize::SmallInput,
        );
    });
    // The same append with the session update switched off, to show how much of
    // the writer is the appender and how much is the interval query.
    group.bench_function("writer_without_session_update", |b| {
        b.iter_batched(
            || {
                app.events_conn()
                    .expect("failed to get events connection")
                    .execute("delete from events where path = '/batch'", [])
                    .expect("failed to reset the previous batch");
                batch_events(false)
            },
            |events| app.events.append(events.into_iter()).expect("failed to append the benchmark batch"),
            BatchSize::SmallInput,
        );
    });
    group.finish();

    let conn = app.events_conn().expect("failed to get events connection");
    let total: u64 = conn.query_row("select count(*) from events", [], |row| row.get(0)).expect("count failed");
    assert!(total >= 1_000_000, "the benchmark lost its seed data: {total}");
    // A filtered run (for example `-- extract_utm`) skips the writer case, so
    // only validate the appended batch when one was appended.
    let appended: u64 = conn
        .query_row("select count(*) from events where path = '/batch'", [], |row| row.get(0))
        .expect("batch count failed");
    if appended > 0 {
        assert!(appended >= BATCH_SIZE as u64, "the benchmark batch went missing: {appended}");
        let filled: u64 = conn
            .query_row(
                "select count(*) from events where path = '/before-batch' and time_to_next_event is not null",
                [],
                |row| row.get(0),
            )
            .expect("interval check failed");
        assert!(filled > 0, "the append did not fill any previous event's next interval");
    }
}

fn bench_exits(c: &mut Criterion) {
    let app = live_app();
    let conn = app.events_conn().expect("failed to get events connection");
    conn.execute(
        "insert into events (entity_id, visitor_group_id, event, created_at, fqdn, path)
         select ?1, 'exit-visitor-' || i, 'pageview', now()::timestamp - interval '2 minutes', 'example.com', '/exit'
         from range(100) t(i)",
        duckdb::params![ENTITY],
    )
    .expect("failed to insert exit pageviews");

    let exits = (0..100)
        .map(|visitor| EventExit {
            entity_id: ENTITY.to_string(),
            visitor_group_id: format!("exit-visitor-{visitor}"),
            event: "pageview".to_string(),
            created_at: Timestamp::now(),
            fqdn: Some("example.com".to_string()),
            path: Some("/exit".to_string()),
        })
        .collect::<Vec<_>>();

    let mut group = c.benchmark_group("ingest");
    configure_group(&mut group);
    group.bench_function("exits", |b| {
        b.iter(|| {
            let matched = app.events.update_exits(exits.clone()).expect("failed to update exits");
            std::hint::black_box(matched)
        });
    });
    group.finish();

    assert_eq!(app.events.update_exits(exits).expect("failed to update exits"), 100);
}

fn bench_request_helpers(c: &mut Criterion) {
    use url::Url;

    let ip: IpAddr = "203.0.113.10".parse().expect("valid ip");
    let allowed = ["example.com".to_string(), "*.example.org".to_string()];
    let tagged_url = Url::parse(
        "https://example.com/pricing?utm_source=newsletter&utm_medium=email&utm_campaign=spring&utm_content=cta&utm_term=analytics&source=ignored&ref=ignored",
    )
    .expect("valid url");

    // Warm the classification cache so the benchmark measures the hit path,
    // which is what a busy server sees.
    let _ = useragent::classify(USER_AGENT);

    let mut group = c.benchmark_group("ingest");
    configure_group(&mut group);
    group.bench_function("classify_user_agent", |b| {
        b.iter(|| std::hint::black_box(useragent::classify(std::hint::black_box(USER_AGENT))));
    });
    group.bench_function("extract_utm", |b| {
        b.iter(|| std::hint::black_box(liwan::web::routes::event::extract_utm(std::hint::black_box(&tagged_url))));
    });
    group.bench_function("visitor_group_id", |b| {
        b.iter(|| {
            std::hint::black_box(visitor_group_id(
                std::hint::black_box(&ip),
                std::hint::black_box(USER_AGENT),
                "daily-salt",
                ENTITY,
            ))
        });
    });
    group.bench_function("hostname_allowed_empty", |b| {
        b.iter(|| std::hint::black_box(liwan::app::models::hostname_allowed("example.com", &[])));
    });
    group.bench_function("hostname_allowed_matching", |b| {
        b.iter(|| std::hint::black_box(liwan::app::models::hostname_allowed("www.example.org", &allowed)));
    });
    group.finish();
}

fn benchmarks(c: &mut Criterion) {
    bench_writer(c);
    bench_exits(c);
    bench_request_helpers(c);
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
