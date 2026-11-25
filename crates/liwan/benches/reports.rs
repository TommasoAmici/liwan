use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use jiff::{SignedDuration, Timestamp};
use liwan::app::Liwan;
use liwan::app::models::{FunnelMatch, FunnelStep, UtmTag};
use liwan::app::reports::{self, DateRange, Dimension, GraphInterval, Metric};
use liwan::config::Config;
use std::time::Duration;

fn configure_group(group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>) {
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);
}

/// Twenty tags built from the values `cargo dev seed` writes, including partial
/// tags and two that match nothing, as a project's list would look.
fn benchmark_utm_tags() -> Vec<UtmTag> {
    let rows: [(&str, &str, &str, &str, &str); 20] = [
        ("google", "cpc", "summer_sale", "banner", "liwan"),
        ("google", "cpc", "black_friday", "sidebar", "analytics"),
        ("bing", "organic", "christmas", "footer", "tracking"),
        ("facebook", "referral", "new_year", "popup", "web"),
        ("twitter", "email", "summer_sale", "banner", "liwan"),
        ("google", "", "", "", ""),
        ("bing", "", "", "", ""),
        ("facebook", "", "", "", ""),
        ("twitter", "", "", "", ""),
        ("google", "cpc", "", "", ""),
        ("bing", "organic", "", "", ""),
        ("facebook", "referral", "", "", ""),
        ("twitter", "email", "", "", ""),
        ("", "cpc", "summer_sale", "", ""),
        ("", "", "black_friday", "", ""),
        ("", "", "christmas", "banner", ""),
        ("", "", "", "sidebar", ""),
        ("", "", "", "", "liwan"),
        ("google", "cpc", "summer_sale", "", "shoes"),
        ("", "", "winter_sale", "", ""),
    ];
    let text = |value: &str| Some(value.to_string()).filter(|value| !value.is_empty());
    rows.iter()
        .enumerate()
        .map(|(index, (source, medium, campaign, content, term))| UtmTag {
            id: format!("tag-{index:02}"),
            project_id: "benchmark".to_string(),
            display_name: format!("Campaign {index:02}"),
            url: "https://example.com/landing".to_string(),
            utm_source: text(source),
            utm_medium: text(medium),
            utm_campaign: text(campaign),
            utm_content: text(content),
            utm_term: text(term),
        })
        .collect()
}

fn benchmark_reports(c: &mut Criterion) {
    let config = Config::load(None, std::env::vars()).expect("failed to load config");
    let app = Liwan::try_new(config).expect("failed to initialize app");

    let project = app
        .projects
        .all()
        .expect("failed to load projects")
        .into_iter()
        .next()
        .expect("no projects found; seed the database first");

    let entities = app.projects.entity_ids(&project.id).expect("failed to resolve entities for benchmark project");
    assert!(!entities.is_empty(), "project has no entities; seed the database first");

    let range = DateRange {
        start: Timestamp::now().checked_sub(SignedDuration::from_hours(8760)).expect("failed to build range start"),
        end: Timestamp::now(),
    };
    let day_buckets = reports::build_graph_buckets(&range, GraphInterval::Day, Some("UTC"), 2000)
        .expect("failed to build day buckets for benchmark");
    let ranges = [range];
    let buckets = [day_buckets];

    let conn = app.events_conn().expect("failed to get events connection");

    {
        let mut group = c.benchmark_group("report_meta");
        configure_group(&mut group);
        group.bench_function("earliest_timestamp", |b| {
            b.iter(|| reports::earliest_timestamp(&conn, &entities).expect("earliest_timestamp failed"));
        });
        group.finish();
    }

    {
        let mut group = c.benchmark_group("comparison_reports");
        configure_group(&mut group);
        let end = ranges[0].end;
        let start = end.checked_sub(SignedDuration::from_hours(48)).expect("failed to build comparison range");
        let comparison_ranges = [
            DateRange { start, end },
            DateRange {
                start: start.checked_sub(SignedDuration::from_hours(8760)).expect("failed to shift range start"),
                end: end.checked_sub(SignedDuration::from_hours(8760)).expect("failed to shift range end"),
            },
        ];
        let comparison_buckets = comparison_ranges
            .iter()
            .map(|range| {
                reports::build_graph_buckets(range, GraphInterval::Day, Some("UTC"), 2000)
                    .expect("failed to build comparison buckets")
            })
            .collect::<Vec<_>>();
        group.bench_function("graph", |b| {
            b.iter(|| {
                reports::overall_report(
                    &conn,
                    &entities,
                    "pageview",
                    &comparison_ranges,
                    &comparison_buckets,
                    &[],
                    &Metric::Views,
                )
                .expect("comparison graph failed")
            })
        });
        group.bench_function("stats", |b| {
            b.iter(|| {
                reports::overall_stats(&conn, &entities, "pageview", &comparison_ranges, &[], &app.config.limits)
                    .expect("comparison stats failed")
            })
        });
        group.bench_function("dimension", |b| {
            b.iter(|| {
                reports::dimension_report(
                    &conn,
                    &entities,
                    "pageview",
                    &comparison_ranges,
                    &Dimension::Url,
                    None,
                    &[],
                    &Metric::Views,
                    1000,
                    0,
                )
                .expect("comparison dimension failed")
            })
        });
        group.bench_function("custom_events", |b| {
            b.iter(|| {
                reports::custom_events_report(&conn, &entities, &comparison_ranges, &[], 100)
                    .expect("comparison custom events failed")
            })
        });
        group.finish();
    }

    {
        let mut group = c.benchmark_group("overall_report");
        configure_group(&mut group);
        for metric in [Metric::Views, Metric::UniqueVisitors, Metric::BounceRate, Metric::AvgTimeOnSite] {
            group.bench_with_input(BenchmarkId::new("metric", format!("{metric:?}")), &metric, |b, metric| {
                b.iter(|| {
                    reports::overall_report(&conn, &entities, "pageview", &ranges, &buckets, &[], metric)
                        .expect("overall_report failed")
                });
            });
        }
        group.finish();
    }

    {
        let mut group = c.benchmark_group("overall_stats");
        configure_group(&mut group);
        group.bench_function("all_metrics", |b| {
            b.iter(|| {
                reports::overall_stats(&conn, &entities, "pageview", &ranges, &[], &app.config.limits)
                    .expect("overall_stats failed")
            });
        });
        group.finish();
    }

    {
        let mut group = c.benchmark_group("custom_events_report");
        configure_group(&mut group);
        group.bench_function("counts", |b| {
            b.iter(|| {
                reports::custom_events_report(&conn, &entities, &ranges, &[], 100).expect("custom_events_report failed")
            });
        });
        group.finish();
    }

    {
        let mut group = c.benchmark_group("utm_tag_stats");
        configure_group(&mut group);
        let tags = benchmark_utm_tags();
        group.bench_function("20_tags", |b| {
            b.iter(|| reports::utm_tag_summary(&conn, &entities, &tags, &ranges[0]).expect("utm_tag_summary failed"));
        });
        group.finish();
    }

    {
        let mut group = c.benchmark_group("funnel_report");
        configure_group(&mut group);
        let two_step = [
            FunnelStep::Page { path: "/pricing".into(), match_type: FunnelMatch::Equal },
            FunnelStep::Event { name: "signup".into() },
        ];
        let three_step = [
            FunnelStep::Page { path: "/pricing".into(), match_type: FunnelMatch::Equal },
            FunnelStep::Event { name: "signup".into() },
            FunnelStep::Event { name: "purchase".into() },
        ];
        group.bench_function("two_step", |b| {
            b.iter(|| {
                reports::funnel_report(&conn, &entities, &two_step, &ranges, &[]).expect("two-step funnel failed")
            })
        });
        group.bench_function("three_step", |b| {
            b.iter(|| {
                reports::funnel_report(&conn, &entities, &three_step, &ranges, &[]).expect("three-step funnel failed")
            })
        });
        group.finish();
    }

    {
        let mut group = c.benchmark_group("dimension_report");
        configure_group(&mut group);
        let dimension = Dimension::Url;
        for metric in [Metric::Views, Metric::UniqueVisitors, Metric::BounceRate, Metric::AvgTimeOnSite] {
            group.bench_with_input(
                BenchmarkId::new("dim_metric", format!("{dimension:?}/{metric:?}")),
                &metric,
                |b, metric| {
                    b.iter(|| {
                        reports::dimension_report(
                            &conn,
                            &entities,
                            "pageview",
                            &ranges,
                            &dimension,
                            None,
                            &[],
                            metric,
                            1000,
                            0,
                        )
                        .expect("dimension_report failed")
                    });
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, benchmark_reports);
criterion_main!(benches);
