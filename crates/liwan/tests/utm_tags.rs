mod common;
use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::json;

fn admin_headers(cookies: &[axum_extra::extract::cookie::Cookie<'static>]) -> Vec<(String, String)> {
    vec![("cookie".into(), common::cookie_header(cookies))]
}

fn tag_body(name: &str) -> serde_json::Value {
    json!({
        "displayName": name,
        "url": "https://example.com/landing?ref=newsletter",
        "utmSource": "newsletter",
        "utmMedium": "email",
        "utmCampaign": "spring_sale",
        "utmContent": null,
        "utmTerm": null,
    })
}

#[tokio::test]
async fn admin_manages_utm_tags() -> Result<()> {
    let app = common::app();
    let (queues, _receivers) = common::events();
    let client = common::TestClient::new(app.clone(), queues);
    app.seed_database(0)?;
    let headers = admin_headers(&common::login(&client, "admin", "admin").await);
    let path = "/api/dashboard/project/public-project/utm-tags";

    let created = client.post_with_headers(path, tag_body("  Spring sale "), headers.clone()).await;
    created.assert_status_success();
    let created: serde_json::Value = created.json();
    assert_eq!(created["displayName"], "Spring sale");
    assert_eq!(created["url"], "https://example.com/landing?ref=newsletter");
    assert_eq!(created["utmSource"], "newsletter");
    assert_eq!(created["utmCampaign"], "spring_sale");
    assert_eq!(created["utmContent"], serde_json::Value::Null);
    let id = created["id"].as_str().expect("created tag id").to_string();
    assert_eq!(id.len(), 16);

    let list = client.get_with_headers(path, headers.clone()).await;
    list.assert_status_success();
    let list: serde_json::Value = list.json();
    assert_eq!(list["utmTags"].as_array().map(Vec::len), Some(1));
    assert_eq!(list["utmTags"][0]["id"], id);

    let updated = client
        .put_with_headers(
            &format!("{path}/{id}"),
            json!({
                "displayName": "Summer sale",
                "url": "https://example.com/summer",
                "utmSource": "google",
                "utmMedium": "cpc",
                "utmCampaign": "summer_sale",
                "utmContent": null,
                "utmTerm": "shoes",
            }),
            headers.clone(),
        )
        .await;
    updated.assert_status_success();
    let updated: serde_json::Value = updated.json();
    assert_eq!(updated["id"], id);
    assert_eq!(updated["displayName"], "Summer sale");
    assert_eq!(updated["utmTerm"], "shoes");

    let list = client.get_with_headers(path, headers.clone()).await;
    let list: serde_json::Value = list.json();
    assert_eq!(list["utmTags"][0]["displayName"], "Summer sale");

    let deleted = client.delete_with_headers(&format!("{path}/{id}"), headers.clone()).await;
    deleted.assert_status_success();
    let list = client.get_with_headers(path, headers.clone()).await;
    let list: serde_json::Value = list.json();
    assert!(list["utmTags"].as_array().unwrap().is_empty());
    assert!(app.utm_tags.list("public-project")?.is_empty());

    Ok(())
}

#[tokio::test]
async fn invalid_tags_are_rejected_with_a_message() -> Result<()> {
    let app = common::app();
    let (queues, _receivers) = common::events();
    let client = common::TestClient::new(app.clone(), queues);
    app.seed_database(0)?;
    let headers = admin_headers(&common::login(&client, "admin", "admin").await);
    let path = "/api/dashboard/project/public-project/utm-tags";

    for (body, expected) in [
        (json!({ "displayName": "  ", "url": "https://example.com", "utmSource": "newsletter" }), "Name"),
        (json!({ "displayName": "Tag", "url": "example.com", "utmSource": "newsletter" }), "URL"),
        (json!({ "displayName": "Tag", "url": "ftp://example.com", "utmSource": "newsletter" }), "http"),
        (json!({ "displayName": "Tag", "url": "https://example.com" }), "UTM"),
        (json!({ "displayName": "Tag", "url": "https://example.com", "utmSource": "a".repeat(201) }), "Source"),
    ] {
        let response = client.post_with_headers(path, body.clone(), headers.clone()).await;
        response.assert_status_bad_request();
        let response: serde_json::Value = response.json();
        assert!(
            response["message"].as_str().is_some_and(|message| message.contains(expected)),
            "message for {body} should mention {expected}, got {response}"
        );
    }

    let created = client.post_with_headers(path, tag_body("Kept"), headers.clone()).await;
    created.assert_status_success();
    let created: serde_json::Value = created.json();
    let id = created["id"].as_str().unwrap();
    let response = client
        .put_with_headers(
            &format!("{path}/{id}"),
            json!({ "displayName": "Broken", "url": "https://example.com" }),
            headers.clone(),
        )
        .await;
    response.assert_status_bad_request();
    let list: serde_json::Value = client.get_with_headers(path, headers).await.json();
    assert_eq!(list["utmTags"][0]["displayName"], "Kept");

    Ok(())
}

#[tokio::test]
async fn tags_are_scoped_to_one_project() -> Result<()> {
    let app = common::app();
    let (queues, _receivers) = common::events();
    let client = common::TestClient::new(app.clone(), queues);
    app.seed_database(0)?;
    let headers = admin_headers(&common::login(&client, "admin", "admin").await);

    let created = client
        .post_with_headers("/api/dashboard/project/private-project/utm-tags", tag_body("Private"), headers.clone())
        .await;
    created.assert_status_success();
    let created: serde_json::Value = created.json();
    let id = created["id"].as_str().unwrap().to_string();

    let public_path = format!("/api/dashboard/project/public-project/utm-tags/{id}");
    client.put_with_headers(&public_path, tag_body("Hijacked"), headers.clone()).await.assert_status_not_found();
    client.delete_with_headers(&public_path, headers.clone()).await.assert_status_not_found();

    let list: serde_json::Value =
        client.get_with_headers("/api/dashboard/project/public-project/utm-tags", headers.clone()).await.json();
    assert!(list["utmTags"].as_array().unwrap().is_empty());
    let list: serde_json::Value =
        client.get_with_headers("/api/dashboard/project/private-project/utm-tags", headers).await.json();
    assert_eq!(list["utmTags"][0]["displayName"], "Private");

    Ok(())
}

#[tokio::test]
async fn readers_follow_project_visibility() -> Result<()> {
    let app = common::app();
    let (queues, _receivers) = common::events();
    let client = common::TestClient::new(app.clone(), queues);
    app.seed_database(0)?;
    let admin = admin_headers(&common::login(&client, "admin", "admin").await);
    client
        .post_with_headers("/api/dashboard/project/public-project/utm-tags", tag_body("Public"), admin.clone())
        .await
        .assert_status_success();
    client
        .post_with_headers("/api/dashboard/project/private-project/utm-tags", tag_body("Private"), admin)
        .await
        .assert_status_success();

    // Anonymous visitors read a public project and cannot see a private one.
    let response = client.get("/api/dashboard/project/public-project/utm-tags").await;
    response.assert_status_success();
    let body: serde_json::Value = response.json();
    assert_eq!(body["utmTags"][0]["displayName"], "Public");
    client.get("/api/dashboard/project/private-project/utm-tags").await.assert_status_not_found();

    // A signed-in non-admin with no grant is treated the same as an anonymous visitor.
    let user = admin_headers(&common::login(&client, "user", "user").await);
    client
        .get_with_headers("/api/dashboard/project/public-project/utm-tags", user.clone())
        .await
        .assert_status_success();
    client
        .get_with_headers("/api/dashboard/project/private-project/utm-tags", user.clone())
        .await
        .assert_status_not_found();
    client.get("/api/dashboard/project/missing/utm-tags").await.assert_status_not_found();

    Ok(())
}

#[tokio::test]
async fn mutations_require_an_administrator() -> Result<()> {
    let app = common::app();
    let (queues, _receivers) = common::events();
    let client = common::TestClient::new(app.clone(), queues);
    app.seed_database(0)?;
    let path = "/api/dashboard/project/public-project/utm-tags";
    let admin = admin_headers(&common::login(&client, "admin", "admin").await);
    let created: serde_json::Value = client.post_with_headers(path, tag_body("Tag"), admin.clone()).await.json();
    let tag_path = format!("{path}/{}", created["id"].as_str().unwrap());

    let user = admin_headers(&common::login(&client, "user", "user").await);
    client.post_with_headers(path, tag_body("Denied"), user.clone()).await.assert_status_forbidden();
    client.put_with_headers(&tag_path, tag_body("Denied"), user.clone()).await.assert_status_forbidden();
    client.delete_with_headers(&tag_path, user.clone()).await.assert_status_forbidden();

    client.post(path, tag_body("Anonymous")).await.assert_status_unauthorized();
    client.put_with_headers(&tag_path, tag_body("Anonymous"), vec![]).await.assert_status_unauthorized();
    client.delete_with_headers(&tag_path, vec![]).await.assert_status_unauthorized();

    client
        .post_with_headers("/api/dashboard/project/missing/utm-tags", tag_body("Tag"), admin)
        .await
        .assert_status_not_found();

    Ok(())
}

#[tokio::test]
async fn deleting_a_project_removes_its_tags() -> Result<()> {
    let app = common::app();
    let (queues, _receivers) = common::events();
    let client = common::TestClient::new(app.clone(), queues);
    app.seed_database(0)?;
    let headers = admin_headers(&common::login(&client, "admin", "admin").await);
    client
        .post_with_headers("/api/dashboard/project/public-project/utm-tags", tag_body("Tag"), headers.clone())
        .await
        .assert_status_success();
    assert_eq!(app.utm_tags.list("public-project")?.len(), 1);

    client.delete_with_headers("/api/dashboard/project/public-project", headers).await.assert_status_success();
    assert!(app.utm_tags.list("public-project")?.is_empty());

    Ok(())
}

#[tokio::test]
async fn summary_counts_pageviews_and_visitors_per_tag() -> Result<()> {
    let app = common::app();
    let (queues, _receivers) = common::events();
    let client = common::TestClient::new(app.clone(), queues);
    app.seed_database(0)?;
    let headers = admin_headers(&common::login(&client, "admin", "admin").await);
    let path = "/api/dashboard/project/public-project/utm-tags";
    let time = "2026-01-01T12:00:00Z".parse::<chrono::DateTime<Utc>>()?;
    for (visitor, event, source, medium, campaign) in [
        ("visitor-1", "pageview", Some("google"), Some("cpc"), Some("summer_sale")),
        ("visitor-2", "pageview", Some("google"), Some("cpc"), Some("summer_sale")),
        ("visitor-1", "pageview", Some("newsletter"), None, None),
        ("visitor-3", "pageview", Some("newsletter"), Some("email"), None),
        // Same source, different campaign: a partial tag matches both.
        ("visitor-6", "pageview", Some("google"), Some("cpc"), Some("winter_sale")),
        // Custom events never count as pageviews.
        ("visitor-4", "signup", Some("google"), Some("cpc"), Some("summer_sale")),
        // Outside the reported range.
        ("visitor-5", "pageview", Some("google"), Some("cpc"), Some("summer_sale")),
    ] {
        let created_at = if visitor == "visitor-5" { time - Duration::days(5) } else { time };
        app.events_conn()?.execute(
            "insert into events (entity_id, visitor_group_id, event, created_at, utm_source, utm_medium, utm_campaign)
             values ('entity-1', ?, ?, ?, ?, ?, ?)",
            duckdb::params![visitor, event, created_at, source, medium, campaign],
        )?;
    }

    let mut ids = Vec::new();
    for (name, body) in [
        (
            "All five",
            json!({ "displayName": "All five", "url": "https://example.com", "utmSource": "google", "utmMedium": "cpc",
                "utmCampaign": "summer_sale", "utmContent": null, "utmTerm": null }),
        ),
        ("Source only", json!({ "displayName": "Source only", "url": "https://example.com", "utmSource": "google" })),
        (
            "Exact",
            json!({ "displayName": "Exact", "url": "https://example.com", "utmSource": "newsletter", "utmMedium": "email" }),
        ),
        ("No traffic", json!({ "displayName": "No traffic", "url": "https://example.com", "utmCampaign": "winter" })),
    ] {
        let created = client.post_with_headers(path, body, headers.clone()).await;
        created.assert_status_success();
        let created: serde_json::Value = created.json();
        assert_eq!(created["displayName"], name);
        ids.push(created["id"].as_str().unwrap().to_string());
    }

    let response = client
        .post_with_headers(
            &format!("{path}/summary"),
            json!({ "range": { "start": "2026-01-01T00:00:00Z", "end": "2026-01-02T00:00:00Z" } }),
            headers.clone(),
        )
        .await;
    response.assert_status_success();
    let body: serde_json::Value = response.json();
    let summaries = body["summaries"].as_array().expect("summaries array");
    assert_eq!(summaries.len(), 4);
    let summarised = |index: usize| -> (u64, u64) {
        let row = summaries
            .iter()
            .find(|row| row["id"] == ids[index])
            .unwrap_or_else(|| panic!("missing summary for {}", ids[index]));
        (row["totalViews"].as_u64().unwrap(), row["uniqueVisitors"].as_u64().unwrap())
    };
    assert_eq!(summarised(0), (2, 2), "all five values");
    assert_eq!(summarised(1), (3, 3), "source only matches every google pageview");
    assert_eq!(summarised(2), (1, 1), "two values");
    assert_eq!(summarised(3), (0, 0), "no traffic still has a row");

    // A range that ends before it starts is rejected like every other report.
    client
        .post_with_headers(
            &format!("{path}/summary"),
            json!({ "range": { "start": "2026-01-02T00:00:00Z", "end": "2026-01-01T00:00:00Z" } }),
            headers.clone(),
        )
        .await
        .assert_status_bad_request();

    // Readers who cannot see the project cannot ask for its summary either.
    client
        .post(
            "/api/dashboard/project/private-project/utm-tags/summary",
            json!({ "range": { "start": "2026-01-01T00:00:00Z", "end": "2026-01-02T00:00:00Z" } }),
        )
        .await
        .assert_status_not_found();

    Ok(())
}
